"""IndexTTS 2.5 日语角色 LoRA：原文/原音频校验、特征、训练与独立权重。

不依赖社区训练器的旧 BPE/二次移位损失；直接使用官方 2.5 tokenizer、
CAMPPlus、语义 codec 和语言嵌入。基础模型和原始素材始终只读。
"""
import gc
import hashlib
import json
import math
import os
from pathlib import Path
import random
import re
import shutil
import sys
from common import checked_path, emit, new_run, progress, separate_output, write_json
from extract import read_manifest, digest
from dataset import select_samples


def prepare(runtime, request, root, torch):
    import numpy as np
    import soundfile as sf
    import torchaudio
    from omegaconf import OmegaConf
    from transformers import SeamlessM4TFeatureExtractor, Wav2Vec2BertModel
    from indextts.codec.models import EnhancedCodec
    from indextts.s2mel.modules.campplus.DTDNN import CAMPPlus
    from indextts.utils.tokenizer import get_tokenizer
    from indextts.utils.ja_g2p import JapaneseG2PProcessor
    from indextts.utils.nemo_tn import normalize_text

    data = read_manifest(request['manifest'])
    samples, excluded = select_samples(data, request['speaker'], request['language'])
    # 仅验证工具可限制样本量；正常 UI 始终训练全部通过质量门槛的素材。
    if request.get('verification_samples'):
        samples = samples[:int(request['verification_samples'])]
    model_dir = Path(runtime['models'])
    cfg = OmegaConf.load(model_dir/'config.yaml')
    features = root/'features'
    features.mkdir()
    wavs = root/'audio'
    wavs.mkdir()
    cache = model_dir/'hf_cache/w2v-bert-2.0'
    extractor = SeamlessM4TFeatureExtractor.from_pretrained(cache, local_files_only=True)
    # 特征模型和 GPT 分阶段占用显存，避免大语料在 CPU 上重复等待。
    device = 'cuda'
    semantic = Wav2Vec2BertModel.from_pretrained(cache, local_files_only=True).to(device).eval()
    stats = torch.load(model_dir/'wav2vec2bert_stats.pt', map_location=device, weights_only=True)
    codec = EnhancedCodec(**cfg.semantic_codec, cfg=cfg.semantic_codec)
    codec.load_checkpoint(str(model_dir/'codec.pth'))
    codec.to(device).eval()
    camp = CAMPPlus(feat_dim=80, embedding_size=192)
    camp.load_state_dict(torch.load(model_dir/'hf_cache/campplus_cn_common.bin', map_location='cpu', weights_only=True))
    camp.to(device).eval()
    tokenizer = get_tokenizer(multilingual=True, model_dir=str(model_dir))
    japanese = JapaneseG2PProcessor(g2p_ratio=0)
    prepared = []
    for i, row in enumerate(samples):
        progress('preparing', i, len(samples))
        audio, sample_rate = sf.read(row['audio'], dtype='float32', always_2d=True)
        duration = len(audio)/sample_rate
        if not 1 <= duration <= 12 or not np.isfinite(audio).all() or np.max(np.abs(audio)) < .005:
            excluded.append(dict(audio=row['audio'], reason='duration_or_signal', duration=duration))
            continue
        mono = torch.from_numpy(audio.mean(axis=1)).unsqueeze(0)
        waveform = torchaudio.functional.resample(mono, sample_rate, 16000)
        text = japanese.process_ja_text(normalize_text(row['text'].lower(), 'ja'))
        text = re.sub(r'<\|([^|]+)\|>', lambda m: '<|'+m[1].upper()+'|>', text)
        ids = tokenizer.encode('<|ja|> '+text, allowed_special='all')
        if not ids or len(ids) > cfg.gpt.max_text_tokens - 2:
            excluded.append(dict(audio=row['audio'], reason='text_length'))
            continue
        with torch.inference_mode():
            inputs = extractor(waveform.squeeze(0).numpy(), sampling_rate=16000, return_tensors='pt').to(device)
            result = semantic(**inputs, output_hidden_states=True)
            feat = (result.hidden_states[17] - stats['mean']) / torch.sqrt(stats['var'])
            codes, _ = codec.quantize(feat)
            fbank = torchaudio.compliance.kaldi.fbank(waveform, num_mel_bins=80, dither=0, sample_frequency=16000)
            fbank -= fbank.mean(dim=0, keepdim=True)
            condition = camp(fbank.unsqueeze(0).to(device))
        stem = row['sha256'][:24]
        path = features/(stem+'.npz')
        np.savez(path, codes=codes.squeeze().cpu().numpy(), features=feat.squeeze(0).cpu().numpy(),
                 condition=condition.squeeze(0).cpu().numpy(), text_ids=np.array(ids,dtype=np.int64))
        # 模型使用独立的规范化音频，原音频只读并保留哈希和全部引用。
        sf.write(wavs/(stem+'.wav'), waveform.squeeze(0).numpy(), 16000, subtype='PCM_16')
        prepared.append(dict(row, feature=str(path), duration=duration, normalized_text=text, prepared_audio=str(wavs/(stem+'.wav'))))
    write_json(root/'dataset.json', dict(language='ja', samples=prepared, excluded=excluded))
    if len(prepared) < 2:
        raise ValueError('voice.insufficient_samples')
    return prepared, excluded


def attach_lora(model, torch, rank=8):
    from torch.nn.utils import parametrize
    class LowRank(torch.nn.Module):
        def __init__(self, weight):
            super().__init__()
            self.a = torch.nn.Parameter(torch.empty(weight.shape[0], rank, device=weight.device))
            self.b = torch.nn.Parameter(torch.zeros(rank, weight.shape[1], device=weight.device))
            torch.nn.init.kaiming_uniform_(self.a, a=math.sqrt(5))

        def forward(self, weight):
            return weight + (self.a @ self.b * 2).to(weight.dtype)
    model.requires_grad_(False)
    attached = {}
    for name, module in model.gpt.named_modules():
        if name.endswith(('attn.c_attn', 'attn.c_proj')):
            adapter = LowRank(module.weight)
            parametrize.register_parametrization(module, 'weight', adapter)
            attached[name] = adapter
    if not attached:
        raise ValueError('IndexTTS GPT attention projections not found')
    return attached


def speech_loss(model, row, torch, device):
    import numpy as np
    from indextts.utils.tokenizer import lang_to_token
    with np.load(row['feature'], allow_pickle=False) as data:
        codes = torch.tensor(data['codes'],dtype=torch.long,device=device).reshape(1,-1)
        ids = torch.tensor(data['text_ids'],dtype=torch.long,device=device).reshape(1,-1)
        feat = torch.tensor(data['features'],dtype=model.text_embedding.weight.dtype,device=device).unsqueeze(0)
        condition = torch.tensor(data['condition'],dtype=feat.dtype,device=device).unsqueeze(0)
    with torch.no_grad():
        lengths = torch.tensor([feat.shape[1]],device=device)
        emo = model.emo_layer(model.emovec_layer(model.get_emo_conditioning(feat.transpose(1,2), lengths)))
        speaker = model.spk_emb_proj(condition).unsqueeze(1)
        conds = torch.cat([speaker + emo.unsqueeze(1), speaker.new_zeros(1,2,model.model_dim)],dim=1)
        text_in = torch.nn.functional.pad(ids,(1,0),value=model.start_text_token)
        text_in = torch.nn.functional.pad(text_in,(0,1),value=model.stop_text_token)
        text_emb = model.text_embedding(text_in) + model.text_pos_embedding(text_in)
        text_emb += model.lang_embedding(torch.tensor([lang_to_token('ja')],device=device)).unsqueeze(1)
        # 每个位置预测一个后继 token，SOT 对齐第一个 code；禁止再次 shift。
        mel_in = torch.nn.functional.pad(codes,(1,0),value=model.start_mel_token)
        targets = torch.nn.functional.pad(codes,(0,1),value=model.stop_mel_token)
        mel_emb = model.mel_embedding(mel_in) + model.mel_pos_embedding(mel_in)
    embeddings = torch.cat([conds,text_emb,mel_emb],dim=1)
    hidden = model.gpt(inputs_embeds=embeddings, return_dict=True, use_cache=False).last_hidden_state
    logits = model.mel_head(model.final_norm(hidden[:,-mel_in.shape[1]:]))
    return torch.nn.functional.cross_entropy(logits.float().transpose(1,2), targets)


def train(runtime, request):
    if request.get('language') != 'ja':
        raise ValueError('voice.language_mismatch')
    epochs = request.get('epochs', 0)
    if not isinstance(epochs, int) or not 1 <= epochs <= 100 or not request.get('character_id'):
        raise ValueError('voice.invalid_training_config')
    data = read_manifest(request['manifest'])
    output = separate_output(request['output_dir'], data['game_dir'], runtime['models'], runtime['source'])
    assets = checked_path(data['asset_root'], directory=True)
    # 输出根可包含先前的 extract 目录，但训练不能写进其只读素材子树。
    if output == assets or assets in output.parents:
        raise ValueError('voice.output_overlap')
    sys.path.insert(0, runtime['source'])
    # 日语 G2P / tokenizer 只写当前任务缓存，不向游戏或另一应用写入。
    root = new_run(output, 'verification' if request.get('verification_samples') else 'training')
    os.environ['HF_HOME'] = str(root/'cache')
    os.environ['HF_HUB_OFFLINE'] = '1'
    import torch
    from omegaconf import OmegaConf
    from indextts.gpt.model_v2 import UnifiedVoice
    torch.set_num_threads(4)
    torch.manual_seed(42)
    if not torch.cuda.is_available():
        raise ValueError('voice.gpu_required')
    write_json(root/'job.json', dict(status='preparing', request=request))
    prepared, excluded = prepare(runtime, request, root, torch)
    gc.collect()
    torch.cuda.empty_cache()
    rng = random.Random(42)
    rng.shuffle(prepared)
    valid_count = max(1, len(prepared)//10)
    valid, training = prepared[:valid_count], prepared[valid_count:]
    write_json(root/'split.json', dict(train=[r['sha256'] for r in training], valid=[r['sha256'] for r in valid]))
    write_json(root/'job.json', dict(status='training', request=request, samples=len(prepared)))
    cfg = OmegaConf.load(Path(runtime['models'])/'config.yaml')
    model = UnifiedVoice(**cfg.gpt, spk_cond_mode='campplus')
    weights = torch.load(Path(runtime['models'])/'gpt.pth', map_location='cpu', weights_only=True)
    model.load_state_dict(weights.get('model', weights), strict=True)
    del weights
    dtype = torch.bfloat16 if torch.cuda.is_bf16_supported() else torch.float32
    model.to(device='cuda', dtype=dtype)
    # 上游空位置编码固定返回 FP32；训练使用 BF16 时须避免把整个隐藏状态提升到 FP32。
    model.gpt.wpe = lambda positions: torch.zeros((*positions.shape, model.model_dim), device=positions.device, dtype=dtype)
    adapters = attach_lora(model, torch)
    model.gpt.gradient_checkpointing_enable(gradient_checkpointing_kwargs={'use_reentrant':False})
    optimizer = torch.optim.AdamW([p for p in model.parameters() if p.requires_grad],lr=1e-4,weight_decay=.01)
    history, step, best = [], 0, float('inf')
    artifact = root/'adapter.pth'
    for epoch in range(epochs):
        rng.shuffle(training)
        # 冻结的音色/情感编码器保持 eval，只启用 GPT 的训练行为。
        model.eval()
        model.gpt.train()
        total_loss = 0
        for row in training:
            optimizer.zero_grad(set_to_none=True)
            loss = speech_loss(model,row,torch,'cuda')
            if not torch.isfinite(loss):
                raise ValueError('Non-finite training loss')
            loss.backward()
            torch.nn.utils.clip_grad_norm_([p for p in model.parameters() if p.requires_grad],1.0)
            optimizer.step()
            total_loss += loss.item()
            step += 1
            progress('training_step',step,epochs*len(training))
        model.eval()
        progress('validating',epoch+1,epochs)
        with torch.no_grad():
            validation = sum(speech_loss(model,row,torch,'cuda').item() for row in valid)/len(valid)
        if not math.isfinite(validation):
            raise ValueError('Non-finite validation loss')
        train_loss = total_loss/len(training)
        history.append(dict(epoch=epoch+1,train_loss=train_loss,validation_loss=validation))
        emit('log', f'Epoch {epoch+1}/{epochs} · train_loss={train_loss:.6f} · validation_loss={validation:.6f}\n')
        write_json(root/'metrics.json',history)
        if validation < best:
            progress('saving',epoch+1,epochs)
            checkpoint = dict(format='galfriends-indextts25-lora-v1',rank=8,scale=2.,language='ja',
                              modules={name:{k:v.detach().cpu() for k,v in adapter.state_dict().items()} for name,adapter in adapters.items()})
            temporary = artifact.with_suffix('.tmp')
            torch.save(checkpoint,temporary)
            torch.load(temporary,map_location='cpu',weights_only=True)
            os.replace(temporary,artifact)
            best = validation
    shutil.copyfile(training[0]['prepared_audio'],root/'reference.wav')
    write_json(root/'model.json',dict(format='galfriends-indextts25-lora-v1',character_id=request['character_id'],
               speaker=request['speaker'],language='ja',base='IndexTeam/IndexTTS-2.5',
               base_sha256=digest(Path(runtime['models'])/'gpt.pth'),adapter='adapter.pth',reference='reference.wav',
               manifest=request['manifest'],samples=len(prepared),validation_samples=len(valid),steps=step,
               verification_only=bool(request.get('verification_samples')),best_validation_loss=best))
    write_json(root/'job.json',dict(status='completed',request=request,artifact=str(artifact)))
    return dict(artifact=str(artifact),samples=len(prepared),excluded=len(excluded))


def apply_adapter(model, path, torch, base_sha256):
    """校验基础权重哈希后将日语 LoRA 加到内存模型，不写回 checkpoint。"""
    path = checked_path(str(path))
    metadata = json.loads(path.with_name('model.json').read_text('utf-8'))
    if metadata.get('base_sha256') != base_sha256 or metadata.get('adapter') != path.name:
        raise ValueError('LoRA base model mismatch')
    value = torch.load(path,map_location='cpu',weights_only=True)
    if value.get('format') != 'galfriends-indextts25-lora-v1' or value.get('language') != 'ja':
        raise ValueError('Invalid IndexTTS 2.5 Japanese adapter')
    expected = {name for name, _ in model.gpt.named_modules() if name.endswith(('attn.c_attn', 'attn.c_proj'))}
    if set(value.get('modules', {})) != expected or not expected or value.get('rank') != 8 or value.get('scale') != 2.:
        raise ValueError('Incomplete IndexTTS 2.5 adapter')
    # 先检查全部模块，避免后面的损坏张量使内存模型只应用一半适配器。
    deltas = []
    with torch.no_grad():
        for name, weights in value['modules'].items():
            module = model.gpt.get_submodule(name)
            delta = weights['a'] @ weights['b'] * value['scale']
            if module.weight.shape != delta.shape or not torch.isfinite(delta).all():
                raise ValueError('Invalid LoRA tensor')
            deltas.append((module, delta))
        for module, delta in deltas:
            module.weight.add_(delta.to(module.weight.device, module.weight.dtype))

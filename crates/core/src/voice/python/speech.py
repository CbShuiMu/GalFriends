"""只合成日语；每次在独立进程内加载角色 LoRA，取消后释放全部显存。"""
import json
import os
from pathlib import Path
import re
import sys
from common import checked_path, new_run, progress, separate_output
from extract import digest
from reference_audio import selected_reference


def speak(runtime, request):
    text = request['text'].strip()
    if request.get('language') != 'ja' or not 0 < len(text) <= 300 or not re.search(r'[ぁ-ヶ]', text):
        raise ValueError('voice.language_mismatch')
    directory = checked_path(request['model_dir'], directory=True)
    metadata = json.loads(checked_path(str(directory/'model.json')).read_text('utf-8'))
    if (metadata['character_id'] != request['character_id'] or metadata['language'] != 'ja'
            or metadata.get('verification_only') is not False
            or metadata['adapter'] != 'adapter.pth' or metadata['reference'] != 'reference.wav'):
        raise ValueError('companion.model_invalid')
    adapter = checked_path(str(directory/'adapter.pth'))
    selected = selected_reference(directory, request)
    reference = selected[0] if selected else checked_path(str(directory/'reference.wav'))
    model_dir = checked_path(runtime['models'], directory=True)
    source = checked_path(runtime['source'], directory=True)
    output = separate_output(request['output_dir'], directory, model_dir, source)
    root = new_run(output, 'speech')
    os.chdir(root)
    os.environ['HF_HOME'] = str(root/'cache')
    os.environ['HF_HUB_OFFLINE'] = '1'
    sys.path.insert(0, str(source))
    import torch
    import soundfile as sf
    from indextts.infer_v2_5 import IndexTTS2
    from training import apply_adapter
    torch.set_num_threads(4)
    if not torch.cuda.is_available():
        raise ValueError('voice.gpu_required')
    progress('loading')
    model = IndexTTS2(cfg_path=str(model_dir/'config.yaml'), model_dir=str(model_dir),
                      use_bf16=True, device='cuda', use_cuda_kernel=False, use_qwen_emo=False)
    apply_adapter(model.gpt, adapter, torch, digest(model_dir/'gpt.pth'))
    released = False
    def report(value, desc=None):
        nonlocal released
        # 官方在 0.1 进度前完成说话人/情感参考编码。此后只用缓存特征，
        # 将 2.4 GB 编码器移回 CPU，避免 8 GB 显卡在 GPT 解码时换页。
        if value >= .1 and not released:
            model.semantic_model.to('cpu')
            torch.cuda.empty_cache()
            released = True
        progress('synthesizing', int(value * 100), 100)
    model.gr_progress = report
    progress('synthesizing')
    wav = root/'reply.wav'
    model.infer(spk_audio_prompt=str(reference), text=text, output_path=str(wav), lang='ja',
                max_text_tokens_per_segment=80, use_emo_text=False, verbose=False,
                num_beams=1, max_mel_tokens=600)
    samples, rate = sf.read(wav)
    import numpy as np
    if not len(samples) or not np.isfinite(samples).all() or np.max(np.abs(samples)) < 1e-5 or len(samples)/rate > 120:
        raise ValueError('companion.audio_invalid')
    return dict(artifact=str(wav))

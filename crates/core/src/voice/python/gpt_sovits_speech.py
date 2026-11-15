"""使用官方 GPT-SoVITS 推理器合成角色的日语语音。"""
import json
import os
import re
import sys
import time
from common import checked_path, new_run, progress, separate_output, write_json
from extract import digest
from reference_audio import selected_reference
from gpt_sovits import FORMAT, V2PRO_FORMAT, V2PRO_SV, base_generator, ready, version


def reference_prompt(directory, request):
    """在加载 GPU 模型前核对用户已确认的参考，缺省使用模型自带参考。"""
    selected = selected_reference(directory, request)
    if selected:
        return selected
    return directory/'reference.wav', (directory/'reference.txt').read_text('utf-8')


def speak(runtime, request):
    segments = request.get('segments', [request])
    texts = [segment['text'].strip() for segment in segments]
    if (request.get('language') != 'ja' or not texts or sum(map(len, texts)) > 300
            or any(not re.search(r'[ぁ-ヶ]', text) for text in texts)):
        raise ValueError('voice.language_mismatch')
    if not ready(runtime):
        raise ValueError('voice.runtime_missing')
    variant = version(runtime)
    if 'segments' in request and variant != 'v5turbo':
        raise ValueError('companion.invalid_reply')
    expected_format = V2PRO_FORMAT if variant == 'v2Pro' else FORMAT
    directory = checked_path(request['model_dir'], directory=True)
    metadata = json.loads(checked_path(str(directory/'model.json')).read_text('utf-8'))
    if (metadata.get('format') != expected_format or metadata.get('character_id') != request['character_id']
            or metadata.get('language') != 'ja' or metadata.get('verification_only') is not False):
        raise ValueError('companion.model_invalid')
    for field, filename in [('gpt','gpt.ckpt'), ('sovits','sovits.pth'), ('reference','reference.wav'), ('transcript','reference.txt')]:
        path = checked_path(str(directory/filename))
        if metadata.get(field) != filename or digest(path) != metadata['files'].get(filename):
            raise ValueError('companion.model_invalid')
    prompts = [reference_prompt(directory, segment) for segment in segments]
    source = checked_path(runtime['source'], directory=True)
    models = checked_path(runtime['models'], directory=True)
    if digest(base_generator(runtime)) != metadata['base_sha256']:
        raise ValueError('companion.model_invalid')
    output = separate_output(request['output_dir'], directory, source, models)
    root = new_run(output, 'speech-gsv')
    # 官方推理器的声码器相对于源码目录解析，配置写入显式转到任务缓存。
    os.chdir(source)
    os.environ.update(HF_HUB_OFFLINE='1', HF_HOME=str(root/'cache'), G2PW='0',
                      language='en_US', version=variant, CUDAGraph='0')
    sys.path[:0] = [str(source), str(source/'GPT_SoVITS')]
    import soundfile as sf
    import torch
    from TTS_infer_pack.TTS import TTS, TTS_Config
    if variant == 'v2Pro':
        import sv
        sv.sv_path = str(models/V2PRO_SV)
    torch.set_num_threads(4)
    torch.manual_seed(42)
    loaded = time.perf_counter()
    config = TTS_Config(dict(custom=dict(version=variant, device='cuda', is_half=True,
                          t2s_weights_path=str(directory/'gpt.ckpt'), vits_weights_path=str(directory/'sovits.pth'),
                          bert_base_path=str(models/'chinese-roberta-wwm-ext-large'),
                          cnhuhbert_base_path=str(models/'chinese-hubert-base')),
                          **{variant: dict(vits_weights_path=str(base_generator(runtime)))}))
    config.configs_path = str(root/'tts_infer.yaml')
    model = TTS(config)
    started = time.perf_counter()
    progress('synthesizing')
    rate, audio = synthesize_segments(model, zip(texts, prompts, strict=True))
    wav = root/'reply.wav'
    sf.write(wav, audio, rate, subtype='PCM_16')
    write_json(root/'timing.json', dict(engine=metadata['engine'], text=''.join(texts), character_id=request['character_id'],
               load_seconds=started-loaded, generation_seconds=time.perf_counter()-started, audio_seconds=len(audio)/rate))
    return dict(artifact=str(wav))


def synthesize_segments(model, segments):
    """复用已加载模型，保留推理器的句间停顿，按朗读顺序拼接采样。"""
    import numpy as np

    clips = []
    rate = None
    frames = 0
    for text, (reference_audio, reference_text) in segments:
        outputs = list(model.run(dict(text=text, text_lang='all_ja', prompt_lang='all_ja',
                        ref_audio_path=str(reference_audio), prompt_text=reference_text,
                        text_split_method='cut5', batch_size=1, parallel_infer=False, seed=42,
                        sample_steps=4, return_fragment=False, streaming_mode=False)))
        if len(outputs) != 1:
            raise ValueError('companion.audio_invalid')
        sample_rate, clip = outputs[0]
        if (sample_rate <= 0 or (rate is not None and sample_rate != rate)
                or clip.ndim != 1 or not clip.size or not np.isfinite(clip).all() or not np.any(clip)):
            raise ValueError('companion.audio_invalid')
        rate = sample_rate
        frames += clip.size
        if frames > rate * 120:
            raise ValueError('companion.audio_invalid')
        clips.append(clip)
    return rate, clips[0] if len(clips) == 1 else np.concatenate(clips)

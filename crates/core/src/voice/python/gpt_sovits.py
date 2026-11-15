"""官方 GPT-SoVITS 的日语数据准备和两阶段训练适配。

所有训练文件保存在独立任务目录；官方脚本以固定参数启动，逐阶段验证
样本集合，避免其捕获单条异常后仍以退出码零结束而漏训。
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time
from common import checked_path, new_run, progress, separate_output, write_json
from extract import read_manifest, digest
from dataset import select_samples

FORMAT = 'galfriends-gpt-sovits-v5turbo-v1'
V2PRO_ENGINE = 'gpt_sovits_v2pro'
V2PRO_FORMAT = 'galfriends-gpt-sovits-v2pro-v1'
V2PRO_SV = 'sv/pretrained_eres2netv2w24s4ep4.ckpt'
V2PRO_DISCRIMINATOR = 'v2Pro/s2Dv2Pro.pth'
ENGINES = ('gpt_sovits_v5turbo', V2PRO_ENGINE)


def version(runtime):
    return 'v2Pro' if runtime.get('engine') == V2PRO_ENGINE else 'v5turbo'


def base_generator(runtime):
    filename = 'v2Pro/s2Gv2Pro.pth' if version(runtime) == 'v2Pro' else 'gsv-v5-pretrained/s2Gv5turbo.pth'
    return Path(runtime['models'])/filename


def ready(runtime):
    root = Path(runtime['models'])
    v2pro = version(runtime) == 'v2Pro'
    record = root/('verified-models-v2pro.json' if v2pro else 'verified-models.json')
    if not record.is_file():
        return False
    provenance = json.loads(record.read_text('utf-8'))
    if provenance.get('revision') != runtime.get('model_revision'):
        return False
    variant_weights = (V2PRO_DISCRIMINATOR, V2PRO_SV) if v2pro else ('gsv-v5-pretrained/vocoder.pth',)
    required = (
        's1v3.ckpt', *variant_weights,
        'fast_langdetect/lid.176.bin', 'fast_langdetect/verified-model.json',
        'chinese-hubert-base/pytorch_model.bin', 'chinese-roberta-wwm-ext-large/pytorch_model.bin')
    paths = [base_generator(runtime), *(root/name for name in required)]
    return all(path.is_file() and path.stat().st_size > 0 for path in paths)


def prepare(runtime, request):
    import numpy as np
    import soundfile as sf
    from scipy.signal import resample_poly
    from math import gcd
    if request.get('language') != 'ja' or not request.get('character_id'):
        raise ValueError('voice.language_mismatch')
    epochs = request.get('epochs')
    if not isinstance(epochs, int) or not 1 <= epochs <= 100:
        raise ValueError('voice.invalid_training_config')
    data = read_manifest(request['manifest'])
    output = separate_output(request['output_dir'], data['game_dir'], runtime['source'], runtime['models'])
    assets = checked_path(data['asset_root'], directory=True)
    if output == assets or assets in output.parents:
        raise ValueError('voice.output_overlap')
    root = new_run(output, 'gpt-sovits-'+version(runtime).lower())
    write_json(root/'job.json', dict(status='preparing', request=request, started_at=time.time()))
    samples, excluded = select_samples(data, request['speaker'], 'ja')
    audio_dir = root/'audio'
    audio_dir.mkdir()
    prepared, lines = [], []
    for index, row in enumerate(samples):
        progress('preparing', index, len(samples))
        audio, rate = sf.read(row['audio'], dtype='float32', always_2d=True)
        duration = len(audio)/rate
        if not 1 <= duration <= 12 or not np.isfinite(audio).all() or np.max(np.abs(audio)) < .005:
            excluded.append(dict(audio=row['audio'], reason='duration_or_signal', duration=duration))
            continue
        # .list 的四列协议不能用含分隔符的文本；拒绝而不是截断或错配。
        if any(c in row['text'] for c in '|\r\n\t') or any(c in request['speaker'] for c in '|\r\n\t'):
            raise ValueError('voice.conflicting_transcripts')
        stem = row['sha256'][:24]
        wav = audio_dir/(stem+'.wav')
        factor = gcd(rate, 32000)
        mono = resample_poly(audio.mean(axis=1), 32000//factor, rate//factor)
        sf.write(wav, mono, 32000, subtype='PCM_16')
        wav.with_suffix('.txt').write_text(row['text']+'\n', encoding='utf-8')
        prepared.append(dict(row, prepared_audio=str(wav), duration=duration))
        lines.append(f'{wav.as_posix()}|{request["speaker"]}|ja|{row["text"]}')
    if len(prepared) < 2:
        raise ValueError('voice.insufficient_samples')
    (root/'dataset.list').write_text('\n'.join(lines)+'\n', encoding='utf-8')
    write_json(root/'dataset.json', dict(language='ja', samples=prepared, excluded=excluded))
    # 官方参考音频要求 3–10 秒，挑选普通短句并保留对应日语原文。
    candidates = [r for r in prepared if 3.1 <= r['duration'] <= 8 and len(r['text']) >= 12]
    if not candidates:
        raise ValueError('voice.insufficient_samples')
    reference = min(candidates, key=lambda r: abs(r['duration']-5))
    shutil.copyfile(reference['prepared_audio'], root/'reference.wav')
    (root/'reference.txt').write_text(reference['text'], encoding='utf-8')
    write_json(root/'reference-source.json', reference)
    return root


def environment(runtime, root):
    source, models = Path(runtime['source']), Path(runtime['models'])
    variant = version(runtime)
    env = os.environ.copy()
    env.update(PYTHONUTF8='1', PYTHONIOENCODING='utf-8', OMP_NUM_THREADS='4',
               PATH=str(source/'runtime')+os.pathsep+env.get('PATH',''),
               PYTHONPATH=os.pathsep.join(str(source/p) for p in ('', 'GPT_SoVITS', 'GPT_SoVITS/eres2net')),
               version=variant, language='en_US', G2PW='0', is_half='True',
               _CUDA_VISIBLE_DEVICES='0', CUDA_VISIBLE_DEVICES='0',
               HF_HUB_OFFLINE='1', HF_HOME=str(root/'cache'),
               inp_text=str(root/'dataset.list'), inp_wav_dir=str(root/'audio'),
               exp_name='character', i_part='0', all_parts='1', opt_dir=str(root),
               bert_pretrained_dir=str(models/'chinese-roberta-wwm-ext-large'),
               cnhubert_base_dir=str(models/'chinese-hubert-base'),
               pretrained_s2G=str(base_generator(runtime)),
               s2config_path=str(source/'GPT_SoVITS/configs'/('s2v2Pro.json' if variant == 'v2Pro' else 's2.json')))
    if variant == 'v2Pro':
        env['sv_path'] = str(models/V2PRO_SV)
    return env


def official_step(runtime, root, script, stage, args=()):
    progress(stage)
    started = time.time()
    # 上游 i18n 用 relpath(__file__)；Windows 上 cwd 不能与源码跨盘。
    # 临时工作区留在源码同盘的独立目录，训练配置和最终产物仍全部指向用户输出盘。
    work = checked_path(str(Path(runtime['source']).parent/'gpt-sovits-work'/root.name))
    work.mkdir(parents=True, exist_ok=True)
    entry = (Path(__file__).with_name('gpt_sovits_gpt.py') if script == 'GPT_SoVITS/s1_train.py'
             else Path(runtime['source'])/script)
    log_path = checked_path(str(root/(stage+'.log')))
    if log_path.exists():
        log_path.rename(root/f'{stage}-{time.time_ns()}.log')
    with log_path.open('xb') as log:
        result = subprocess.run([runtime['python'], '-B', '-u', str(entry), *args],
                       cwd=work, env=environment(runtime, root), stdout=log, stderr=subprocess.STDOUT,
                       check=False, timeout=48*3600,
                       creationflags=0x08000000 if os.name == 'nt' else 0)
    timings_path = root/'stage-timings.json'
    timings = json.loads(timings_path.read_text('utf-8')) if timings_path.exists() else []
    timings.append(dict(stage=stage, started_at=started, finished_at=time.time(), exit_code=result.returncode))
    write_json(timings_path, timings)
    if result.returncode:
        with (root/(stage+'.log')).open('rb') as log:
            log.seek(max(0, log.seek(0, 2)-3000))
            detail = log.read().decode('utf-8', errors='replace')
        raise ValueError(f'voice.training_failed: {stage}\n{detail}')


def verify_names(path, expected, header=False):
    rows = path.read_text('utf-8').strip().splitlines()
    if header:
        rows = rows[1:]
    names = [line.split('\t')[0] for line in rows]
    if set(names) != expected or len(names) != len(expected):
        raise ValueError('voice.incomplete_features')


def validate_weights(runtime, root, torch):
    """完成标记之前核对有限权重和实际更新，拒绝只保存底模或零适配器。"""
    source = Path(runtime['source'])
    original = Path.cwd()
    sys.path[:0] = [str(source), str(source/'GPT_SoVITS')]
    try:
        os.chdir(source)
        from process_ckpt import load_sovits_new
        sovits = load_sovits_new(str(root/'sovits.pth'))['weight']
    finally:
        os.chdir(original)
    gpt = torch.load(root/'gpt.ckpt', map_location='cpu', weights_only=False)['weight']
    for weights in (gpt, sovits):
        if not weights or any(not torch.is_tensor(t) or not torch.isfinite(t).all() for t in weights.values()):
            raise ValueError('voice.incomplete_training')
    if version(runtime) == 'v2Pro':
        base = torch.load(base_generator(runtime), map_location='cpu', weights_only=False)['weight']
        # 上游仅导出推理需要的生成器层，并转换为半精度。
        decoder_keys = [key for key in sovits if key.startswith('dec.')]
        if not decoder_keys or not any(key in base and not torch.equal(sovits[key], base[key].to(sovits[key].dtype))
                                       for key in decoder_keys):
            raise ValueError('voice.incomplete_training')
        del base
    elif not any('lora_B' in key and torch.count_nonzero(tensor) for key, tensor in sovits.items()):
        raise ValueError('voice.incomplete_training')
    base = torch.load(Path(runtime['models'])/'s1v3.ckpt', map_location='cpu', weights_only=False)['weight']
    if set(gpt) != set(base) or all(torch.equal(gpt[key], base[key].to(gpt[key].dtype)) for key in base):
        raise ValueError('voice.incomplete_training')


def run_training(runtime, root):
    import yaml
    import torch
    if not ready(runtime) or not torch.cuda.is_available():
        raise ValueError('voice.runtime_missing')
    root = checked_path(str(root), directory=True)
    job = json.loads((root/'job.json').read_text('utf-8'))
    job['run_started_at'] = time.time()
    write_json(root/'job.json', job)
    request = job['request']
    dataset = json.loads((root/'dataset.json').read_text('utf-8'))
    names = {Path(r['prepared_audio']).name for r in dataset['samples']}
    source, models = Path(runtime['source']), Path(runtime['models'])
    variant = version(runtime)
    v2pro = variant == 'v2Pro'
    if not (root/'2-name2text-0.txt').is_file():
        official_step(runtime, root, 'GPT_SoVITS/prepare_datasets/1-get-text.py', 'gsv_phonemes')
    verify_names(root/'2-name2text-0.txt', names)
    shutil.copyfile(root/'2-name2text-0.txt', root/'2-name2text.txt')
    if not all((root/'4-cnhubert'/(name+'.pt')).is_file() and (root/'5-wav32k'/name).is_file() for name in names):
        official_step(runtime, root, 'GPT_SoVITS/prepare_datasets/2-get-hubert-wav32k.py', 'gsv_hubert')
    for name in names:
        if not (root/'4-cnhubert'/(name+'.pt')).is_file() or not (root/'5-wav32k'/name).is_file():
            raise ValueError('voice.incomplete_features')
    if v2pro:
        sv_dir = root/'7-sv_cn'
        if not all((sv_dir/(name+'.pt')).is_file() for name in names):
            official_step(runtime, root, 'GPT_SoVITS/prepare_datasets/2-get-sv.py', 'gsv_speaker')
        if {path.name for path in sv_dir.glob('*.pt') if path.stat().st_size > 0} != {name+'.pt' for name in names}:
            raise ValueError('voice.incomplete_features')
    if not (root/'6-name2semantic-0.tsv').is_file():
        official_step(runtime, root, 'GPT_SoVITS/prepare_datasets/3-get-semantic.py', 'gsv_semantic')
    verify_names(root/'6-name2semantic-0.tsv', names)
    (root/'6-name2semantic.tsv').write_text('item_name\tsemantic_audio\n'+(root/'6-name2semantic-0.tsv').read_text('utf-8'), encoding='utf-8')
    job.update(status='training', prepared_at=time.time(), samples=len(names))
    write_json(root/'job.json', job)
    epochs = request['epochs']
    weights = root/'weights'
    weights.mkdir(exist_ok=True)
    config = yaml.safe_load((source/'GPT_SoVITS/configs/s1longer-v2.yaml').read_text('utf-8'))
    config['train'].update(batch_size=4, epochs=epochs, save_every_n_epoch=epochs,
                           if_save_every_weights=True, if_save_latest=True, if_dpo=False,
                           half_weights_save_dir=str(weights), exp_name='character')
    config['data']['num_workers'] = 2
    config.update(pretrained_s1=str(models/'s1v3.ckpt'), train_semantic_path=str(root/'6-name2semantic.tsv'),
                  train_phoneme_path=str(root/'2-name2text.txt'), output_dir=str(root/'logs_s1'))
    (root/'s1.yaml').write_text(yaml.safe_dump(config), encoding='utf-8')
    official_step(runtime, root, 'GPT_SoVITS/s1_train.py', 'gsv_gpt', ['--config_file', str(root/'s1.yaml')])
    config_path = source/'GPT_SoVITS/configs'/('s2v2Pro.json' if v2pro else 's2.json')
    config = json.loads(config_path.read_text('utf-8'))
    config['train'].update(batch_size=1 if v2pro else 2, epochs=epochs, pretrained_s2G=str(base_generator(runtime)),
                           pretrained_s2D=str(models/V2PRO_DISCRIMINATOR) if v2pro else '',
                           if_save_latest=True, if_save_every_weights=True, save_every_epoch=epochs, gpu_numbers='0')
    if not v2pro:
        config['train'].update(grad_ckpt=True, lora_rank=16)
    config['model']['version'] = variant
    config['data']['exp_dir'] = str(root)
    config.update(s2_ckpt_dir=str(root), save_weight_dir=str(weights), name='character', version=variant)
    write_json(root/'s2.json', config)
    trainer = 's2_train.py' if v2pro else 's2_train_v3_lora.py'
    if v2pro:
        (root/'logs_s2_v2Pro').mkdir(exist_ok=True)
    official_step(runtime, root, 'GPT_SoVITS/'+trainer, 'gsv_sovits', ['--config', str(root/'s2.json')])
    gpt = weights/f'character-e{epochs}.ckpt'
    sovits = list(weights.glob(f'character_e{epochs}_s*.pth'))
    if not gpt.is_file() or len(sovits) != 1:
        raise ValueError('voice.incomplete_training')
    shutil.copyfile(gpt, root/'gpt.ckpt')
    shutil.copyfile(sovits[0], root/'sovits.pth')
    validate_weights(runtime, root, torch)
    gpt_data = json.loads((root/'gpt-dataset.json').read_text('utf-8'))
    gpt_names = set(gpt_data['used'])
    metadata = dict(format=V2PRO_FORMAT if v2pro else FORMAT,
                    engine=V2PRO_ENGINE if v2pro else ENGINES[0], character_id=request['character_id'],
                    speaker=request['speaker'], language='ja', verification_only=False,
                    gpt='gpt.ckpt', sovits='sovits.pth', reference='reference.wav', transcript='reference.txt',
                    samples=len(names), duration=sum(r['duration'] for r in dataset['samples']), epochs=epochs,
                    gpt_samples=len(gpt_data['used']), sovits_samples=len(names),
                    gpt_duration=sum(r['duration'] for r in dataset['samples'] if Path(r['prepared_audio']).name in gpt_names),
                    base_sha256=digest(base_generator(runtime)),
                    source_revision=runtime['revision'], model_revision=runtime['model_revision'],
                    files={name:digest(root/name) for name in ['gpt.ckpt','sovits.pth','reference.wav','reference.txt']})
    write_json(root/'model.json', metadata)
    job.update(status='completed', completed_at=time.time())
    write_json(root/'job.json', job)
    return dict(artifact=str(root), samples=len(names), excluded=len(dataset['excluded']))


def train(runtime, request):
    root = prepare(runtime, request)
    try:
        return run_training(runtime, root)
    except Exception:
        job = json.loads((root/'job.json').read_text('utf-8'))
        job.update(status='failed', failed_at=time.time())
        write_json(root/'job.json', job)
        raise

"""固定本地任务入口。结果仅在验证后发送；退出时终止本任务的所有子进程。"""
import json
from pathlib import Path
import sys
from common import emit, progress


sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'python'))
from worker_guard import guard_parent

def main():
    runtime = json.loads(Path(sys.argv[1]).read_text('utf-8-sig'))
    guard_parent(int(sys.argv[3]) if len(sys.argv) > 3 else 0)
    request = json.load(sys.stdin)
    action = sys.argv[2]
    if action == 'train':
        # 隔离训练库的普通打印，避免它们和结构化进度共用一行。
        sys.stdout = sys.stderr
    progress('loading')
    import gpt_sovits
    if runtime.get('engine') in gpt_sovits.ENGINES and action in ('status', 'train', 'speak'):
        if action == 'status':
            result = dict(ready=gpt_sovits.ready(runtime))
        elif action == 'train':
            result = gpt_sovits.train(runtime, request)
        else:
            from gpt_sovits_speech import speak
            result = speak(runtime, request)
    elif action == 'status':
        models = Path(runtime['models'])
        files = ['config.yaml','gpt.pth','codec.pth','s2mel.pth','multilingual_zh_ja_yue_char_del.tiktoken','wav2vec2bert_stats.pt']
        ready = all((models/f).is_file() and (models/f).stat().st_size > 0 for f in files)
        ready = ready and (models/'hf_cache/w2v-bert-2.0/model.safetensors').is_file()
        ready = ready and (models/'hf_cache/campplus_cn_common.bin').is_file()
        result = dict(ready=ready)
    elif action == 'skill_extract':
        from skill_extract import extract
        result = extract(runtime, request)
    elif action == 'extract':
        from extract import extract
        result = extract(runtime, request)
    elif action == 'inspect':
        from extract import inspect
        result = inspect(Path(request['manifest']))
    elif action == 'speak':
        from speech import speak
        result = speak(runtime, request)
    elif action == 'train':
        from training import train
        result = train(runtime, request)
    else:
        raise ValueError('voice.invalid_action')
    emit('result', result)


if __name__ == '__main__':
    try:
        main()
    except Exception as error:
        emit('error', str(error))
        sys.exit(1)

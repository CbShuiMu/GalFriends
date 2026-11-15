"""固定本地任务入口。结果仅在验证后发送；退出时终止本任务的所有子进程。"""
import ctypes
import json
import os
from pathlib import Path
import sys
import threading
from common import emit, progress


def guard_parent(pid):
    if os.name != 'nt' or not pid:
        return
    # Windows Job 只包含当前 worker 及其后代，关闭句柄会清理提取工具。
    kernel = ctypes.WinDLL('kernel32', use_last_error=True)
    kernel.CreateJobObjectW.restype = ctypes.c_void_p
    kernel.OpenProcess.restype = ctypes.c_void_p
    kernel.GetCurrentProcess.restype = ctypes.c_void_p
    kernel.SetInformationJobObject.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_void_p, ctypes.c_uint]
    kernel.AssignProcessToJobObject.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
    kernel.WaitForSingleObject.argtypes = [ctypes.c_void_p, ctypes.c_uint]
    job = kernel.CreateJobObjectW(None, None)
    info = ctypes.create_string_buffer(144)
    ctypes.c_uint.from_buffer(info, 16).value = 0x2000
    if not kernel.SetInformationJobObject(job, 9, info, len(info)) or not kernel.AssignProcessToJobObject(job, kernel.GetCurrentProcess()):
        raise OSError('Cannot establish worker process boundary')
    parent = kernel.OpenProcess(0x00100000, False, pid)
    if not parent:
        raise OSError('Parent process is unavailable')
    def wait():
        kernel.WaitForSingleObject(parent, 0xffffffff)
        os._exit(2)
    threading.Thread(target=wait, daemon=True).start()


def main():
    runtime = json.loads(Path(sys.argv[1]).read_text('utf-8-sig'))
    guard_parent(int(sys.argv[3]) if len(sys.argv) > 3 else 0)
    request = json.load(sys.stdin)
    action = sys.argv[2]
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
        print(json.dumps(dict(event='error', detail=str(error)), ensure_ascii=False), flush=True)
        sys.exit(1)

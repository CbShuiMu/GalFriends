"""语音任务的路径边界、原子结果和结构化进度。"""
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import uuid

PROTOCOL_OUTPUT = sys.stdout

def emit(event, data):
    print(json.dumps({"event": event, "data": data}, ensure_ascii=False),
          file=PROTOCOL_OUTPUT, flush=True)


def progress(stage, current=0, total=0):
    emit("progress", dict(stage=stage, current=current, total=total))


def checked_path(value, directory=False):
    # Rust canonicalize 返回 Windows 扩展路径；先统一别名，避免目录重叠检查被不同前缀绕过。
    if os.name == 'nt' and isinstance(value, str):
        value = value.replace('/', '\\')
        if value.startswith('\\\\?\\UNC\\'):
            value = '\\\\' + value[8:]
        elif value.startswith('\\\\?\\'):
            value = value[4:]
        if value.startswith('\\\\.\\'):
            raise ValueError('voice.unsafe_path')
    if not value or not Path(value).is_absolute():
        raise ValueError("voice.path_required")
    path = Path(os.path.abspath(value))
    for ancestor in [*reversed(path.parents), path]:
        if ancestor.exists() or ancestor.is_symlink():
            metadata = ancestor.lstat()
            if stat.S_ISLNK(metadata.st_mode) or getattr(metadata, "st_file_attributes", 0) & 0x400:
                raise ValueError("voice.unsafe_path")
    # 检查重解析点之后再展开 8.3 短路径，保证后续边界比较使用同一目录身份。
    path = Path(os.path.realpath(path))
    if directory and not path.is_dir():
        raise ValueError("voice.path_required")
    return path


def separate_output(value, *sources):
    output = checked_path(value)
    for source in sources:
        source = checked_path(str(source))
        if output == source or output in source.parents or source in output.parents:
            raise ValueError("voice.output_overlap")
    return output


def new_run(output, prefix):
    output.mkdir(parents=True, exist_ok=True)
    checked_path(str(output), directory=True)
    result = output / (prefix + '-' + uuid.uuid4().hex[:12])
    result.mkdir()
    return result


def write_json(path, value):
    temporary = path.with_name(path.name + '.tmp-' + uuid.uuid4().hex)
    with temporary.open('x', encoding='utf-8') as stream:
        json.dump(value, stream, ensure_ascii=False, indent=2)
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(temporary, path)


def run_command(args, cwd=None, timeout=180):
    result = subprocess.run([str(a) for a in args], cwd=cwd, capture_output=True,
                            timeout=timeout, creationflags=0x08000000 if os.name == 'nt' else 0)
    if result.returncode:
        raise ValueError(result.stderr.decode('utf-8', errors='replace')[-2000:])
    return result

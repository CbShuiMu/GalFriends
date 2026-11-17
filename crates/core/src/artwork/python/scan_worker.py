"""桌面后台扫描入口，与人工 CLI 复用提取、归档和去重实现。"""
import json
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'python'))
from worker_guard import guard_parent
from sprite_expressions.scan import scan

if __name__ == '__main__':
    try:
        guard_parent(int(sys.argv[2]))
        scan(json.loads(Path(sys.argv[1]).read_text('utf-8')))
    except Exception as error:
        print(str(error), file=sys.stderr)
        raise SystemExit(1)

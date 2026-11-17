"""GalFriends 离线立绘表情工具的命令行入口。"""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent / 'crates/core/src/artwork/python'))

from sprite_expressions.cli import main


if __name__ == '__main__':
    sys.stdout.reconfigure(encoding='utf-8')
    sys.stderr.reconfigure(encoding='utf-8')
    raise SystemExit(main())

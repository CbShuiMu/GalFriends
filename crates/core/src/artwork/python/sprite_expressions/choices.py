"""归档时保存游戏原始表情近景，不读取或融合 GalFriends 当前立绘。"""
import json
from pathlib import Path

from PIL import Image

from .compose import default_variants, read_manifest
from .pipeline import write_json


def prepare(directory):
    """只在离线提取/归档时运行，face 弹窗直接读取这些已保存的 PNG。"""
    body, entries = read_manifest(directory / 'manifest.json')
    source = json.loads((directory / 'manifest.json').read_text(encoding='utf-8'))['source']
    left = min(offset[0] for _, offset in entries.values())
    top = min(offset[1] for _, offset in entries.values())
    right = max(offset[0] + image.width for image, offset in entries.values())
    bottom = max(offset[1] + image.height for image, offset in entries.values())
    padding = max(10, (right - left) // 3)
    bounds = (max(0, left - padding), max(0, top - padding),
              min(body.width, right + padding), min(body.height, bottom + padding))
    (directory / 'choices').mkdir(exist_ok=True)
    choices = []
    for number, layers in enumerate(default_variants(entries)):
        original = body.copy()
        for layer in layers:
            image, offset = entries[layer]
            original.alpha_composite(image, offset)
        thumbnail = original.crop(bounds)
        thumbnail.thumbnail((230, 185), Image.Resampling.LANCZOS)
        filename = f'{number:03}.png'
        thumbnail.save(directory / 'choices' / filename)
        choices.append(dict(layers=layers, thumbnail=filename))
    write_json(directory / 'choices.json', dict(format_version=1, source=Path(source).name, variants=choices))

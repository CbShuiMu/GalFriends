"""读取 GARbro 导出的 PNAP 容器，保留空记录、原始 PNG、坐标与预乘 alpha。

格式依据为 GARbro ArcFormats/Will/ArcPNA.cs。这里只支持已验证的 PNG 负载，
组编号仅记录容器结构，不将其推断为游戏脚本中的表情语义。
"""
from dataclasses import dataclass
import hashlib
from io import BytesIO
from pathlib import Path
import struct

import numpy as np
from PIL import Image

from .library import rgba


@dataclass
class Layer:
    index: int
    group: int
    x: int
    y: int
    image: Image.Image
    raw: bytes


@dataclass
class Sprite:
    path: Path
    sha256: str
    canvas: tuple
    records: list
    layers: list


def straight_alpha(image):
    """遵循 GARbro 的整数除法，恢复 PNA PNG 中预乘的 RGB，保留 alpha。"""
    pixels = np.array(image)
    alpha = pixels[:, :, 3].astype(np.uint32)
    partial = (alpha > 0) & (alpha < 255)
    rgb = pixels[:, :, :3].astype(np.uint32)
    rgb[partial] = rgb[partial] * 255 // alpha[partial, None]
    pixels[:, :, :3] = rgb.astype(np.uint8)
    return Image.fromarray(pixels)


def read_pna(path):
    """验证完整 PNAP 表和负载边界，返回独立解码后的图层。"""
    path = Path(path)
    if path.stat().st_size > 512 * 1024 * 1024:
        raise ValueError('PNA 容器超过 512 MiB')
    data = path.read_bytes()
    if len(data) < 20 or data[:4] != b'PNAP':
        raise ValueError(f'不支持的 PNA 签名或截断头部 {path.name}')
    _, _, width, height, count = struct.unpack_from('<5I', data)
    if not (0 < width * height <= 40_000_000 and 0 < count <= 10000):
        raise ValueError('无效的 PNA 画布或记录数量')
    offset = 20 + count * 40
    if offset > len(data):
        raise ValueError('PNA 记录表截断')
    records, layers = [], []
    group = -1
    for index in range(count):
        words = struct.unpack_from('<10i', data, 20 + index * 40)
        kind, _, x, y, w, h, _, _, _, size = words
        if kind == 1:
            group += 1
        records.append(dict(index=index, group=group, words=list(words), payload_offset=offset))
        if size < 0 or offset + size > len(data):
            raise ValueError('PNA 图片负载越界')
        if size:
            raw = data[offset:offset + size]
            if raw[:8] != b'\x89PNG\r\n\x1a\n':
                raise ValueError('此适配器只支持 PNG 负载')
            image = rgba(BytesIO(raw))
            if image.size != (w, h) or min(x, y) < 0 or x + w > width or y + h > height:
                raise ValueError('PNA 图片尺寸或偏移越过画布')
            layers.append(Layer(index, group, x, y, straight_alpha(image), raw))
        offset += size
    if offset != len(data) or not layers:
        raise ValueError('PNA 存在未解析的尾部或没有图片')
    return Sprite(path, hashlib.sha256(data).hexdigest(), (width, height), records, layers)


def bases(sprite):
    """按图层高度挑选身体候选，容器中较小的局部图层留待几何筛选。"""
    height = max(layer.image.height for layer in sprite.layers)
    return [layer for layer in sprite.layers if layer.image.height >= height * 0.65]


def on_canvas(sprite, layer):
    image = Image.new('RGBA', sprite.canvas)
    image.paste(layer.image, (layer.x, layer.y))
    return image


def face_layers(sprite, base):
    """筛选位于身体上方 35% 内的小图层，候选关系仍须查看预览确认。"""
    return [layer for layer in sprite.layers
            if layer.group != base.group
            and layer.image.height < base.image.height * 0.4
            and layer.y >= base.y
            and layer.y + layer.image.height <= base.y + base.image.height * 0.35
            and layer.x >= base.x
            and layer.x + layer.image.width <= base.x + base.image.width]

"""读取当前库索引与旧内嵌库，显式定位当前选择的本地图片。"""
import base64
import hashlib
from io import BytesIO
import json
from pathlib import Path
import re
import stat

from PIL import Image


def rgba(source):
    """解码独立 RGBA 副本，限制异常尺寸，及时关闭输入。"""
    with Image.open(source) as image:
        if image.width * image.height > 40_000_000:
            raise ValueError('图片超过 4000 万像素')
        image.load()
        return image.convert('RGBA')


def character_directory(library, character_id):
    """与 core::archive 共用罗马字及旧摘要目录的定位规则。"""
    root = Path(library).parent
    identity = hashlib.sha256(character_id.encode()).hexdigest()
    legacy = root / ('character-' + identity)
    paths = [path for path in root.iterdir()
             if path.name == legacy.name or path.name.endswith('--' + identity)]
    if len(paths) > 1:
        raise ValueError('同一角色有多个资料目录')
    if paths:
        metadata = paths[0].lstat()
        if not stat.S_ISDIR(metadata.st_mode) or getattr(metadata, 'st_file_attributes', 0) & 0x400:
            raise ValueError('角色资料目录无效')
        return paths[0]
    return legacy


def library_reference(library, character_id, assets_root=None, artwork_index=None, prefer_enhanced=False):
    """只读取得指定身份当前立绘，索引覆盖归档中的旧 selected_art。"""
    library = Path(library).resolve(strict=True)
    data = json.loads(library.read_text(encoding='utf-8-sig'))
    if data.get('format_version') not in (None, 2):
        raise ValueError('不支持的角色库版本')
    entries = [entry for entry in data['characters'] if entry['id'] == character_id]
    if len(entries) != 1:
        raise ValueError('角色 ID 不存在或重复')
    entry = entries[0]
    profile_path = library
    character = entry
    if data.get('format_version') == 2:
        digest = entry['profile']
        if not re.fullmatch(r'[0-9a-f]{64}', digest):
            raise ValueError('无效的角色归档摘要')
        profile_path = character_directory(library, character_id) / f'profile-{digest}.json'
        payload = profile_path.read_bytes()
        if hashlib.sha256(payload).hexdigest() != digest:
            raise ValueError('角色归档摘要不一致')
        character = json.loads(payload)
        if character['id'] != character_id:
            raise ValueError('角色归档身份不一致')
    index = entry.get('selected_art', character.get('selected_art', 0))
    if artwork_index is not None:
        index = artwork_index
    artworks = character['artworks']
    if not isinstance(index, int) or not 0 <= index < len(artworks):
        raise ValueError('所选立绘下标越界')
    art = artworks[index]
    # 使用原尺寸透明图，避免超分纹理影响匹配；只在缺少 url 时使用 enhanced。
    value = (art.get('enhanced') if prefer_enhanced else None) or art.get('url') or art.get('enhanced')
    if not value:
        raise ValueError('所选立绘没有本地图片')
    if value.startswith('data:image/') and ';base64,' in value:
        payload = base64.b64decode(value.split(';base64,', 1)[1], validate=True)
        image = rgba(BytesIO(payload))
        image_source = 'embedded'
    else:
        if '://' in value:
            raise ValueError('离线工具需要已保存的图片，当前立绘只有远程 URL')
        path = Path(value)
        if not path.is_absolute():
            if assets_root is None:
                raise ValueError('相对资源路径需要 --assets-root 指定包含 assets 的目录')
            root = Path(assets_root).resolve(strict=True)
            path = (root / path).resolve(strict=True)
            if not path.is_relative_to(root):
                raise ValueError('图片路径越过资源目录')
        payload = path.read_bytes()
        image = rgba(BytesIO(payload))
        image_source = str(path.resolve())
    return image, dict(library=str(library), profile=str(profile_path),
                      character_id=character_id, character_name=character.get('name'),
                      artwork_index=index, artwork_id=art.get('id'), title=art.get('title'),
                      original=art.get('original'), image_source=image_source,
                      sha256=hashlib.sha256(payload).hexdigest())

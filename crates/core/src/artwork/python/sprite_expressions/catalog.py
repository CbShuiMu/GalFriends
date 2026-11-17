"""角色专属表情素材归档；先完整复制校验，再原子发布目录索引。"""
import hashlib
import json
import os
from contextlib import contextmanager
from pathlib import Path
import re
import tempfile

from .library import character_directory, library_reference
from .pipeline import checked_path, write_json


@contextmanager
def catalog_lock(path):
    """短期文件锁，进程退出自动释放；锁文件保留以避免删除竞争。"""
    with checked_path(path).open('a+b') as stream:
        if stream.tell() == 0:
            stream.write(b'0')
            stream.flush()
        stream.seek(0)
        if os.name == 'nt':
            import msvcrt
            msvcrt.locking(stream.fileno(), msvcrt.LK_NBLCK, 1)
        else:
            import fcntl
            fcntl.flock(stream.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
        try:
            yield
        finally:
            stream.seek(0)
            if os.name == 'nt':
                msvcrt.locking(stream.fileno(), msvcrt.LK_UNLCK, 1)
            else:
                fcntl.flock(stream.fileno(), fcntl.LOCK_UN)


def catalog_path(library, character_id):
    return checked_path(character_directory(library, character_id) / 'expressions/catalog.json')


def load_catalog(path, character_id):
    path = checked_path(path)
    if not path.exists():
        return dict(format_version=1, character_id=character_id, packs=[])
    data = json.loads(path.read_text(encoding='utf-8'))
    if data.get('format_version') != 1 or data.get('character_id') != character_id:
        raise ValueError('表情目录版本或角色身份不一致')
    if not isinstance(data.get('packs'), list) or len(data['packs']) > 128:
        raise ValueError('表情素材目录无效')
    return data


def pack_files(root):
    """拒绝链接和超大素材包，摘要覆盖原始图层、身体及清单。"""
    root = checked_path(root)
    files = {}
    total = 0
    for path in sorted(root.rglob('*')):
        path = checked_path(path)
        if not path.is_relative_to(root):
            raise ValueError('表情素材路径越界')
        if path.is_file():
            total += path.stat().st_size
            if len(files) >= 1024 or total > 512 * 1024 * 1024:
                raise ValueError('表情素材包过大')
            files[path.relative_to(root).as_posix()] = hashlib.sha256(path.read_bytes()).hexdigest()
    if 'manifest.json' not in files:
        raise ValueError('缺少表情清单')
    return files


def pack_digest(files):
    return hashlib.sha256(json.dumps(files, sort_keys=True).encode()).hexdigest()


def manifests(path, character_id):
    """只返回当前角色目录内已完整发布且所有文件摘要有效的素材。"""
    path = checked_path(path)
    result = []
    for entry in load_catalog(path, character_id)['packs']:
        digest = entry['digest']
        if not re.fullmatch(r'[0-9a-f]{64}', digest):
            raise ValueError('无效的表情素材摘要')
        root = checked_path(path.parent / 'packs' / digest)
        if not root.is_dir() or pack_digest(pack_files(root)) != digest:
            raise ValueError('表情素材缺失或摘要不一致')
        result.append(root / 'manifest.json')
    return result


def register(library, character_id, manifest_paths, assets_root=None, artwork_index=None):
    """归档到现有角色目录，不改写角色库、原始导出或已发布素材。"""
    # 明确核对身份，不能仅凭姓名将外部素材放入角色目录。
    library_reference(library, character_id, assets_root, artwork_index)
    catalog = catalog_path(library, character_id)
    catalog.parent.mkdir(parents=True, exist_ok=True)
    checked_path(catalog.parent)
    lock = checked_path(catalog.parent / '.catalog.lock')
    # 独立 CLI 并发注册时明确失败，避免后写索引丢失先写内容。
    with catalog_lock(lock):
        data = load_catalog(catalog, character_id)
        packs = checked_path(catalog.parent / 'packs')
        packs.mkdir(exist_ok=True)
        # 同一原始 PNA/身体记录的新版预览替换索引，保留旧素材目录以便追溯。
        identities = {}
        for existing in manifests(catalog, character_id):
            value = json.loads(existing.read_text(encoding='utf-8'))
            identities[(value['source_sha256'], value['base_index'])] = existing.parent.name
        for manifest_path in manifest_paths:
            manifest_path = checked_path(manifest_path)
            if manifest_path.name != 'manifest.json':
                raise ValueError('表情清单必须为 manifest.json')
            from .choices import prepare
            source = manifest_path.parent
            files = pack_files(source)
            # 所有派生近景均在发布前生成；打开 face 不需要 Python 或图像合成。
            with tempfile.TemporaryDirectory(prefix='.import-', dir=packs) as temporary:
                staging = Path(temporary) / 'pack'
                staging.mkdir()
                for relative, expected in files.items():
                    payload = checked_path(source / relative).read_bytes()
                    if hashlib.sha256(payload).hexdigest() != expected:
                        raise ValueError('复制期间表情素材已变化')
                    output = staging / relative
                    output.parent.mkdir(parents=True, exist_ok=True)
                    output.write_bytes(payload)
                if pack_files(staging) != files:
                    raise ValueError('表情素材复制验证失败')
                prepare(staging)
                prepared_files = pack_files(staging)
                digest = pack_digest(prepared_files)
                target = checked_path(packs / digest)
                if target.exists():
                    if pack_files(target) != prepared_files:
                        raise ValueError('已有表情素材摘要不一致')
                else:
                    staging.rename(target)
            manifest = json.loads(manifest_path.read_text(encoding='utf-8'))
            identity = (manifest['source_sha256'], manifest['base_index'])
            previous = identities.get(identity)
            data['packs'] = [entry for entry in data['packs'] if entry['digest'] != previous]
            if not any(entry['digest'] == digest for entry in data['packs']):
                data['packs'].append(dict(digest=digest))
            identities[identity] = digest
        if len(data['packs']) > 128:
            raise ValueError('表情素材包数量超过 128')
        from .dedup import rebuild
        write_json(catalog, rebuild(catalog, data))
    return catalog

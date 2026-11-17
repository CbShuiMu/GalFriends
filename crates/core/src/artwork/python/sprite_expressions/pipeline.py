"""离线任务编排，结果写入独立新目录，完成清单最后提交。"""
import hashlib
from io import BytesIO
import json
import os
from pathlib import Path
import stat
import tempfile

import numpy as np
from PIL import Image, ImageDraw

from .library import character_directory, rgba
from .matching import alignment_preview, features, match
from .pna import bases, face_layers, on_canvas, read_pna, straight_alpha


def checked_path(value):
    """在规范化前拒绝输入和输出路径上的链接及 Windows 重解析点。"""
    path = Path(os.path.abspath(value))
    for component in [*reversed(path.parents), path]:
        if component.exists() or component.is_symlink():
            metadata = component.lstat()
            if stat.S_ISLNK(metadata.st_mode) or getattr(metadata, 'st_file_attributes', 0) & 0x400:
                raise ValueError(f'路径包含链接或重解析点 {component}')
    resolved = str(path.resolve())
    # Rust 的 Windows canonicalize 使用扩展路径前缀；统一后才能可靠比较目录边界。
    if os.name == 'nt' and resolved.startswith('\\\\?\\'):
        resolved = ('\\\\' + resolved[8:]) if resolved.startswith('\\\\?\\UNC\\') else resolved[4:]
    return Path(resolved)


def write_json(path, value):
    checked_path(path)
    with tempfile.NamedTemporaryFile(mode='w', encoding='utf-8', dir=path.parent,
                                     suffix='.partial', delete=False) as stream:
        temporary = Path(stream.name)
        try:
            json.dump(value, stream, ensure_ascii=False, indent=2)
            stream.flush()
            os.fsync(stream.fileno())
        except BaseException:
            stream.close()
            temporary.unlink()
            raise
    try:
        temporary.replace(path)
    finally:
        temporary.unlink(missing_ok=True)


def walk_error(error):
    raise error


def managed_output(metadata, output, kind):
    """仅为明确角色的固定子目录开放写入，不允许覆盖归档或输入图片。"""
    if not metadata.get('library') or not metadata.get('character_id'):
        return False
    expected = character_directory(metadata['library'], metadata['character_id']) / 'expressions' / kind
    return output == checked_path(expected)


def export_sprite(sprite, base, directory, reference, evidence):
    """导出原生表情层，读取落盘文件按原坐标回贴，逐像素核验。"""
    directory.mkdir()
    original = directory / 'original'
    original.mkdir()
    layers_dir = directory / 'expressions'
    layers_dir.mkdir()
    previews = directory / 'previews'
    previews.mkdir()
    body = on_canvas(sprite, base)
    body.save(directory / 'base.png')
    (original / f'{base.index:03}.png').write_bytes(base.raw)
    alignment_preview(reference, body, evidence).save(directory / 'alignment.jpg')
    expressions = face_layers(sprite, base)
    if not expressions:
        raise ValueError(f'{sprite.path.name} 没有满足几何条件的局部表情层')
    face_box = (min(layer.x for layer in expressions), min(layer.y for layer in expressions),
                max(layer.x + layer.image.width for layer in expressions),
                max(layer.y + layer.image.height for layer in expressions))
    padding = max(10, (face_box[2] - face_box[0]) // 3)
    face_box = (max(0, face_box[0] - padding), max(0, face_box[1] - padding),
                min(sprite.canvas[0], face_box[2] + padding),
                min(sprite.canvas[1], face_box[3] + padding))
    columns = min(7, len(expressions))
    sheet = Image.new('RGB', (columns * 200, ((len(expressions) + columns - 1) // columns) * 240), '#eeeeee')
    draw = ImageDraw.Draw(sheet)
    entries = []
    for index, layer in enumerate(expressions):
        name = f'{layer.index:03}.png'
        (original / name).write_bytes(layer.raw)
        layer.image.save(layers_dir / name)
        expected = body.copy()
        expected.alpha_composite(layer.image, (layer.x, layer.y))
        expected.save(previews / name)
        # 验证实际导出文件和元数据足以重建，包含半透明像素及 PNG 往返。
        restored = rgba(directory / 'base.png')
        restored.alpha_composite(rgba(layers_dir / name), (layer.x, layer.y))
        mismatches = int(np.any(np.asarray(expected) != np.asarray(restored), axis=2).sum())
        raw = (original / name).read_bytes()
        raw_matches = raw == layer.raw
        decoded = straight_alpha(rgba(BytesIO(raw)))
        if mismatches or not raw_matches or decoded.tobytes() != rgba(layers_dir / name).tobytes():
            raise ValueError('图层导出或回贴验证失败')
        thumb = expected.crop(face_box)
        thumb.thumbnail((195, 210))
        x, y = index % columns * 200, index // columns * 240
        sheet.paste(thumb, (x + (200 - thumb.width) // 2, y + 25), thumb)
        draw.text((x + 8, y + 5), f'Layer {layer.index:03}', fill='black')
        entries.append(dict(index=layer.index, group=layer.group, offset=[layer.x, layer.y],
                            size=list(layer.image.size), image=f'expressions/{name}',
                            original=f'original/{name}', preview=f'previews/{name}',
                            raw_sha256=hashlib.sha256(raw).hexdigest(),
                            rgba_sha256=hashlib.sha256(layer.image.tobytes()).hexdigest(),
                            blend='source_over', roundtrip_mismatched_pixels=mismatches))
    sheet.save(directory / 'contact-sheet.jpg')
    manifest = dict(source=str(sprite.path), source_sha256=sprite.sha256, canvas=list(sprite.canvas),
                    base_index=base.index, base_offset=[base.x, base.y],
                    base='base.png', matching=evidence, records=sprite.records,
                    expressions=entries,
                    semantics='local-layer candidates; auxiliary layers may require another face layer',
                    alpha='original PNG uses premultiplied RGB; expressions use straight alpha')
    write_json(directory / 'manifest.json', manifest)
    return manifest


def run(reference, reference_metadata, source, output, base_index=None, head_fraction=0.3):
    """扫描导出的 PNA 并提取通过匹配的姿势，每次生成独立目录。"""
    source = checked_path(source)
    output = checked_path(output)
    protected = [source]
    for key in ('library', 'profile', 'image_source'):
        value = reference_metadata.get(key)
        if key != 'image_source' and managed_output(reference_metadata, output, 'runs'):
            continue
        if value and value != 'embedded':
            path = checked_path(value)
            protected.append(path if key == 'image_source' else path.parent)
    for path in protected:
        if output == path or output.is_relative_to(path) or path.is_relative_to(output):
            raise ValueError('输出目录与输入资源或角色库目录重叠')
    if not source.is_dir():
        raise ValueError('--source 必须是 GARbro 导出的目录')
    paths = []
    for folder, directories, files in os.walk(source, followlinks=False, onerror=walk_error):
        for name in directories + files:
            checked_path(Path(folder) / name)
        paths.extend(Path(folder) / name for name in files if Path(name).suffix.lower() == '.pna')
    if not paths:
        raise ValueError('目录中没有 PNA 文件；先用 GARbro 导出立绘容器')
    output.mkdir(parents=True, exist_ok=True)
    checked_path(output)
    root = Path(tempfile.mkdtemp(prefix='expressions-', dir=output))
    print(f'输出目录 {root}', flush=True)
    report = dict(format_version=1, status='running', reference=reference_metadata,
                  source=str(source), parameters=dict(base_index=base_index, head_fraction=head_fraction),
                  candidates=[], exports=[], failures=[])
    write_json(root / 'report.json', report)
    try:
        reference.save(root / 'reference.png')
        ref_features = features(reference, head_fraction)
        for index, path in enumerate(sorted(paths)):
            print(f'匹配 {index + 1}/{len(paths)} {path.name}', flush=True)
            try:
                sprite = read_pna(path)
                scores = []
                for base in bases(sprite):
                    evidence = match(ref_features, on_canvas(sprite, base))
                    scores.append(dict(base_index=base.index, **evidence))
                report['candidates'].append(dict(source=str(path), sha256=sprite.sha256, bases=scores))
                accepted = [score for score in scores if score['accepted']]
                if base_index is not None:
                    accepted = [score for score in accepted if score['base_index'] == base_index]
                if not accepted:
                    continue
                best = max(accepted, key=lambda score: (score['inliers'], score['inlier_ratio']))
                base = next(layer for layer in sprite.layers if layer.index == best['base_index'])
                # 文件编号与清单绑定，避免源文件名导致路径穿越或跨目录同名覆盖。
                destination = root / f'{index:03}'
                manifest = export_sprite(sprite, base, destination, reference, best)
                if hashlib.sha256(path.read_bytes()).hexdigest() != sprite.sha256:
                    raise ValueError('处理期间源 PNA 已变化')
                report['exports'].append(dict(source=str(path), directory=destination.name,
                                              base_index=base.index, expressions=len(manifest['expressions'])))
            except (OSError, ValueError) as error:
                report['failures'].append(dict(source=str(path), error=str(error)))
        if report['failures']:
            report['status'] = 'partial_failure'
        elif report['exports']:
            report['status'] = 'complete'
        else:
            report['status'] = 'no_match'
    except BaseException as error:
        report['status'] = 'interrupted' if isinstance(error, KeyboardInterrupt) else 'failed'
        report['failures'].append(dict(error=str(error)))
        write_json(root / 'report.json', report)
        raise
    write_json(root / 'report.json', report)
    return root, report

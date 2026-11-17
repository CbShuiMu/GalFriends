"""在当前立绘上替换面部，保留遮罩外 RGBA，重新匹配每张参考图。"""
import hashlib
import json
from pathlib import Path
import tempfile

import cv2
import numpy as np
from PIL import Image, ImageDraw

from .library import rgba
from .matching import features, match
from .pipeline import checked_path, managed_output, write_json


def asset(root, relative):
    path = checked_path(root / relative)
    if not path.is_relative_to(root) or not path.is_file():
        raise ValueError('表情清单的图片路径越界或不存在')
    return rgba(path)


def composite(reference, display, body, entries, variants):
    """返回合成图、遮罩和匹配证据；只允许在完全不透明的面部内替换。"""
    evidence = match(features(reference, 0.3), body)
    if not evidence['accepted']:
        raise ValueError('expression.no_match')
    scale = display.width / reference.width
    if abs(display.height / reference.height - scale) > 0.001:
        raise ValueError('显示立绘与匹配立绘的比例不同')
    transform = np.array(evidence['transform']) * scale
    transform[:, 2] += (scale - 1) / 2
    warp = lambda pixels: cv2.warpAffine(pixels, transform, display.size, flags=cv2.INTER_LANCZOS4)
    footprint = np.zeros((body.height, body.width), np.uint8)
    body_box = body.getbbox()
    if body_box is None:
        raise ValueError('身体层没有前景')
    for layer, (x, y) in entries.values():
        if min(x, y) < 0 or x + layer.width > body.width or y + layer.height > body.height:
            raise ValueError('表情偏移超出身体画布')
        if y < body_box[1] or y + layer.height > body_box[1] + (body_box[3] - body_box[1]) * 0.35:
            raise ValueError('图层超出面部范围')
        area = footprint[y:y + layer.height, x:x + layer.width]
        np.maximum(area, np.asarray(layer.getchannel('A')), out=area)
    mask = np.uint8(warp(footprint) > 18) * 255
    interior = mask > 0
    radius = max(1, round(7 * scale / 2))
    kernel = 2 * radius + 1
    mask = cv2.dilate(mask, cv2.getStructuringElement(cv2.MORPH_ELLIPSE, (kernel, kernel)))
    mask = cv2.GaussianBlur(mask, (kernel, kernel), max(0.5, 2.8 * scale / 2))
    mask[interior] = 255
    ys, xs = np.nonzero(mask)
    pixels = np.array(display)
    if not len(ys) or np.any(pixels[ys, xs, 3] != 255):
        raise ValueError('对齐后的面部遮罩越过人物不透明区域，请更换同姿势素材')
    weight = mask[ys, xs, None].astype(np.uint16)
    results = []
    for indices in variants:
        composed = body.copy()
        for index in indices:
            layer, offset = entries[index]
            composed.alpha_composite(layer, offset)
        aligned = warp(np.array(composed)[:, :, :3].copy())
        result = pixels.copy()
        result[ys, xs, :3] = ((aligned[ys, xs] * weight + pixels[ys, xs, :3] * (255 - weight) + 127) // 255).astype(np.uint8)
        if not np.array_equal(result[mask == 0], pixels[mask == 0]) or not np.array_equal(result[:, :, 3], pixels[:, :, 3]):
            raise ValueError('合成改变了面部范围外的像素或透明通道')
        results.append(Image.fromarray(result))
    return results, mask, evidence


def read_manifest(manifest_path):
    """统一验证提取清单、身体层和局部表情的像素摘要。"""
    manifest_path = checked_path(manifest_path)
    root = manifest_path.parent
    manifest = json.loads(manifest_path.read_text(encoding='utf-8'))
    body = asset(root, manifest['base'])
    entries = {}
    for entry in manifest['expressions']:
        index = entry['index']
        if index in entries or not isinstance(index, int):
            raise ValueError('重复或无效的图层编号')
        image = asset(root, entry['image'])
        if hashlib.sha256(image.tobytes()).hexdigest() != entry['rgba_sha256']:
            raise ValueError('表情图片摘要与清单不一致')
        entries[index] = (image, tuple(entry['offset']))
    if not entries or len(entries) > 64:
        raise ValueError('表情数量需位于 1–64')
    return body, entries


def select_manifest(reference, catalog, character_id):
    """从通过同姿势门槛的素材中优先使用大尺寸图，避免重复列出大小差分。"""
    from .catalog import manifests
    ref_features = features(reference, 0.3)
    candidates = []
    for path in manifests(catalog, character_id):
        body, _ = read_manifest(path)
        evidence = match(ref_features, body)
        if evidence['accepted']:
            candidates.append(((body.width, evidence['inliers'], evidence['inlier_ratio']), path))
    if not candidates:
        raise ValueError('expression.no_match')
    return max(candidates, key=lambda item: item[0])[1]


def default_variants(entries):
    """原始预览与替换共用组合：局部腮红放在完整五官后方。"""
    height = max(image.height for image, _ in entries.values())
    full = [i for i, (image, _) in entries.items() if image.height >= height * 0.6]
    return [[i] for i in full] + [[i, full[-1]] for i in entries if i not in full]


def compose_run(reference, display, metadata, manifest_path, output, variants=None):
    """验证已导出清单后生成新预览目录，清单在完整验证后提交。"""
    manifest_path = checked_path(manifest_path)
    root = manifest_path.parent
    output = checked_path(output)
    for key in ('library', 'profile', 'image_source'):
        value = metadata.get(key)
        if key != 'image_source' and managed_output(metadata, output, 'previews'):
            continue
        if value and value != 'embedded':
            protected = checked_path(value)
            if key != 'image_source':
                protected = protected.parent
            if output.is_relative_to(protected) or protected.is_relative_to(output):
                raise ValueError('合成输出与参考图片或角色库重叠')
    if output.is_relative_to(root) or root.is_relative_to(output):
        raise ValueError('合成输出与表情素材目录重叠')
    body, entries = read_manifest(manifest_path)
    if variants is None:
        variants = default_variants(entries)
    if not variants or len(variants) > 64 or any(not v or any(i not in entries for i in v) for v in variants):
        raise ValueError('无效的图层组合')
    images, mask, evidence = composite(reference, display, body, entries, variants)
    output.mkdir(parents=True, exist_ok=True)
    destination = Path(tempfile.mkdtemp(prefix='composed-', dir=output))
    report = dict(status='running', reference=metadata, source_manifest=str(manifest_path),
                  source=Path(json.loads(manifest_path.read_text(encoding='utf-8'))['source']).name,
                  matching=evidence, alpha_preserved=True, outside_mask_pixel_differences=0, variants=[])
    write_json(destination / 'manifest.json', report)
    try:
        display.save(destination / 'reference.png')
        Image.fromarray(mask).save(destination / 'face-mask.png')
        ys, xs = np.nonzero(mask)
        bounds = (max(0, int(xs.min()) - 24), max(0, int(ys.min()) - 35),
                  min(display.width, int(xs.max()) + 25), min(display.height, int(ys.max()) + 30))
        sheet = Image.new('RGB', (4 * 240, ((len(images) + 4) // 4) * 220), '#eeeeee')
        draw = ImageDraw.Draw(sheet)
        for number, image in enumerate([display, *images]):
            thumb = image.crop(bounds)
            thumb.thumbnail((230, 185), Image.Resampling.LANCZOS)
            x, y = number % 4 * 240, number // 4 * 220
            draw.text((x + 8, y + 5), 'Reference' if number == 0 else ' + '.join(map(str, variants[number - 1])), fill='black')
            sheet.paste(thumb, (x + 5, y + 28), thumb)
            if number:
                filename = f'{number:03}.png'
                image.save(destination / filename)
                if rgba(destination / filename).tobytes() != image.tobytes():
                    raise ValueError('输出 PNG 往返验证失败')
                thumbnail = f'face-{number:03}.png'
                thumb.save(destination / thumbnail)
                report['variants'].append(dict(file=filename, thumbnail=thumbnail, layers=variants[number - 1]))
        sheet.save(destination / 'preview.png')
        report['status'] = 'complete'
    except BaseException:
        report['status'] = 'failed'
        raise
    finally:
        write_json(destination / 'manifest.json', report)
    return destination, report

"""以原生面部像素合并分辨率差分；原始素材保留供来源追溯。"""
import hashlib
import json

import numpy as np
from PIL import Image, ImageFilter

from .compose import read_manifest
from .pipeline import checked_path, write_json


def descriptors(directory):
    body, entries = read_manifest(directory / 'manifest.json')
    choices = json.loads((directory / 'choices.json').read_text('utf-8'))['variants']
    height = max(image.height for image, _ in entries.values())
    full = [(image, offset) for image, offset in entries.values() if image.height >= height * .6]
    bounds = (min(x for _, (x, y) in full), min(y for _, (x, y) in full),
              max(x + image.width for image, (x, y) in full),
              max(y + image.height for image, (x, y) in full))
    result = []
    for index, choice in enumerate(choices):
        face = Image.new('RGBA', body.size)
        for layer in choice['layers']:
            face.alpha_composite(*entries[layer])
        face = face.crop(bounds)
        exact = hashlib.sha256(str(face.size).encode() + face.tobytes()).hexdigest()
        normalized = face.resize((64, 64), Image.Resampling.LANCZOS).filter(ImageFilter.GaussianBlur(.7))
        pixels = np.asarray(normalized, dtype=np.float32).copy()
        pixels[:, :, :3] *= pixels[:, :, 3:] / 255
        result.append(dict(index=index, exact=exact, pixels=pixels, quality=face.width * face.height))
    return result


def rebuild(path, data):
    """互为最佳且明显优于其他表情才合并；保留稳定 ID 和最高原生分辨率。"""
    previous = { (origin['digest'], origin['index']): item['id']
                 for item in data.get('unique', []) for origin in item['origins'] }
    unique = []
    representatives = []
    for pack in data['packs']:
        digest = pack['digest']
        candidates = descriptors(checked_path(path.parent / 'packs' / digest))
        distances = np.array([[float(np.abs(c['pixels'] - r['pixels']).mean())
                               for r in representatives] for c in candidates])
        for row, candidate in enumerate(candidates):
            origin = dict(digest=digest, index=candidate['index'])
            target = None
            exact = next((i for i, r in enumerate(representatives) if r['exact'] == candidate['exact']), None)
            if exact is not None:
                target = exact
            elif distances.size:
                column = int(distances[row].argmin())
                score = distances[row, column]
                alternatives = [*np.delete(distances[row], column), *np.delete(distances[:, column], row)]
                # 单候选需要更严格的像素证据；绝不按文件名或图层编号去重。
                limit = min(alternatives) * .65 if alternatives else 2.0
                if score < 6 and score < limit and int(distances[:, column].argmin()) == row:
                    target = column
            if target is None:
                identity = previous.get((digest, candidate['index']), candidate['exact'])
                unique.append(dict(id=identity, digest=digest, index=candidate['index'], origins=[origin]))
                representatives.append(candidate)
            else:
                unique[target]['origins'].append(origin)
                if candidate['quality'] > representatives[target]['quality']:
                    unique[target].update(digest=digest, index=candidate['index'])
                    representatives[target] = candidate
    data['unique'] = unique
    data['dedup_version'] = 1
    return data


def migrate(path, character_id):
    from .catalog import catalog_lock, load_catalog, manifests
    if not path.exists():
        return
    with catalog_lock(path.parent / '.catalog.lock'):
        data = load_catalog(path, character_id)
        if data.get('dedup_version') == 1:
            return
        manifests(path, character_id)
        write_json(path, rebuild(path, data))

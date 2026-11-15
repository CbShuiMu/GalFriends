"""应用使用的主人物抠图：人物检测 / SAM 2 实例选择 / 原 ISNet Anime 边缘。

保留 character_art.py 的独立用法、原图、alpha 曲线和人工 keep 修正。
只在环境准备时下载固定版本模型；运行抠图完全离线，不上传图片。
"""

import argparse
from collections import deque
import hashlib
import json
from pathlib import Path

import numpy as np
import onnxruntime as ort
from PIL import Image, ImageFilter

import character_art as legacy


MANIFEST = Path(__file__).with_suffix(".models.json")


def prepare_models(proxy=None):
    """安装阶段下载并校验模型；推理时不调用。"""
    root = legacy.default_model_path().parent
    with legacy.http_session(proxy) as session:
        for item in json.loads(MANIFEST.read_text(encoding="utf-8")):
            path = root / item["file"]
            if not path.is_file() or hashlib.sha256(path.read_bytes()).hexdigest() != item["sha256"]:
                legacy.download_file(session, item["url"], path, item["sha256"])
            print(f"Ready: {path.name}", flush=True)


def components(binary):
    """提取四邻域连通区域；使用副本，不改调用方蒙版。"""
    remaining = np.asarray(binary, dtype=bool).copy()
    height, width = remaining.shape
    for y, x in zip(*np.nonzero(remaining)):
        if not remaining[y, x]:
            continue
        remaining[y, x] = False
        queue = deque([(y, x)])
        pixels = []
        while queue:
            y, x = queue.popleft()
            pixels.append((y, x))
            for ny, nx in ((y - 1, x), (y + 1, x), (y, x - 1), (y, x + 1)):
                if 0 <= ny < height and 0 <= nx < width and remaining[ny, nx]:
                    remaining[ny, nx] = False
                    queue.append((ny, nx))
        yield np.asarray(pixels)


def remove_specks(alpha, radius=3):
    """去掉细桥连接的文字碎片；保留所有较大区域，包括与裙摆分离的鞋子。"""
    # 仅在有限大小的蒙版上找连通区域，避免大画布的 Python 像素遍历。
    size = alpha.size
    working = alpha.copy()
    working.thumbnail((512, 512), Image.Resampling.BILINEAR)
    seeds = np.asarray(working.filter(ImageFilter.MinFilter(radius))) >= 128
    keep = np.zeros(seeds.shape, dtype=np.uint8)
    minimum = max(8, seeds.size * 0.0008)
    for pixels in components(seeds):
        if len(pixels) >= minimum:
            y, x = pixels.T
            keep[y, x] = 255
    support = Image.fromarray(keep).filter(ImageFilter.MaxFilter(radius + 2))
    support = support.resize(size, Image.Resampling.BILINEAR)
    return Image.fromarray(np.minimum(np.asarray(alpha), np.asarray(support)))


def leg_gap(logits, box):
    """只在两个高置信前景区域之间找腿间低置信间隙；长裙和并腿不强加背景点。"""
    height, width = logits.shape
    top, bottom = box[1] * height, box[3] * height
    candidates = []
    for y in range(int(top + (bottom - top) * 0.78), min(height, int(bottom * 0.95))):
        xs = np.flatnonzero(logits[y] > 4)
        if len(xs) < 6:
            continue
        gaps = np.diff(xs)
        index = int(np.argmax(gaps))
        gap = int(gaps[index])
        x = int((xs[index] + xs[index + 1]) // 2)
        if width * 0.02 < gap < width * 0.28 and logits[y, x] < 3.5:
            candidates.append((gap, x, y))
    if not candidates:
        return None
    _, x, y = max(candidates)
    # 已经是透明间隙时不重新提示，防止模型将背景文字重新引入。
    return (x / width, y / height) if logits[y, x] > 0 else None


class ForegroundSegmenter(legacy.AnimeSegmenter):
    def __init__(self, model_path, threads):
        super().__init__(model_path, threads)
        options = ort.SessionOptions()
        options.intra_op_num_threads = threads
        root = model_path.parent
        for item in json.loads(MANIFEST.read_text(encoding="utf-8")):
            if not (root / item["file"]).is_file():
                raise ValueError(f"Missing {item['file']}; run scripts/setup-cutout.ps1")
        self.options = options
        self.root = root
        self.detector = None
        self.encoder = None
        self.decoder = None

    def session_for(self, file):
        return ort.InferenceSession(
            str(self.root / file), sess_options=self.options, providers=["CPUExecutionProvider"]
        )

    def person_box(self, image):
        if self.detector is None:
            self.detector = self.session_for("anime-person.onnx")
        scale = 640 / max(image.size)
        size = tuple(max(1, round(side * scale)) for side in image.size)
        offset = ((640 - size[0]) // 2, (640 - size[1]) // 2)
        sample = Image.new("RGB", (640, 640), (114, 114, 114))
        sample.paste(image.resize(size, Image.Resampling.BILINEAR), offset)
        rgb = np.asarray(sample, dtype=np.float32) / 255
        predictions = self.detector.run(None, {"images": rgb.transpose(2, 0, 1)[None]})[0][0].T
        candidates = []
        for cx, cy, width, height, confidence in predictions:
            if confidence < 0.3 or not np.isfinite([cx, cy, width, height, confidence]).all():
                continue
            box = np.array([
                cx - width / 2 - offset[0], cy - height / 2 - offset[1],
                cx + width / 2 - offset[0], cy + height / 2 - offset[1],
            ]) / scale / np.array([*image.size, *image.size])
            box = np.clip(box, 0, 1)
            # 优先高而完整的人物，降低背景大头像的选择权重。
            score = confidence * (box[3] - box[1]) * (1 - abs((box[0] + box[2]) / 2 - 0.5) * 0.3)
            candidates.append((float(score), box))
        if not candidates:
            return None
        box = max(candidates, key=lambda entry: entry[0])[1]
        # 检测框可能含海报大头像。收窄的是 SAM 提示框，绝不硬裁掉画布或框外发丝。
        inset = (box[2] - box[0]) * 0.15
        box[0] += inset
        box[2] -= inset
        return box

    def predict(self, features, box, points, labels):
        _, masks, _ = self.decoder.run(None, features | {
            "input_points": np.array([[points]], dtype=np.float32) * 1024,
            "input_labels": np.array([[labels]], dtype=np.int64),
            "input_boxes": np.array([[box]], dtype=np.float32) * 1024,
        })
        if not np.isfinite(masks).all():
            raise ValueError("Invalid foreground mask")
        return masks[0, 0]

    def mask(self, image):
        original = super().mask(image)
        box = self.person_box(image)
        if box is None:
            # 头像等无完整人物检测结果的图片沿用原算法。
            return original
        if self.encoder is None:
            self.encoder = self.session_for("sam2.1-base-plus/vision_encoder.onnx")
            self.decoder = self.session_for("sam2.1-base-plus/prompt_encoder_mask_decoder.onnx")
        rgb = np.asarray(image.resize((1024, 1024), Image.Resampling.BILINEAR), dtype=np.float32) / 255
        rgb = (rgb - legacy.MEAN) / np.array([0.229, 0.224, 0.225], dtype=np.float32)
        outputs = self.encoder.run(None, {"pixel_values": rgb.transpose(2, 0, 1)[None]})
        features = dict(zip((output.name for output in self.encoder.get_outputs()), outputs))
        point = [(box[0] + box[2]) / 2, box[1] + (box[3] - box[1]) * 0.4]
        # 固定导出的第一个候选对带框主人物提示更完整；IoU 最高的候选可能只是衣服。
        logits = self.predict(features, box, [point], [1])[0]
        old = np.asarray(original.resize((256, 256), Image.Resampling.BILINEAR)) > 128
        foreground = logits > 0
        if foreground.mean() < 0.01:
            raise ValueError("Main character segmentation found no usable foreground")
        # 简单背景维持原 ISNet 的细发丝和透明边缘，不为已干净的图再次收紧轮廓。
        if np.count_nonzero(old & ~foreground) <= np.count_nonzero(old) * 0.18:
            return original
        gap = leg_gap(logits, box)
        lower_start = None
        support = Image.fromarray(np.uint8(logits > 0) * 255)
        if gap is not None:
            refined = self.predict(features, box, [point, gap], [1, 0])[2]
            lower_start = max(0, round((gap[1] - (box[3] - box[1]) * 0.22) * logits.shape[0]))
            # 背景点只收紧脚边局部，不能让一次局部提示误删头发、脸和上身。
            local = Image.fromarray(np.uint8(refined > 1) * 255)
            local = local.filter(ImageFilter.MaxFilter(3)).filter(ImageFilter.MinFilter(3))
            combined = np.minimum(np.asarray(support), np.asarray(local))
            support.paste(Image.fromarray(combined).crop((0, lower_start, 256, 256)), (0, lower_start))
        # SAM 置信分数只决定实例归属，不能当作人物内部的透明度；软边缘交给原 ISNet。
        selected = np.asarray(support.resize(image.size, Image.Resampling.BILINEAR))
        alpha = Image.fromarray(np.minimum(selected, np.asarray(original)))
        clean = remove_specks(alpha)
        if lower_start is not None:
            lower_y = round(lower_start * image.height / 256)
            local = remove_specks(alpha, radius=5)
            clean.paste(local.crop((0, lower_y, image.width, image.height)), (0, lower_y))
        if np.count_nonzero(np.asarray(clean) > 128) < np.count_nonzero(selected > 128) * 0.65:
            raise ValueError("Foreground cleanup removed too much of the character")
        return clean

    def cutout(self, image, alpha_curve, keep_polygons):
        if keep_polygons:
            # 人工修正过的旧样本保持原算法，避免新模型改变已校准的脚部像素。
            original_mask = super().mask(image.convert("RGB"))
            # 通过独立适配器复用原 cutout；不临时修改实例方法或全局状态。
            return FixedMaskSegmenter(original_mask).cutout(image, alpha_curve, keep_polygons)
        return super().cutout(image, alpha_curve, keep_polygons)


class FixedMaskSegmenter(legacy.AnimeSegmenter):
    def __init__(self, alpha):
        self.alpha = alpha

    def mask(self, image):
        return self.alpha


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["cutout"])
    parser.add_argument("input", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--model", required=True, type=Path)
    parser.add_argument("--keep", required=True, type=Path)
    parser.add_argument("--threads", type=int, default=2)
    args = parser.parse_args()
    if args.threads < 1:
        parser.error("--threads must be positive")
    segmenter = ForegroundSegmenter(args.model, args.threads)
    keep = json.loads(args.keep.read_text(encoding="utf-8-sig"))
    legacy.cutout_directory(args.input, args.output, segmenter, set(), (38, 225), keep)


if __name__ == "__main__":
    main()

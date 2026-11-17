"""用局部特征及相似变换匹配跨服装立绘，输出证据与置信门槛。"""
import math

import cv2
import numpy as np
from PIL import Image


def features(image, head_fraction):
    """统一工作尺寸，屏蔽透明区及头部以下区域，保留到原图的缩放。"""
    scale = min(1.0, 1400 / max(image.size))
    working = image.resize((round(image.width * scale), round(image.height * scale)))
    pixels = np.asarray(working)
    alpha = pixels[:, :, 3]
    foreground = alpha > 128
    ys, xs = np.nonzero(foreground)
    if not len(ys):
        raise ValueError('参考图或候选图没有可见前景')
    top, bottom = int(ys.min()), int(ys.max()) + 1
    mask = np.uint8(foreground) * 255
    mask[top + round((bottom - top) * head_fraction):] = 0
    # 在统一浅色底上计算梯度，屏蔽透明区中的未定义 RGB。
    rgb = pixels[:, :, :3].astype(np.float32)
    weight = alpha[:, :, None] / 255.0
    gray = cv2.cvtColor(np.uint8(rgb * weight + 240 * (1 - weight)), cv2.COLOR_RGB2GRAY)
    keypoints, descriptors = cv2.SIFT_create(nfeatures=2500, contrastThreshold=0.02).detectAndCompute(gray, mask)
    return keypoints, descriptors, scale, (int(xs.min()), top, int(xs.max()) + 1, bottom)


def match(reference_features, candidate, head_fraction=0.35):
    """估计候选原画布到参考图的等比缩放、旋转和平移；不声称识别人类骨骼。"""
    return match_features(reference_features, features(candidate, head_fraction))


def match_features(reference_features, candidate_features):
    """复用已计算特征，使同一个候选匹配多张已保存立绘时不重复运行 SIFT。"""
    ref_keys, ref_desc, ref_scale, bounds = reference_features
    keys, desc, scale, _ = candidate_features
    result = dict(inliers=0, matches=0, inlier_ratio=0.0, accepted=False)
    if ref_desc is None or desc is None or min(len(ref_desc), len(desc)) < 2:
        return result
    pairs = cv2.BFMatcher().knnMatch(desc, ref_desc, k=2)
    good = [first for first, second in pairs if first.distance < 0.72 * second.distance]
    # SIFT 可能在同一位置产生多个方向，独立位置才算几何证据。
    unique = {}
    for item in sorted(good, key=lambda item: item.distance):
        position = tuple(round(v) for v in ref_keys[item.trainIdx].pt)
        unique.setdefault(position, item)
    good = list(unique.values())
    result['matches'] = len(good)
    if len(good) < 4:
        return result
    source = np.float32([keys[item.queryIdx].pt for item in good])
    target = np.float32([ref_keys[item.trainIdx].pt for item in good])
    cv2.setRNGSeed(0)
    matrix, mask = cv2.estimateAffinePartial2D(source, target, method=cv2.RANSAC,
                                             ransacReprojThreshold=3, maxIters=3000)
    if matrix is None:
        return result
    inliers = mask.ravel().astype(bool)
    count = int(inliers.sum())
    ratio = count / len(good)
    positions = target[inliers]
    spread = np.ptp(positions, axis=0)
    angle = math.degrees(math.atan2(matrix[1, 0], matrix[0, 0]))
    matrix[:, :2] *= scale / ref_scale
    matrix[:, 2] /= ref_scale
    predicted = source[inliers] / scale @ matrix[:, :2].T + matrix[:, 2]
    error = float(np.median(np.linalg.norm(predicted - target[inliers] / ref_scale, axis=1)))
    result.update(inliers=count, inlier_ratio=round(ratio, 4), angle=round(angle, 3),
                  median_error=round(error, 4), transform=matrix.tolist(),
                  reference_points=(target[inliers] / ref_scale).round(2).tolist(),
                  candidate_points=(source[inliers] / scale).round(2).tolist(),
                  accepted=bool(count >= 8 and ratio >= 0.45 and abs(angle) <= 8
                                and spread[0] >= (bounds[2] - bounds[0]) * 0.12
                                and spread[1] >= (bounds[3] - bounds[1]) * 0.025))
    return result


def alignment_preview(reference, candidate, evidence):
    """并排显示参考图、对齐候选与叠加图，供检查头部和姿势。"""
    aligned = Image.fromarray(cv2.warpAffine(np.asarray(candidate),
                             np.array(evidence['transform'], dtype=np.float64), reference.size))
    result = Image.new('RGB', (reference.width * 3, reference.height), '#eeeeee')
    for index, image in enumerate((reference, aligned, Image.blend(reference, aligned, 0.5))):
        result.paste(image, (index * reference.width, 0), image)
    result.thumbnail((1500, 1000))
    return result

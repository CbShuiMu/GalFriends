"""离线验证背景碎片清理的关键边界，不下载模型，不依赖真实角色库。"""
import unittest

import numpy as np
from PIL import Image

from character_foreground import FixedMaskSegmenter, leg_gap, remove_specks


class CleanupTests(unittest.TestCase):
    def test_keeps_disconnected_shoes_and_removes_small_caption_fragments(self):
        alpha = np.zeros((256, 160), dtype=np.uint8)
        alpha[10:190, 40:120] = 255
        alpha[205:240, 43:61] = 255
        alpha[205:240, 93:111] = 255
        alpha[210:213, 72:75] = 255
        before = alpha.copy()
        result = np.asarray(remove_specks(Image.fromarray(alpha)))
        self.assertEqual(result[220, 50], 255)
        self.assertEqual(result[220, 100], 255)
        self.assertEqual(result[100, 80], 255)
        self.assertEqual(result[211, 73], 0)
        np.testing.assert_array_equal(alpha, before)

    def test_does_not_invent_leg_gap_in_long_dress_or_already_clean_gap(self):
        logits = np.full((256, 256), -10, dtype=np.float32)
        logits[10:250, 60:196] = 6
        box = (0.2, 0, 0.8, 1)
        self.assertIsNone(leg_gap(logits, box))
        logits[190:, 110:146] = -3
        self.assertIsNone(leg_gap(logits, box))
        logits[190:, 110:146] = 1.5
        x, y = leg_gap(logits, box)
        self.assertTrue(110 <= x * 256 < 146)
        self.assertGreater(y, 0.78)

    def test_fixed_mask_keeps_original_rgb_and_existing_transparency(self):
        image = Image.new("RGBA", (4, 4), (20, 50, 80, 255))
        alpha = Image.new("L", image.size, 255)
        alpha.putpixel((0, 0), 0)
        result = FixedMaskSegmenter(alpha).cutout(image, list(range(256)), [])
        self.assertEqual(result.getpixel((1, 1)), (20, 50, 80, 255))
        self.assertEqual(result.getpixel((0, 0)), (0, 0, 0, 0))
        image.putpixel((2, 2), (20, 50, 80, 90))
        self.assertEqual(FixedMaskSegmenter(alpha).cutout(image, list(range(256)), []).tobytes(), image.tobytes())


if __name__ == "__main__":
    unittest.main()

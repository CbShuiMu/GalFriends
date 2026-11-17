"""使用隔离的合成图片验证原生差分、旧库选择与跨服装匹配。"""
import base64
import hashlib
from io import BytesIO
import json
from pathlib import Path
import struct
import sys
import tempfile
import unittest
from unittest.mock import patch

import numpy as np
from PIL import Image

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'crates/core/src/artwork/python'))
from sprite_expressions.library import library_reference
from sprite_expressions.cli import main
from sprite_expressions.matching import features, match
from sprite_expressions.pipeline import export_sprite, run
from sprite_expressions.pna import bases, read_pna, straight_alpha
from sprite_expressions.compose import composite
from sprite_expressions.catalog import catalog_path, manifests, register


def png(image):
    stream = BytesIO()
    image.save(stream, format='PNG')
    return stream.getvalue()


def pna_fixture():
    face = png(Image.new('RGBA', (30, 20), (50, 20, 10, 128)))
    body = png(Image.new('RGBA', (70, 280), (100, 150, 200, 255)))
    records = [(1, -1, 0, 0, 0, 0, 0, 0, 0, 0),
               (0, 2, 25, 35, 30, 20, 0, 0, 0, len(face)),
               (2, -1, 0, 0, 0, 0, 0, 0, 0, 0),
               (1, -1, 0, 0, 0, 0, 0, 0, 0, 0),
               (0, 1, 10, 10, 70, 280, 0, 0, 0, len(body))]
    return struct.pack('<4s4I', b'PNAP', 220, 100, 300, len(records)) + b''.join(
        struct.pack('<10i', *record) for record in records) + face + body


class ExpressionTests(unittest.TestCase):
    def test_catalog_is_character_owned_retryable_and_rejects_modified_assets(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            library = root / 'characters/library.json'
            library.parent.mkdir()
            artwork = 'data:image/png;base64,' + base64.b64encode(png(Image.new('RGBA', (100, 300), 'white'))).decode()
            library.write_text(json.dumps(dict(characters=[dict(id='test:1', artworks=[dict(url=artwork)])])), encoding='utf-8')
            before = library.read_bytes()
            source = root / 'sprite.pna'
            source.write_bytes(pna_fixture())
            sprite = read_pna(source)
            export_sprite(sprite, bases(sprite)[0], root / 'export', Image.new('RGBA', (100, 300)),
                          dict(transform=[[1, 0, 0], [0, 1, 0]]))
            original_manifest = (root / 'export/manifest.json').read_bytes()
            # 发布失败不留下可见的半成品索引，文件锁也必须释放，支持重试。
            with patch('sprite_expressions.catalog.write_json', side_effect=OSError('disk full')):
                with self.assertRaises(OSError):
                    register(library, 'test:1', [root / 'export/manifest.json'])
            self.assertFalse(catalog_path(library, 'test:1').exists())
            path = register(library, 'test:1', [root / 'export/manifest.json'])
            register(library, 'test:1', [root / 'export/manifest.json'])
            saved = manifests(path, 'test:1')
            self.assertEqual(len(saved), 1)
            self.assertTrue(saved[0].is_relative_to(library.parent / ('character-' + hashlib.sha256(b'test:1').hexdigest())))
            self.assertEqual(saved[0].read_bytes(), original_manifest)
            choices = json.loads((saved[0].parent / 'choices.json').read_text(encoding='utf-8'))
            self.assertEqual(choices['variants'][0]['layers'], [1])
            self.assertTrue((saved[0].parent / 'choices' / choices['variants'][0]['thumbnail']).is_file())
            self.assertEqual(library.read_bytes(), before)
            with self.assertRaisesRegex(ValueError, '身份'):
                manifests(path, 'test:2')
            (saved[0].parent / 'base.png').write_bytes(b'corrupt')
            with self.assertRaisesRegex(ValueError, '摘要'):
                manifests(path, 'test:1')

    def test_original_choices_do_not_match_or_use_current_artwork(self):
        from sprite_expressions.choices import prepare
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / 'sprite.pna'
            source.write_bytes(pna_fixture())
            sprite = read_pna(source)
            export_sprite(sprite, bases(sprite)[0], root / 'export', Image.new('RGBA', (100, 300)),
                          dict(transform=[[1, 0, 0], [0, 1, 0]]))
            # 原始表情近景的准备与目标图、SIFT、目标立绘合成完全无关。
            with patch('sprite_expressions.compose.match', side_effect=AssertionError('must not match')), \
                 patch('sprite_expressions.compose.composite', side_effect=AssertionError('must not replace')):
                prepare(root / 'export')
            self.assertTrue((root / 'export/choices/000.png').is_file())

    def test_library_extraction_automatically_saves_without_output_argument(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            library = root / 'characters/library.json'
            library.parent.mkdir()
            artwork = 'data:image/png;base64,' + base64.b64encode(png(Image.new('RGBA', (100, 300), 'white'))).decode()
            library.write_text(json.dumps(dict(characters=[dict(id='test:1', artworks=[dict(url=artwork)])])), encoding='utf-8')
            source = root / 'source'
            source.mkdir()
            (source / 'sprite.pna').write_bytes(pna_fixture())
            evidence = dict(accepted=True, transform=[[1, 0, 0], [0, 1, 0]], inliers=12, inlier_ratio=1)
            with patch('sprite_expressions.pipeline.match', return_value=evidence):
                result = main(['--library', str(library), '--character', 'test:1', '--source', str(source)])
            self.assertEqual(result, 0)
            self.assertEqual(len(manifests(catalog_path(library, 'test:1'), 'test:1')), 1)

    def test_composition_replaces_old_face_and_preserves_clothing_and_alpha(self):
        reference = Image.new('RGBA', (120, 300), (40, 50, 60, 255))
        reference.putpixel((50, 40), (255, 0, 0, 255))
        reference.putpixel((0, 299), (7, 8, 9, 0))
        body = Image.new('RGBA', (120, 300), (200, 180, 160, 255))
        layer = Image.new('RGBA', (30, 20), (0, 0, 0, 0))
        layer.putpixel((10, 10), (0, 0, 255, 255))
        evidence = dict(accepted=True, transform=[[1, 0, 0], [0, 1, 0]])
        with patch('sprite_expressions.compose.match', return_value=evidence):
            images, mask, _ = composite(reference, reference, body, {1: (layer, (40, 30))}, [[1]])
        result = np.asarray(images[0])
        original = np.asarray(reference)
        self.assertEqual(images[0].getpixel((50, 40)), (0, 0, 255, 255))
        np.testing.assert_array_equal(result[mask == 0], original[mask == 0])
        np.testing.assert_array_equal(result[:, :, 3], original[:, :, 3])

    def test_alpha_preserves_alpha_and_uses_garbro_integer_rounding(self):
        image = Image.fromarray(np.array([[[50, 20, 10, 128], [7, 8, 9, 0],
                                          [12, 34, 56, 255]]], dtype=np.uint8))
        self.assertEqual(list(straight_alpha(image).get_flattened_data()),
                         [(99, 39, 19, 128), (7, 8, 9, 0), (12, 34, 56, 255)])

    def test_exports_raw_bytes_offsets_and_exact_composition(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            path = root / 'sprite.pna'
            path.write_bytes(pna_fixture())
            sprite = read_pna(path)
            body = bases(sprite)[0]
            result = export_sprite(sprite, body, root / 'export', Image.new('RGBA', (100, 300)),
                                   {'transform': [[1, 0, 0], [0, 1, 0]]})
            entry = result['expressions'][0]
            self.assertEqual(entry['offset'], [25, 35])
            self.assertEqual(entry['roundtrip_mismatched_pixels'], 0)
            self.assertEqual((root / 'export/original/001.png').read_bytes(), sprite.layers[0].raw)
            with Image.open(root / 'export/previews/001.png') as preview:
                self.assertEqual(preview.getpixel((0, 0)), (0, 0, 0, 0))
                self.assertEqual(preview.getpixel((15, 15)), (100, 150, 200, 255))
                self.assertEqual(preview.getpixel((25, 35)), (99, 94, 109, 255))

    def test_rejects_truncated_payload_wrong_dimensions_and_trailing_data(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / 'broken.pna'
            wrong_size = bytearray(pna_fixture())
            struct.pack_into('<i', wrong_size, 20 + 40 + 16, 1000)
            for data in [pna_fixture()[:10], pna_fixture()[:-1], pna_fixture() + b'junk', wrong_size]:
                with self.subTest(length=len(data)):
                    path.write_bytes(data)
                    with self.assertRaises(ValueError):
                        read_pna(path)

    def test_current_index_selection_overrides_archived_selection_and_old_library_works(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            arts = [{'id': str(i), 'url': 'data:image/png;base64,' + base64.b64encode(
                png(Image.new('RGBA', (10, 10), color))).decode()} for i, color in enumerate(['red', 'blue'])]
            character = dict(id='test:1', name='fixture', selected_art=0, artworks=arts)
            path = root / 'library.json'
            path.write_text(json.dumps(dict(characters=[character])), encoding='utf-8')
            image, _ = library_reference(path, 'test:1')
            self.assertEqual(image.getpixel((0, 0)), (255, 0, 0, 255))
            payload = json.dumps(character).encode()
            digest = hashlib.sha256(payload).hexdigest()
            folder = root / ('character-' + hashlib.sha256(b'test:1').hexdigest())
            folder.mkdir()
            (folder / f'profile-{digest}.json').write_bytes(payload)
            index = dict(format_version=2, characters=[dict(id='test:1', profile=digest, selected_art=1)])
            path.write_text(json.dumps(index), encoding='utf-8')
            image, metadata = library_reference(path, 'test:1')
            self.assertEqual(metadata['artwork_index'], 1)
            self.assertEqual(image.getpixel((0, 0)), (0, 0, 255, 255))
            (folder / f'profile-{digest}.json').write_bytes(payload + b' ')
            with self.assertRaisesRegex(ValueError, '摘要不一致'):
                library_reference(path, 'test:1')

    def test_matches_shared_head_after_scale_change_with_different_clothing(self):
        rng = np.random.default_rng(3)
        pixels = np.full((600, 240, 4), 255, dtype=np.uint8)
        pixels[:180, :, :3] = rng.integers(0, 256, (180, 240, 3), dtype=np.uint8)
        reference = Image.fromarray(pixels)
        candidate = pixels.copy()
        candidate[180:, :, :3] = [20, 80, 130]
        candidate = Image.fromarray(candidate).resize((360, 900))
        evidence = match(features(reference, 0.3), candidate)
        self.assertTrue(evidence['accepted'])
        self.assertAlmostEqual(evidence['transform'][0][0], 2 / 3, places=2)
        unrelated = Image.fromarray(rng.integers(0, 256, pixels.shape, dtype=np.uint8))
        self.assertFalse(match(features(reference, 0.3), unrelated)['accepted'])

    def test_failure_is_reported_and_overlapping_output_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / 'source'
            source.mkdir()
            (source / 'broken.pna').write_bytes(b'broken')
            image = Image.new('RGBA', (100, 300), 'white')
            with self.assertRaisesRegex(ValueError, '重叠'):
                run(image, {}, source, source / 'output')
            destination, report = run(image, {}, source, root / 'output')
            self.assertEqual(report['status'], 'partial_failure')
            self.assertEqual(len(report['failures']), 1)
            self.assertTrue((destination / 'report.json').exists())
            self.assertEqual((source / 'broken.pna').read_bytes(), b'broken')

    def test_cli_reference_accepts_sibling_output_and_returns_failure_exit_code(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            reference = root / 'reference.png'
            Image.new('RGBA', (100, 300), 'white').save(reference)
            source = root / 'source'
            source.mkdir()
            (source / 'broken.pna').write_bytes(b'broken')
            code = main(['--reference', str(reference), '--source', str(source),
                         '--output', str(root / 'results')])
            self.assertEqual(code, 2)
            reports = list((root / 'results').glob('expressions-*/report.json'))
            self.assertEqual(len(reports), 1)
            self.assertEqual(json.loads(reports[0].read_text(encoding='utf-8'))['status'], 'partial_failure')


class DiscoveryTests(unittest.TestCase):
    def fixture(self, root):
        from sprite_expressions.pipeline import write_json
        library = root / 'characters/library.json'
        library.parent.mkdir()
        artwork = 'data:image/png;base64,' + base64.b64encode(png(Image.new('RGBA', (100, 300), 'white'))).decode()
        library.write_text(json.dumps(dict(characters=[dict(id='test:1', artworks=[dict(url=artwork)])])), encoding='utf-8')
        catalog = catalog_path(library, 'test:1')
        catalog.parent.mkdir(parents=True)
        write_json(catalog.parent / 'discovery.json', dict(format_version=1, character_id='test:1', enabled=True, games={}))
        project = Path(__file__).resolve().parents[1]
        return dict(library=str(library), character_id='test:1', assets=str(root), artwork_indices=[0], games=[],
                    helper=str(project / 'tools/voice/agent_archive_tools.py'), garbro='unused'), catalog

    def test_resume_keeps_completed_members_new_directory_only_and_deduplicates(self):
        from sprite_expressions.scan import scan
        class Interrupted(BaseException): pass
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            config, catalog = self.fixture(root)
            game = root / 'game'; game.mkdir()
            for name in ['first.pna', 'second.pna']:
                (game / name).write_bytes(pna_fixture())
            original = {p.name:p.read_bytes() for p in game.iterdir()}
            config['games'] = [str(game)]
            def checkpoint(state):
                current = next(iter(state['games'].values()))
                if current.get('files') and current['files'][0].get('cursor') == 1:
                    raise Interrupted()
            evidence = dict(accepted=True, inliers=20, transform=[[1,0,0],[0,1,0]])
            with patch('sprite_expressions.scan.match', return_value=evidence) as matching:
                with self.assertRaises(Interrupted): scan(config, checkpoint)
                self.assertEqual(matching.call_count, 1)
                self.assertEqual(len(json.loads(catalog.read_text('utf-8'))['unique']), 1)
                scan(config)
                self.assertEqual(matching.call_count, 2)
                scan(config)
                self.assertEqual(matching.call_count, 2, 'completed game must not be extracted again')
                second = root / 'new-game'; second.mkdir()
                (second / 'copy.pna').write_bytes(pna_fixture())
                config['games'].append(str(second))
                scan(config)
                self.assertEqual(matching.call_count, 3, 'only the newly associated directory is scanned')
                self.assertEqual(len(json.loads(catalog.read_text('utf-8'))['unique']), 1)
            self.assertEqual({p.name:p.read_bytes() for p in game.iterdir()}, original)

    def test_failed_resource_is_retryable_and_success_is_not_repeated(self):
        from sprite_expressions.scan import scan
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); config, catalog = self.fixture(root)
            game = root / 'game'; game.mkdir()
            (game / 'first.pna').write_bytes(pna_fixture())
            (game / 'second.pna').write_bytes(pna_fixture())
            config['games'] = [str(game)]
            evidence = dict(accepted=True, inliers=20, transform=[[1,0,0],[0,1,0]])
            with patch('sprite_expressions.scan.match', side_effect=[evidence, ValueError('temporary failure')]) as matching:
                self.assertEqual(scan(config)['result'], 'failed')
                self.assertEqual(matching.call_count, 2)
            with patch('sprite_expressions.scan.match', return_value=evidence) as matching:
                self.assertEqual(scan(config)['result'], 'complete')
                self.assertEqual(matching.call_count, 1)
            self.assertEqual(len(json.loads(catalog.read_text('utf-8'))['unique']), 1)

    def test_perceptual_dedup_keeps_distinct_mouth_and_blush_and_prefers_resolution(self):
        from sprite_expressions.dedup import rebuild
        # 同一表情的重采样误差很小；嘴形与腮红变化必须保留为不同选择。
        def descriptor(index, value, quality):
            pixels = np.zeros((64,64,4), dtype=np.float32)
            pixels[10:25,10:55,:3] = value
            return dict(index=index, exact=str(value)+str(quality), pixels=pixels, quality=quality)
        small = [descriptor(i, v, 100) for i,v in enumerate([50,100,150])]
        large = [descriptor(i, v, 400) for i,v in enumerate([51,101,151])]
        data = dict(packs=[dict(digest='small'),dict(digest='large')])
        with patch('sprite_expressions.dedup.descriptors', side_effect=[small,large]):
            result = rebuild(Path('catalog.json').absolute(), data)
        self.assertEqual(len(result['unique']), 3)
        self.assertTrue(all(item['digest']=='large' and len(item['origins'])==2 for item in result['unique']))

    def test_source_overlap_and_unsafe_archive_member_never_extract(self):
        from sprite_expressions.scan import scan
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); config, catalog = self.fixture(root)
            config['games'] = [str(root)]
            with self.assertRaisesRegex(ValueError, 'overlaps'): scan(config)
            game = root / 'game'; game.mkdir(); (game / 'resource.arc').write_bytes(b'archive')
            config['games'] = [str(game)]
            with patch('sprite_expressions.scan.garbro', return_value=b'     12 [00000010] ../escape.pna') as garbro:
                self.assertEqual(scan(config)['result'], 'failed')
                self.assertEqual(garbro.call_count, 1, 'unsafe entry must be rejected before extraction')
            self.assertFalse(catalog.exists())


if __name__ == '__main__':
    unittest.main()

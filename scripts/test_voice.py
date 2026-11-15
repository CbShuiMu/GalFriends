"""临时夹具验证路径、语音关联、日语门槛和重复素材；不访问真实游戏。"""
import base64
import json
from pathlib import Path
import struct
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]/'crates/core/src/voice/python'))
from common import checked_path, separate_output
from extract import parse_records, read_manifest
from training import train
from dataset import select_samples
from speech import speak
from reference_audio import selected_reference
from gpt_sovits import verify_names
from gpt_sovits import prepare as gsv_prepare
from gpt_sovits_speech import speak as gsv_speak, reference_prompt, synthesize_segments


def record(code, payload, offset):
    return dict(code=code, offset=offset, bytes=base64.b64encode(bytes([code])+payload).decode())


class VoiceTests(unittest.TestCase):
    def test_skill_host_boundaries_and_byte_evidence(self):
        from skill_host import create_host, span
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            host = create_host({'Agent': object}, root / 'work', root / 'game', root / 'skill')
            self.assertEqual(host.path('work', 'raw/script.ws2'), root / 'work/raw/script.ws2')
            for name, path in [('work', '../game/asset'), ('save', 'save.dat'), ('skill', str(root/'private'))]:
                with self.assertRaises(ValueError):
                    host.path(name, path)
            data = '刹那こんにちは'.encode('cp932')
            self.assertEqual(span(data, dict(offset=0, length=4, encoding='cp932')), '刹那')
            for proof in [dict(offset=-1,length=4,encoding='cp932'), dict(offset=0,length=999,encoding='cp932'),
                          dict(offset=True,length=4,encoding='cp932'), dict(offset=0,length=4,encoding='pickle')]:
                with self.assertRaises(ValueError):
                    span(data, proof)

    @unittest.skipUnless(sys.platform == 'win32', 'Windows canonical path aliases')
    def test_canonical_windows_source_cannot_bypass_output_boundary(self):
        with tempfile.TemporaryDirectory() as folder:
            source = Path(folder)
            canonical = '\\\\?\\' + str(source)
            self.assertEqual(checked_path(canonical), source)
            with self.assertRaisesRegex(ValueError, 'output_overlap'):
                separate_output(str(source/'output'), canonical)

    def test_skill_result_checks_provenance_and_rejects_external_assets(self):
        import hashlib
        from skill_extract import validate_result
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            game = root / 'game'
            game.mkdir()
            task = root / 'task'
            task.mkdir()
            assets = task / 'raw'
            assets.mkdir()
            script = assets / 'scene.bin'
            script.write_bytes(b'0123456789')
            audio = assets / 'voice.wav'
            audio.write_bytes(b'RIFFfixture')
            row = dict(speaker='役', text='こんにちは', language='ja', audio=str(audio), dialogue_id='scene:1',
                       source=dict(script=str(script), sha256=hashlib.sha256(script.read_bytes()).hexdigest(),
                                   message_offset=1, speaker_offset=2, voice_offset=3))
            data = dict(schema=1, original_language='ja', game_dir=str(game), asset_root=str(assets), entries=[row])
            manifest = task / 'dialogue-voice.json'
            manifest.write_text(json.dumps(data), encoding='utf-8')
            (task / 'extraction-report.json').write_text(json.dumps(dict(engine='fixture', unresolved=['unmapped'])), encoding='utf-8')
            self.assertEqual(validate_result(task, game)['excluded'], 1)
            row['source']['sha256'] = 'wrong'
            manifest.write_text(json.dumps(data), encoding='utf-8')
            with self.assertRaisesRegex(ValueError, 'source hash'):
                validate_result(task, game)
            data['asset_root'] = str(game)
            manifest.write_text(json.dumps(data), encoding='utf-8')
            with self.assertRaisesRegex(ValueError, 'unsafe_path'):
                validate_result(task, game)


    def test_segmented_speech_switches_references_and_joins_pcm_in_order(self):
        import numpy as np
        from unittest.mock import Mock

        model = Mock()
        model.run.side_effect = [
            iter([(32000, np.array([1200, 700, 0], dtype=np.int16))]),
            iter([(32000, np.array([-500, -200], dtype=np.int16))]),
        ]
        rate, audio = synthesize_segments(model, [
            ('来てくれたんだ！', (Path('happy.wav'), 'やったね！')),
            ('……待ってたわけじゃないよ。', (Path('shy.wav'), 'は、恥ずかしいよ。')),
        ])
        self.assertEqual([
            (inputs['text'], inputs['ref_audio_path'], inputs['prompt_text'])
            for inputs, in (call.args for call in model.run.call_args_list)
        ], [
            ('来てくれたんだ！', 'happy.wav', 'やったね！'),
            ('……待ってたわけじゃないよ。', 'shy.wav', 'は、恥ずかしいよ。'),
        ])
        self.assertEqual((rate, audio.dtype, audio.tolist()), (32000, np.int16, [1200, 700, 0, -500, -200]))

    def test_segmented_speech_rejects_a_silent_segment_instead_of_skipping_it(self):
        import numpy as np
        from unittest.mock import Mock

        model = Mock()
        model.run.side_effect = [
            iter([(32000, np.array([1200], dtype=np.int16))]),
            iter([(32000, np.zeros(10, dtype=np.int16))]),
        ]
        with self.assertRaisesRegex(ValueError, 'companion.audio_invalid'):
            synthesize_segments(model, [
                ('ありがとう。', (Path('happy.wav'), '嬉しいです。')),
                ('でも、さびしいな。', (Path('sad.wav'), '寂しいです。')),
            ])

    def test_emotion_reference_uses_matching_text_and_rejects_changes_or_escape(self):
        from extract import digest
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root/'audio').mkdir()
            (root/'reference.txt').write_text('こんにちは。', encoding='utf-8')
            self.assertEqual(reference_prompt(root, {}), (root/'reference.wav', 'こんにちは。'))
            audio = root/'audio/happy.wav'
            audio.write_bytes(b'fixture')
            request = dict(reference=dict(path=str(audio), text='嬉しいですね。', sha256=digest(audio)))
            self.assertEqual(reference_prompt(root, request), (audio, '嬉しいですね。'))
            audio.write_bytes(b'changed')
            with self.assertRaisesRegex(ValueError, 'emotion.changed'):
                reference_prompt(root, request)
            request['reference']['path'] = str(root/'outside.wav')
            with self.assertRaisesRegex(ValueError, 'emotion.changed'):
                reference_prompt(root, request)

    def test_switching_models_keeps_the_original_reference_for_both_inference_adapters(self):
        from extract import digest
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root/'original-model'
            (source/'audio').mkdir(parents=True)
            audio = source/'audio/happy.wav'
            audio.write_bytes(b'original selected audio')
            request = dict(reference=dict(source_dir=str(source), path=str(audio),
                text='嬉しいですね。', sha256=digest(audio)))
            for engine in ('gpt_sovits_v5turbo', 'gpt_sovits_v2pro', 'index_tts25'):
                current = root/engine
                (current/'audio').mkdir(parents=True)
                (current/'audio/happy.wav').write_bytes(b'different clip with the same name')
                self.assertEqual(selected_reference(current, request), (audio, '嬉しいですね。'))
                if engine != 'index_tts25':
                    self.assertEqual(reference_prompt(current, request), (audio, '嬉しいですね。'))
            request['reference']['path'] = str(root/'gpt_sovits_v2pro/audio/happy.wav')
            with self.assertRaisesRegex(ValueError, 'emotion.changed'):
                selected_reference(root/'gpt_sovits_v2pro', request)

    def test_gsv_exports_the_matching_japanese_line_beside_every_clip(self):
        import numpy as np
        import soundfile as sf
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            for name in ('assets','game','models','source'):(root/name).mkdir()
            rows=[]
            for i,text in enumerate(['こんにちは、今日もよろしく。','お帰りなさい、待っていたよ。']):
                audio=root/'assets'/f'{i}.wav'
                sf.write(audio,.1*np.sin(2*np.pi*(220+i*50)*np.arange(16000*4)/16000),16000)
                rows.append(dict(speaker='fixture',text=text+'%K%P',language='ja',audio=str(audio),source={'message_offset':i}))
            manifest=root/'map.json'
            manifest.write_text(json.dumps(dict(schema=1,game_dir=str(root/'game'),asset_root=str(root/'assets'),original_language='ja',entries=rows)),encoding='utf-8')
            run=gsv_prepare(dict(source=str(root/'source'),models=str(root/'models')),
                dict(language='ja',epochs=1,character_id='fixture',speaker='fixture',manifest=str(manifest),output_dir=str(root/'output')))
            lines=(run/'dataset.list').read_text('utf-8').splitlines()
            self.assertEqual(len(lines),2)
            for line,row in zip(lines,rows):
                wav,speaker,language,text=line.split('|')
                self.assertEqual((speaker,language,text),('fixture','ja',row['text'].replace('%K%P','')))
                self.assertEqual(Path(wav).with_suffix('.txt').read_text('utf-8').strip(),text)
            self.assertFalse((run/'model.json').exists())
    def test_speech_rejects_chinese_before_loading_models(self):
        for language, text in [('zh','こんにちは'),('ja','你好'),('ja','こんにちは'*100)]:
            with self.assertRaisesRegex(ValueError,'language_mismatch'):
                speak({},dict(language=language,text=text))
            with self.assertRaisesRegex(ValueError,'language_mismatch'):
                gsv_speak({},dict(language=language,text=text))

    def test_official_feature_stage_must_not_silently_skip_or_duplicate_clips(self):
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory)/'features.tsv'
            for text in ['a.wav\t1\n', 'a.wav\t1\na.wav\t2\n', 'a.wav\t1\nc.wav\t2\n']:
                path.write_text(text,encoding='utf-8')
                with self.assertRaisesRegex(ValueError,'incomplete_features'):
                    verify_names(path,{'a.wav','b.wav'})
            path.write_text('a.wav\t1\nb.wav\t2\n',encoding='utf-8')
            verify_names(path,{'a.wav','b.wav'})

    def test_speech_rejects_another_character_before_loading_weights(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            (root/'model.json').write_text(json.dumps(dict(character_id='setsuna',language='ja',
                verification_only=False,adapter='adapter.pth',reference='reference.wav')),encoding='utf-8')
            with self.assertRaisesRegex(ValueError,'model_invalid'):
                speak({},dict(language='ja',text='こんにちは',model_dir=str(root),character_id='yuuka'))

    def test_output_cannot_overlap_game_or_models_in_either_direction(self):
        with tempfile.TemporaryDirectory() as directory:
            game=Path(directory)/'game'
            game.mkdir()
            for output in (game,game/'training',game.parent):
                with self.assertRaisesRegex(ValueError,'output_overlap'):
                    separate_output(str(output),game)
            self.assertEqual(separate_output(str(game.parent/'output'),game),game.parent/'output')

    def test_mapping_uses_voice_instruction_and_resets_after_message(self):
        name='刹那'.encode('cp932')
        records=[record(0x28,b'charSET\0SET1.ogg\0',0),record(0x15,b'%LF'+name+b'\0\0',30),
                 record(0x14,struct.pack('<I',17)+b'char\0'+ 'こんにちは'.encode('cp932')+b'\0\0',50),
                 record(0x14,struct.pack('<I',18)+b'char\0'+ '次の台詞'.encode('cp932')+b'\0\0',80)]
        rows=parse_records(records,{'set1':'audio/SET1.ogg'},Path('chapter.ws2'))
        self.assertEqual(len(rows),1)
        self.assertEqual((rows[0]['speaker'],rows[0]['dialogue_id'],rows[0]['source']['voice_offset']),('刹那','chapter.ws2:17',0))

    def test_manifest_rejects_external_audio_and_language_mismatch(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            assets=root/'raw'
            assets.mkdir()
            external=root/'external.ogg'
            external.write_bytes(b'voice')
            manifest=root/'map.json'
            data=dict(schema=1,asset_root=str(assets),entries=[dict(speaker='刹那',text='こんにちは',language='ja',audio=str(external))])
            manifest.write_text(json.dumps(data),encoding='utf-8')
            with self.assertRaisesRegex(ValueError,'unsafe_path'):read_manifest(manifest)
            with self.assertRaisesRegex(ValueError,'language_mismatch'):
                select_samples(dict(original_language='zh',entries=[]),'刹那','ja')
            with self.assertRaisesRegex(ValueError,'language_mismatch'):
                select_samples(dict(original_language='ja',entries=[]),'刹那','zh')

    def test_repeated_audio_with_conflicting_text_is_not_silently_deduplicated(self):
        with tempfile.TemporaryDirectory() as directory:
            audio=Path(directory)/'voice.ogg'
            audio.write_bytes(b'same source audio')
            rows=[dict(speaker='刹那',text=t,language='ja',audio=str(audio),source={}) for t in ('こんにちは','さようなら')]
            with self.assertRaisesRegex(ValueError,'conflicting_transcripts'):
                select_samples(dict(original_language='ja',entries=rows),'刹那','ja')

    def test_linked_audio_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            target=root/'target'
            target.mkdir()
            link=root/'link'
            try:link.symlink_to(target,target_is_directory=True)
            except OSError:self.skipTest('Symlink creation is unavailable')
            with self.assertRaisesRegex(ValueError,'unsafe_path'):checked_path(str(link/'audio.wav'))

    def test_training_never_creates_output_inside_imported_assets(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            for name in ('assets','game','models','source'):(root/name).mkdir()
            audio=root/'assets/voice.ogg'
            audio.write_bytes(b'fixture')
            manifest=root/'map.json'
            manifest.write_text(json.dumps(dict(schema=1,game_dir=str(root/'game'),asset_root=str(root/'assets'),
                original_language='ja',entries=[dict(speaker='刹那',text='こんにちは',language='ja',audio=str(audio))])),encoding='utf-8')
            output=root/'assets/training'
            with self.assertRaisesRegex(ValueError,'output_overlap'):
                train(dict(models=str(root/'models'),source=str(root/'source')),
                      dict(language='ja',epochs=1,character_id='fixture',manifest=str(manifest),output_dir=str(output)))
            self.assertFalse(output.exists())


if __name__=='__main__':unittest.main()

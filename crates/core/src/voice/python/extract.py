"""复用 GalExtract / GalGear 的 GARbro 路线，按 AdvHD 指令关联原文和语音。

只对完整走表成功的 WS2 产出映射。未知引擎和指令明确报告，不扫描随机字节猜台词。
"""
import base64
from collections import Counter
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import struct
from common import checked_path, new_run, progress, run_command, separate_output, write_json


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def archive(runtime, source, target):
    spec = importlib.util.spec_from_file_location('archive_tools', runtime['archive_tools'])
    helper = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(helper)
    command = runtime['garbro']
    listing = run_command([command, source])
    if listing.stderr.strip():
        raise ValueError(listing.stderr.decode('utf-8', errors='replace')[-1500:])
    names = helper.mod_members(listing.stdout)
    helper.safe_members(names)
    if not names:
        raise ValueError('voice.unsupported_archive')
    target.mkdir()
    result = run_command([command, '-x', source], cwd=target, timeout=900)
    if result.stderr.strip():
        raise ValueError(result.stderr.decode('utf-8', errors='replace')[-1500:])
    actual = sorted(p.relative_to(target).as_posix().casefold() for p in target.rglob('*') if p.is_file())
    if actual != sorted(n.replace('\\', '/').casefold() for n in names):
        raise ValueError('voice.incomplete_extraction')
    for path in target.rglob('*'):
        checked_path(str(path))
    return dict(archive=str(source), sha256=digest(source), files=len(names))


def string_at(data, start):
    end = data.index(b'\0', start)
    return data[start:end].decode('cp932', errors='strict'), end + 1


def parse_records(records, audio, source):
    speaker, speaker_offset = '', 0
    voice, voice_offset = None, 0
    entries = []
    for record in records:
        raw = base64.b64decode(record['bytes'], validate=True)
        code, offset = record['code'], record['offset']
        if code == 0x15:
            speaker, _ = string_at(raw, 1)
            speaker = speaker.removeprefix('%LF')
            speaker_offset = offset
        elif code == 0x28:
            channel, pos = string_at(raw, 1)
            name, _ = string_at(raw, pos)
            candidate = audio.get(Path(name).stem.casefold())
            # 只接受 Voice 归档中实际存在的语音；音效通道不能沿用旧语音。
            if channel.lower().startswith(('voice', 'vo')) or candidate:
                voice, voice_offset = candidate, offset
        elif code == 0x14:
            message_id = struct.unpack_from('<I', raw, 1)[0]
            _, pos = string_at(raw, 5)
            text, _ = string_at(raw, pos)
            if voice and speaker:
                # 保留原文供审计；控制标记只在训练副本清理。
                entries.append(dict(speaker=speaker, text=text, audio=voice,
                                    language='ja', dialogue_id=f'{source.name}:{message_id}',
                                    source=dict(script=str(source), message_offset=offset,
                                                speaker_offset=speaker_offset, voice_offset=voice_offset)))
            voice = None
    return entries


def extract(runtime, request):
    game = checked_path(request['game_dir'], directory=True)
    output = separate_output(request['output_dir'], game, runtime['models'], runtime['source'])
    # 此版本的自动关联适配器为 AdvHD；其他引擎可导入 GalExtract 映射。
    if not (game / 'AdvHD.exe').is_file() or not (game / 'Voice.arc').is_file():
        raise ValueError('voice.unsupported_engine')
    archives = [p for p in (game/'Voice.arc', game/'Rio.arc', game/'Rio1.arc', game/'Script.arc') if p.is_file()]
    root = new_run(output, 'extract')
    raw = root / 'raw'
    raw.mkdir()
    inventory = []
    for i, path in enumerate(archives):
        checked_path(str(path))
        progress('extracting', i, len(archives))
        inventory.append(archive(runtime, path, raw/path.stem))
    return map_resources(runtime, root, game, inventory)


def map_resources(runtime, root, game, inventory):
    """对本任务已验证的独立解包根重新生成映射，不再次触碰源游戏。"""
    raw = root/'raw'
    audio = {}
    for path in (raw/'Voice').rglob('*'):
        if path.is_file():
            key = path.stem.casefold()
            if key in audio:
                raise ValueError('voice.ambiguous_audio')
            audio[key] = str(path)
    entries, failures = [], []
    scripts = sorted(raw.rglob('*.ws2'))
    for i, script in enumerate(scripts):
        progress('mapping', i, len(scripts))
        try:
            result = run_command([runtime['php'], '-d', 'error_reporting=22527',
                                  '-d', 'extension_dir='+str(Path(runtime['php']).parent/'ext'), '-d', 'extension=mbstring',
                                  Path(__file__).with_name('ws2_records.php'), runtime['ws2'], script])
            records = json.loads(result.stdout)
            rows = parse_records(records, audio, script)
            sha = digest(script)
            for row in rows:
                row['source']['sha256'] = sha
            entries.extend(rows)
        except (ValueError, UnicodeError, OSError) as error:
            failures.append(dict(script=script.name, error=str(error)))
    write_json(root/'extraction-report.json', dict(inventory=inventory, scripts=len(scripts), failed=failures, linked=len(entries)))
    if not entries:
        raise ValueError('voice.no_mapping: ' + str(root/'extraction-report.json'))
    manifest = root/'dialogue-voice.json'
    write_json(manifest, dict(schema=1, engine='AdvHD', game_dir=str(game), asset_root=str(raw), entries=entries,
                             failed_scripts=failures, original_language='ja'))
    return inspect(manifest)


def read_manifest(path):
    path = checked_path(str(path))
    if path.stat().st_size > 64 * 1024 * 1024:
        raise ValueError('voice.manifest_too_large')
    data = json.loads(path.read_text('utf-8-sig'))
    if data.get('schema') != 1 or not isinstance(data.get('entries'), list):
        raise ValueError('voice.invalid_manifest')
    root = checked_path(data.get('asset_root', ''), directory=True)
    for entry in data['entries']:
        audio = checked_path(entry['audio'])
        if root not in audio.parents or not audio.is_file():
            raise ValueError('voice.unsafe_path')
        if not all(isinstance(entry.get(k), str) and entry[k].strip() for k in ('speaker','text','language')):
            raise ValueError('voice.invalid_manifest')
    return data


def inspect(path):
    data = read_manifest(path)
    speakers = {}
    for entry in data['entries']:
        speakers.setdefault(entry['speaker'], set()).add(entry['audio'])
    return dict(manifest=str(path), speakers=[dict(name=k, clips=len(v)) for k,v in sorted(speakers.items())],
                samples=len(data['entries']), excluded=len(data.get('failed_scripts', [])))

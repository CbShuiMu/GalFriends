"""GalGear 宿主的声音任务适配：复用工具和纯解析进程，不启动剧情/存档工作流。"""
import hashlib
import json
import os
from pathlib import Path
import runpy
import time

from common import checked_path, progress, write_json
from extract import digest, extract as extract_advhd, read_manifest


def create_host(api, root, game, skill):
    class VoiceHost(api['Agent']):
        def __init__(self):
            # 只装配下方白名单工具所需状态。原 Agent 构造器包含剧情、存档及游戏内写入约束。
            self.roots = dict(work=root, game=game, skill=skill)
            self.work, self.config = root, dict(cold_start=True)
            self.sources, self.documents, self.events = set(), {}, []
            self.read_skill, self.used_skill = False, False
            self.skill_run_cache, self.extracted_from, self.extraction_stamps = {}, {}, {}
            self.host_probes, self.reference_hints = [], []

        def path(self, name, relative):
            if name not in self.roots or not isinstance(relative, str) or Path(relative).is_absolute():
                raise ValueError('Use a relative path under game, skill or work')
            result = checked_path(str(self.roots[name] / relative))
            if not result.is_relative_to(self.roots[name]):
                raise ValueError('voice.unsafe_path')
            if name == 'game' and '.galgear-ai-summary' in (p.casefold() for p in result.parts):
                raise ValueError('Previous plot caches are outside this task')
            return result

    return VoiceHost()


def span(data, proof):
    if not isinstance(proof, dict):
        raise ValueError('Byte evidence is required for text, speaker and voice')
    offset, length, encoding = proof.get('offset'), proof.get('length'), proof.get('encoding')
    if (type(offset) is not int or type(length) is not int or offset < 0 or length < 1
            or offset + length > len(data) or length > 65536
            or encoding not in ('utf-8', 'utf-16-le', 'utf-16-be', 'cp932', 'ascii')):
        raise ValueError('Invalid byte span')
    return data[offset:offset+length].decode(encoding, errors='strict')


class Session:
    def __init__(self, api, root, game, skill, runtime):
        self.api, self.root, self.game, self.runtime = api, root, game, runtime
        self.host = create_host(api, root, game, skill)
        self.result = None
        self.inventory = []
        self.mapped = False
        self.tools = [t for t in api['TOOLS'] if t['function']['name'] in
                      ('list_files', 'read_text', 'read_bytes', 'run_skill')]
        string = {'type': 'string'}
        tool = api['tool']
        self.tools += [
            tool('inspect_data', 'Run a pure Python extract(data, metadata) function on a file (32 MiB maximum). '
                 'No file, network or process access. Imports: struct, re, json, hashlib, base64, zlib. '
                 'Use this to inspect instruction structure before writing a batch parser.',
                 dict(root=string, path=string, source=string), ['root', 'path', 'source']),
            tool('prepare_advhd', 'For confirmed AdvHD 1.9/WS2 only: extract Voice/Rio/Rio1/Script archives '
                 'using GalExtract GARbro and the installed WS2 instruction reader. Map all supported scripts '
                 'and preserve failures. Read skill/engine/willplus.md first. No training.', {}, []),
            tool('map_dialogue', 'Batch a pure Python extract(data, metadata) over ALL matching work scripts. '
                 'Return a list of {speaker,text,voice,dialogue_id,evidence:{text,speaker,voice}}. '
                 'Each evidence is {offset,length,encoding} pointing to exact original bytes in that script. '
                 'voice must equal an actual audio filename or relative path under audio_root. '
                 'Use instruction associations, never order or guessed names. metadata has filename only. '
                 'Failures remain unresolved; none are silently skipped. Output becomes a training manifest.',
                 dict(directory=string, pattern=string, audio_root=string, source=string, engine=string),
                 ['directory', 'pattern', 'audio_root', 'source', 'engine']),
            tool('finish_extraction', 'Validate the host-produced manifest and report. Return success only '
                 'after usable mapped audio and source provenance pass validation.', {}, []),
        ]

    def dispatch(self, name, args):
        if name in ('list_files', 'read_text', 'read_bytes', 'run_skill'):
            if name == 'run_skill':
                if args.get('command') not in ('i', 'l', 'x'):
                    raise ValueError('Read-only archive commands only')
                progress('skill_extracting')
            value = self.host._dispatch(name, args)
            if name == 'run_skill' and args['command'] == 'x' and not value.get('error'):
                self.inventory.append(dict(archive=args['archive'], filter=args.get('filter'), result=value))
            self.host.events.append(dict(tool=name))
            return value
        if name == 'inspect_data':
            path = self.host.path(args['root'], args['path'])
            if path.stat().st_size > 32 * 1024 * 1024:
                raise ValueError('Input exceeds 32 MiB; inspect a header or extract members first')
            return self.api['run_parser'](args['source'], path.read_bytes(), dict(filename=path.name))
        if name == 'prepare_advhd':
            if not self.host.read_skill or not self.host.documents.get('engine/willplus.md', {}).get('complete'):
                raise ValueError('Read complete SKILL.md and engine/willplus.md first')
            if self.mapped:
                raise ValueError('A map already exists; validate it with finish_extraction')
            result = extract_advhd(self.runtime, dict(game_dir=str(self.game), output_dir=str(self.root)))
            data = read_manifest(Path(result['manifest']))
            source_report = json.loads(Path(result['manifest']).with_name('extraction-report.json').read_text('utf-8'))
            write_json(self.root / 'dialogue-voice.json', data)
            write_json(self.root / 'extraction-report.json', dict(engine='AdvHD', unresolved=data.get('failed_scripts', []),
                scope='Voice/Rio/Rio1/Script; complete supported WS2 instruction streams',
                adapter='GalExtract GARbro + installed advhd_ws2_tools', verification='native instruction mapping',
                tools={name: dict(path=self.runtime[name], sha256=digest(Path(self.runtime[name])))
                       for name in ('garbro', 'php')}, ws2_source=self.runtime['ws2'],
                layers='Archives are read-only; raw members are separate per archive; source bytes remain unchanged',
                limitations='Unsupported scripts remain unresolved; this does not verify other engine dialects',
                **source_report))
            self.mapped = True
            return dict(result, manifest=str(self.root / 'dialogue-voice.json'))
        if name == 'map_dialogue':
            return self.map_dialogue(args)
        if name == 'finish_extraction':
            from skill_extract import validate_result
            if not self.mapped:
                raise ValueError('No host-produced map; extract and map actual resources first')
            progress('validating')
            self.result = validate_result(self.root, self.game)
            return self.result
        raise ValueError('Tool unavailable')

    def map_dialogue(self, args):
        directory = self.host.path('work', args['directory'])
        audio_root = self.host.path('work', args['audio_root'])
        # 模型编写的解析程序、提示词和审计文本都不能充当游戏原始字节证据。
        extracted = [Path(path) for path in self.host.extracted_from]
        if not all(any(path.is_relative_to(root) for root in extracted) for path in (directory, audio_root)):
            raise ValueError('Select script and audio directories produced by run_skill extraction')
        pattern = args['pattern']
        if not pattern or len(pattern) > 128 or any(c in pattern for c in ('/', '\\', ':')):
            raise ValueError('Use a filename glob without directory components')
        scripts = sorted(directory.rglob(pattern))
        if not scripts or len(scripts) > 5000 or not args['engine'].strip():
            raise ValueError('Select 1–5000 real scripts and specify the detected engine')
        audio = {}
        for path in audio_root.rglob('*'):
            if path.is_file() and path.suffix.casefold() in ('.wav', '.ogg', '.mp3', '.flac', '.opus'):
                checked_path(str(path))
                for key in {path.name.casefold(), path.relative_to(audio_root).as_posix().casefold()}:
                    audio.setdefault(key, set()).add(path)
        source = args['source']
        self.api['validate_program'](source)
        (self.root / 'dialogue-parser.py').write_text(source, encoding='utf-8')
        entries, failures = [], []
        for index, path in enumerate(scripts):
            progress('mapping', index, len(scripts))
            checked_path(str(path))
            try:
                if not path.is_file() or path.stat().st_size > 32 * 1024 * 1024:
                    raise ValueError('Input exceeds parser file limit')
                data = path.read_bytes()
                rows = self.api['run_parser'](source, data, dict(filename=path.name))
                if not isinstance(rows, list):
                    raise ValueError('Parser must return a list')
                verified = []
                for row in rows:
                    proof = row['evidence']
                    for field in ('speaker', 'text', 'voice'):
                        if not isinstance(row[field], str) or not row[field] or span(data, proof[field]) != row[field]:
                            raise ValueError('Source bytes do not match ' + field)
                    matches = audio.get(row['voice'].replace('\\', '/').casefold(), set())
                    if len(matches) != 1 or not isinstance(row.get('dialogue_id'), str) or not row['dialogue_id']:
                        raise ValueError('Voice is missing/ambiguous or dialogue_id is absent')
                    verified.append(dict(speaker=row['speaker'], text=row['text'], audio=str(next(iter(matches))),
                        language='ja', dialogue_id=path.relative_to(self.root).as_posix() + ':' + row['dialogue_id'],
                        source=dict(script=str(path), sha256=hashlib.sha256(data).hexdigest(),
                            message_offset=proof['text']['offset'], speaker_offset=proof['speaker']['offset'],
                            voice_offset=proof['voice']['offset']), evidence=proof))
                entries.extend(verified)
            except (ValueError, KeyError, TypeError, OSError, RuntimeError) as error:
                failures.append(dict(script=str(path), error=str(error)[:2000]))
        write_json(self.root / 'extraction-report.json', dict(engine=args['engine'], unresolved=failures,
            scope=dict(directory=args['directory'], pattern=pattern), inventory=self.inventory,
            scripts=len(scripts), linked=len(entries), verification='exact byte spans and existing audio',
            limitations='Byte evidence does not independently prove engine control-flow semantics'))
        if not entries:
            raise ValueError('No verified voice mapping; inspect extraction-report.json and repair the parser')
        write_json(self.root / 'dialogue-voice.json', dict(schema=1, engine=args['engine'], game_dir=str(self.game),
            asset_root=str(audio_root), original_language='ja', entries=entries, failed_scripts=failures))
        self.mapped = True
        return dict(samples=len(entries), scripts=len(scripts), unresolved=len(failures))


def run(root, game, skill, runtime, request, prompt):
    host_path = checked_path(request['host_dir'], directory=True) / 'agent_host.py'
    if not host_path.is_file():
        raise ValueError('voice.agent_missing')
    api = runpy.run_path(str(host_path))
    session = Session(api, root, game, skill, runtime)
    config = request['agent']
    # GalGear 按供应商决定代理；显式配置仅作用于当前独立 worker。
    if config.get('proxy'):
        os.environ['HTTPS_PROXY'] = config['proxy']
        os.environ['HTTP_PROXY'] = config['proxy']
    messages = [dict(role='system', content=prompt)]
    errors, idle = {}, 0
    deadline = time.monotonic() + 3600
    allowed = {t['function']['name'] for t in session.tools}
    with (root / 'agent-events.jsonl').open('w', encoding='utf-8') as log:
        for turn in range(80):
            remaining = deadline - time.monotonic()
            if remaining < 1:
                raise ValueError('voice.skill_failed: agent time budget exhausted')
            progress('skill_analyzing', turn, 80)
            reply = api['TRANSPORT']['complete'](config, messages, session.tools,
                max_tokens=16000, timeout=min(300, remaining))
            message = reply['message']
            messages.append(message)
            calls = message.get('tool_calls') or []
            idle = 0 if calls else idle + 1
            for call in calls:
                name, args = call['function']['name'], {}
                try:
                    if name not in allowed or session.result:
                        raise ValueError('Tool unavailable')
                    args = json.loads(call['function']['arguments'])
                    value = session.dispatch(name, args)
                except Exception as error:
                    value = dict(error=str(error)[:3000])
                record = dict(turn=turn, tool=name, arguments=args, result=value)
                # 凭据不进入模型上下文或审计；脱敏为最后一道防线。
                log.write(json.dumps(record, ensure_ascii=False).replace(config['token'], '[redacted]') + '\n')
                log.flush()
                bounded = api['CONTEXT']['bounded_result'](root, value, 24000)
                messages.append(dict(role='tool', tool_call_id=call['id'], content=json.dumps(bounded, ensure_ascii=False)))
                if isinstance(value, dict) and value.get('error'):
                    signature = json.dumps([name, args, value['error']], sort_keys=True)
                    errors[signature] = errors.get(signature, 0) + 1
                    if errors[signature] >= 3:
                        raise ValueError('voice.skill_failed: repeated tool failure; see ' + str(root / 'agent-events.jsonl'))
            if session.result:
                return session.result
            if idle >= 2:
                raise ValueError('voice.skill_failed: agent did not produce a verified manifest')
            if not calls:
                messages.append(dict(role='user', content='Use a tool to prepare/repair the mapping, then finish_extraction. A text report is not completion.'))
    raise ValueError('voice.skill_failed: agent request budget exhausted')

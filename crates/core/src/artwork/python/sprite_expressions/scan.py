"""关联游戏的可恢复离线扫描；每个归档/成员完成后提交断点，不修改游戏。"""
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

from .catalog import catalog_lock, catalog_path, register
from .dedup import migrate
from .library import library_reference
from .matching import features, match_features as match
from .pipeline import checked_path, export_sprite, write_json
from .pna import bases, face_layers, on_canvas, read_pna

ARCHIVES = {'.arc', '.xp3', '.pak', '.paz', '.pfs', '.dat', '.bin'}


class UnsupportedArchive(ValueError):
    """GARbro 明确不认识的容器，不作为可重试 I/O 故障。"""


def inventory(folder):
    result = []
    count = 0
    for directory, folders, files in os.walk(folder, followlinks=False, onerror=lambda e: (_ for _ in ()).throw(e)):
        for name in folders + files:
            checked_path(Path(directory) / name)
            count += 1
            if count > 100000:
                raise ValueError('Expression game file count exceeds 100000')
        for name in sorted(files):
            path = Path(directory) / name
            if path.suffix.lower() in ARCHIVES | {'.pna'}:
                stat = path.stat()
                result.append(dict(path=str(path), size=stat.st_size, modified=stat.st_mtime_ns))
        folders.sort()
    return result


def garbro(executable, source, output, member=None):
    command = [str(executable), str(source)]
    if member is not None:
        command.append(member)
    # GARbro 的异常只写 stderr，不保证非零退出码；两者必须同时检查。
    with tempfile.TemporaryFile() as stdout, tempfile.TemporaryFile() as stderr:
        result = subprocess.run(command, cwd=output, stdout=stdout, stderr=stderr, timeout=120,
                                creationflags=0x08000000 if os.name == 'nt' else 0)
        stdout.seek(0)
        stderr.seek(0)
        raw, error = stdout.read(16 * 1024 * 1024 + 1), stderr.read(8192)
    if b'unknown format' in (raw + error).lower():
        raise UnsupportedArchive('GARbro does not support this archive format')
    if result.returncode or error.strip() or len(raw) > 16 * 1024 * 1024:
        raise ValueError('GARbro: ' + error.decode('utf-8', 'replace')[:2000])
    return raw


def scan(config, hook=None):
    """hook 供断点故障注入测试；提交素材后才推进游标，重跑同一成员保持幂等。"""
    root = catalog_path(config['library'], config['character_id']).parent
    root.mkdir(parents=True, exist_ok=True)
    with catalog_lock(root / '.scan.lock'):
        # 上次进程被终止留下的未发布临时目录；验证全部后代再清理，不触及素材包。
        for temporary in list(root.glob('.scan-*')) + list(root.glob('.list-*')):
            temporary = checked_path(temporary)
            if temporary.parent != root or not temporary.is_dir():
                raise ValueError('Invalid expression temporary directory')
            for child in temporary.rglob('*'):
                checked_path(child)
            shutil.rmtree(temporary)
        return _scan(config, hook)


def _scan(config, hook):
    library = checked_path(config['library'])
    catalog = catalog_path(library, config['character_id'])
    root = catalog.parent
    root.mkdir(parents=True, exist_ok=True)
    path = checked_path(root / 'discovery.json')
    state = json.loads(path.read_text('utf-8'))
    if state['character_id'] != config['character_id'] or state['format_version'] != 1:
        raise ValueError('Invalid expression discovery identity')
    spec = importlib.util.spec_from_file_location('expression_archive_adapter', checked_path(config['helper']))
    adapter = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(adapter)
    games = state.setdefault('games', {})
    for folder in config['games']:
        folder = checked_path(folder)
        if folder == root or folder.is_relative_to(root) or root.is_relative_to(folder):
            raise ValueError('Expression output overlaps game directory')
        key = os.path.normcase(str(folder)).casefold()
        games.setdefault(key, dict(folder=str(folder), status='pending'))

    def save(stage, current=0, total=0):
        state.update(stage=stage, current=current, total=total)
        write_json(path, state)
        write_json(root / 'scan-progress.json', dict(run=config.get('run', ''), stage=stage, current=current, total=total,
                                                    result=state.get('result', '') if stage == 'finished' else ''))
        if hook:
            hook(state)

    save('indexing')
    migrate(catalog, config['character_id'])
    references = None
    for game in games.values():
        if game['status'] in ('complete', 'unsupported', 'no_match'):
            continue
        game['status'] = 'running'
        folder = checked_path(game['folder'])
        if not folder.is_dir():
            game.update(status='unavailable', error='Local game directory is unavailable')
            save('indexing')
            continue
        if 'files' not in game:
            game.update(files=inventory(folder), cursor=0, matches=0, failures=[], members_seen=0)
            save('indexing')
        if references is None:
            references = []
            for index in config['artwork_indices']:
                image, _ = library_reference(library, config['character_id'], config['assets'], index)
                references.append((image, features(image, .3)))
        files = game['files']
        while game['cursor'] < len(files):
            entry = files[game['cursor']]
            source = checked_path(entry['path'])
            try:
                if not source.is_relative_to(folder):
                    raise ValueError('Game resource escaped associated directory')
                stat = source.stat()
                if (stat.st_size, stat.st_mtime_ns) != (entry['size'], entry['modified']):
                    raise ValueError('Game resource changed after checkpoint')
                if 'members' not in entry:
                    save('indexing', game['cursor'], len(files))
                    if source.suffix.lower() == '.pna':
                        entry['members'] = [None]
                    else:
                        with tempfile.TemporaryDirectory(prefix='.list-', dir=root) as work:
                            raw = garbro(config['garbro'], source, work)
                        names = adapter.mod_members(raw)
                        entry['members'] = [name for name in names if Path(name).suffix.lower() == '.pna']
                    entry['cursor'] = 0
                    game['members_seen'] += len(entry['members'])
                    save('extracting', 0, len(entry['members']))
                while entry['cursor'] < len(entry['members']):
                    member = entry['members'][entry['cursor']]
                    # 独立临时工作目录只接收已验证成员，失败不会发布半份表情。
                    with tempfile.TemporaryDirectory(prefix='.scan-', dir=root) as work:
                        work = Path(work)
                        if member is not None:
                            adapter.safe_members([member])
                            garbro(config['garbro'], source, work, member)
                            candidate = checked_path(work / member.replace('\\', '/'))
                            if not candidate.is_relative_to(work) or not candidate.is_file():
                                raise ValueError('GARbro did not export the selected member')
                        else:
                            candidate = source
                        sprite = read_pna(candidate)
                        accepted = []
                        for base in bases(sprite):
                            if not face_layers(sprite, base):
                                continue
                            body = on_canvas(sprite, base)
                            candidate_features = features(body, .35)
                            for image, reference in references:
                                evidence = match(reference, candidate_features)
                                if evidence['accepted']:
                                    accepted.append((evidence['inliers'], base, image, evidence))
                        if accepted:
                            _, base, reference_image, evidence = max(accepted, key=lambda item: item[0])
                            destination = work / 'export'
                            manifest = export_sprite(sprite, base, destination, reference_image, evidence)
                            # 临时提取路径不作为唯一来源：记录实际归档与成员。
                            manifest['source'] = member or str(source)
                            manifest['game_source'] = dict(folder=str(folder), archive=str(source), member=member,
                                                           size=entry['size'], modified=entry['modified'])
                            write_json(destination / 'manifest.json', manifest)
                            after = source.stat()
                            if (after.st_size, after.st_mtime_ns) != (entry['size'], entry['modified']):
                                raise ValueError('Game resource changed during extraction')
                            register(library, config['character_id'], [destination / 'manifest.json'], config['assets'])
                            game['matches'] += 1
                    entry['cursor'] += 1
                    save('extracting', entry['cursor'], len(entry['members']))
                game['cursor'] += 1
            except UnsupportedArchive:
                entry['unsupported'] = True
                game['cursor'] += 1
            except (OSError, ValueError, subprocess.TimeoutExpired) as error:
                # 失败项保留在清单，下一次打开时重试；其余成员和归档继续处理。
                game['failures'].append(dict(file=game['cursor'], error=str(error)))
                game['cursor'] += 1
            save('extracting', game['cursor'], len(files))
        if game['failures']:
            game['status'] = 'failed'
        else:
            game['status'] = 'complete' if game['matches'] else ('no_match' if game['members_seen'] else 'unsupported')
        save('indexing')
    failures = [game for game in games.values() if game['status'] in ('failed', 'unavailable')]
    # 只重试失败文件，已处理成员游标保留；退出恢复不会重新扫描成功的归档。
    for game in failures:
        if game.get('failures'):
            indices = sorted({failure['file'] for failure in game['failures']})
            game['files'] = [game['files'][i] for i in indices]
            game.update(cursor=0, failures=[])
    state['result'] = ('failed' if failures else 'empty' if not games else
                       'unsupported' if all(g['status'] == 'unsupported' for g in games.values()) else 'complete')
    save('finished')
    return state

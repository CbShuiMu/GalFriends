"""离线工具参数解析，不注册 Tauri 命令。"""
import argparse
import hashlib
from pathlib import Path
import sys

from .library import library_reference, rgba
from .pipeline import checked_path, run
from .catalog import catalog_path, manifests, register


def main(argv=None):
    parser = argparse.ArgumentParser(description='从 GARbro 导出的 PNA 匹配同姿势并提取原生表情层')
    reference = parser.add_mutually_exclusive_group()
    reference.add_argument('--library', type=Path, help='GalFriends characters/library.json')
    reference.add_argument('--reference', type=Path, help='直接指定本地参考立绘')
    parser.add_argument('--character', help='明确角色 ID，例如 bangumi:76896')
    parser.add_argument('--assets-root', type=Path, help='旧库相对资源路径的根目录，包含 assets/')
    parser.add_argument('--artwork-index', type=int, help='覆盖当前立绘下标，只影响本次离线任务')
    parser.add_argument('--source', type=Path, help='GARbro 导出的 PNA 目录')
    parser.add_argument('--compose', action='store_true', help='将表情替换到当前所选立绘')
    parser.add_argument('--manifest', type=Path, help='已提取表情的 manifest.json')
    parser.add_argument('--import-manifest', type=Path, action='append', help='将已有提取清单归档到指定角色，可重复')
    parser.add_argument('--catalog', type=Path, help='桌面端传入的角色表情目录索引')
    parser.add_argument('--pack', help='确认后使用的素材包摘要，与 --catalog 同时使用')
    parser.add_argument('--display', type=Path, help='直接参考图对应的超分显示图')
    parser.add_argument('--variant', action='append', help='按底到顶顺序指定图层号，例如 7,6，可重复')
    parser.add_argument('--output', type=Path, help='可选的审计输出目录；默认自动保存在角色目录')
    parser.add_argument('--base-index', type=int, help='指定身体记录号用于匹配与合成预览')
    parser.add_argument('--head-fraction', type=float, default=0.3, help='参考图前景顶部匹配比例，默认 0.3')
    args = parser.parse_args(argv)
    if not args.library and not args.reference:
        project = Path(__file__).resolve().parents[6]
        candidates = [project / 'characters/library.json', project / 'artifacts/characters/library.json']
        args.library = next((path for path in candidates if path.is_file()), candidates[0])
    if args.compose and not (args.manifest or args.library or args.catalog):
        parser.error('--reference 合成需要 --manifest 或 --catalog')
    if not args.compose and not args.import_manifest and not args.source:
        parser.error('提取需要 --source')
    if not 0.1 <= args.head_fraction <= 0.6:
        parser.error('--head-fraction 需要位于 0.1–0.6')
    if args.library and not args.character:
        parser.error('--library 需要 --character 指定角色身份')
    if args.reference and ((args.character and not args.catalog) or args.assets_root or args.artwork_index is not None):
        parser.error('--reference 不与角色库选择参数同时使用')
    if args.catalog and not args.character:
        parser.error('--catalog 需要 --character 核对身份')
    if args.pack and (not args.catalog or not args.compose or args.manifest):
        parser.error('--pack 需要 --compose 和 --catalog，不与 --manifest 混用')
    if args.import_manifest and (not args.library or args.compose or args.source):
        parser.error('--import-manifest 仅用于指定角色库的素材归档')
    if not args.library and not args.output:
        parser.error('--reference 需要 --output')
    try:
        if args.import_manifest:
            path = register(args.library, args.character, args.import_manifest, args.assets_root, args.artwork_index)
            print(f'表情已保存到角色目录 {path}')
            return 0
        if args.library and not args.output:
            args.output = catalog_path(args.library, args.character).parent / ('previews' if args.compose else 'runs')
        if args.library:
            checked_path(args.library)
            image, metadata = library_reference(args.library, args.character, args.assets_root, args.artwork_index)
        else:
            path = checked_path(args.reference)
            image = rgba(path)
            metadata = dict(image_source=str(path), sha256=hashlib.sha256(path.read_bytes()).hexdigest())
        if args.compose:
            from .compose import compose_run, select_manifest
            if args.library:
                display, _ = library_reference(args.library, args.character, args.assets_root, args.artwork_index, True)
            else:
                display = rgba(checked_path(args.display)) if args.display else image
            variants = [[int(value) for value in item.split(',')] for item in args.variant] if args.variant else None
            if args.pack:
                args.manifest = next((p for p in manifests(args.catalog, args.character) if p.parent.name == args.pack), None)
                if args.manifest is None:
                    raise ValueError('所选表情素材已变化')
            elif not args.manifest:
                catalog = args.catalog or catalog_path(args.library, args.character)
                args.manifest = select_manifest(image, catalog, args.character)
            root, report = compose_run(image, display, metadata, args.manifest, args.output, variants)
            print(f"{report['status']}，合成 {len(report['variants'])} 个表情，清单 {root / 'manifest.json'}")
            return 0
        root, report = run(image, metadata, args.source, args.output, args.base_index, args.head_fraction)
        if args.library and report['status'] == 'complete':
            path = register(args.library, args.character,
                            [root / entry['directory'] / 'manifest.json' for entry in report['exports']],
                            args.assets_root, args.artwork_index)
            print(f'表情已保存到角色目录 {path}')
        count = sum(entry['expressions'] for entry in report['exports'])
        print(f"{report['status']}，导出 {count} 个图层，清单 {root / 'report.json'}")
        return 0 if report['status'] == 'complete' else 2
    except (OSError, ValueError, KeyError) as error:
        print(f'处理失败 {error}', file=sys.stderr)
        return 2
    except KeyboardInterrupt:
        print('任务已中断，任务清单记录当前状态', file=sys.stderr)
        return 130

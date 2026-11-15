"""三个引擎共用的角色参考读取；素材来源独立于当前推理模型。"""
from common import checked_path
from extract import digest


def selected_reference(directory, request):
    """返回已经选择的音频和台词；未选择时交给各引擎使用原默认参考。"""
    reference = request.get('reference')
    if not reference:
        return None
    # 旧桌面进程没有 source_dir，兼容其当前模型目录内的参考请求。
    source = checked_path(reference.get('source_dir', str(directory)), directory=True)
    audio = checked_path(reference['path'])
    if audio.parent.resolve() != (source/'audio').resolve() or digest(audio) != reference['sha256']:
        raise ValueError('emotion.changed')
    text = reference['text']
    if not text.strip() or len(text) > 300:
        raise ValueError('emotion.invalid_data')
    return audio, text

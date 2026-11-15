"""预先准备官方日语文本流程需要的语言识别组件，避免首次合成再联网。"""
import hashlib
import json
import os
from pathlib import Path
import sys

source = Path(sys.argv[1]).resolve()
root = source/'GPT_SoVITS/pretrained_models/fast_langdetect'
root.mkdir(parents=True, exist_ok=True)
sys.path[:0] = [str(source), str(source/'GPT_SoVITS')]
os.chdir(source)
from text.LangSegmenter import LangSegmenter

segments = LangSegmenter.getTexts('おはようございます。今日も一緒に頑張りましょう。', 'ja')
assert segments and all(row['lang'] == 'ja' for row in segments)
path = root/'lid.176.bin'
with path.open('rb') as stream:
    md5 = hashlib.file_digest(stream, 'md5').hexdigest()
if md5 != '01810bc59c6a3d2b79c79e6336612f65':
    raise ValueError('fastText model checksum mismatch')
with path.open('rb') as stream:
    sha = hashlib.file_digest(stream, 'sha256').hexdigest()
(root/'verified-model.json').write_text(json.dumps(dict(
    source='https://dl.fbaipublicfiles.com/fasttext/supervised-models/lid.176.bin',
    md5=md5, sha256=sha, size=path.stat().st_size), indent=2), encoding='utf-8')
print('Japanese segmentation verified')

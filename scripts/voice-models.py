"""准备固定版本的官方 IndexTTS 2.5 权重，校验所有 LFS 哈希。"""
import hashlib
import json
from pathlib import Path
import sys
from huggingface_hub import HfApi, hf_hub_download

REPOSITORY = 'IndexTeam/IndexTTS-2.5'
REVISION = 'c39ce5ba981572cb187443877ff559dfb246ce63'
AUXILIARY = [
    ('facebook/w2v-bert-2.0', 'da985ba0987f70aaeb84a80f2851cfac8c697a7b', 'w2v-bert-2.0',
     {'config.json', 'preprocessor_config.json', 'model.safetensors', 'README.md'}),
    ('funasr/campplus', 'e4b6ede7ce16997aff4ae69fbca1f0175e2afede', '',
     {'campplus_cn_common.bin', 'README.md'}),
    ('nvidia/bigvgan_v2_22khz_80band_256x', '633ff708ed5b74903e86ff1298cf4a98e921c513', 'bigvgan',
     {'config.json', 'bigvgan_generator.pt', 'LICENSE', 'README.md'}),
]


def verify(root, repository, revision, names=None, *, record_name='verified-models.json'):
    root.mkdir(parents=True, exist_ok=True)
    info = HfApi().model_info(repository, revision=revision, files_metadata=True, timeout=30)
    records = []
    for item in info.siblings:
        if item.rfilename.startswith('qwen') or (names is not None and item.rfilename not in names):
            continue  # 训练不使用文本情绪模型。
        path = root/item.rfilename
        if not path.is_file():
            path = Path(hf_hub_download(repository,item.rfilename,revision=revision,local_dir=root))
        if item.size is not None and path.stat().st_size != item.size:
            raise ValueError('Model size mismatch: '+item.rfilename)
        with path.open('rb') as stream:
            sha = hashlib.file_digest(stream,'sha256').hexdigest()
        if item.lfs and sha != item.lfs.sha256:
            raise ValueError('Model hash mismatch: '+item.rfilename)
        records.append(dict(path=item.rfilename,sha256=sha,size=path.stat().st_size))
    if names is not None and names != {r['path'] for r in records}:
        raise ValueError('Missing required auxiliary files: '+repository)
    (root/record_name).write_text(json.dumps(dict(repository=repository,revision=revision,files=records),indent=2),encoding='utf-8')
    print('Verified',repository,'files:',len(records),flush=True)


def main():
    root = Path(sys.argv[1])
    verify(root, REPOSITORY, REVISION)
    for repository, revision, subfolder, names in AUXILIARY:
        verify(root/'hf_cache'/subfolder, repository, revision, names)


if __name__ == '__main__':
    main()

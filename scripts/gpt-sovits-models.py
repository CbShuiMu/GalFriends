"""下载固定版本的官方 GPT-SoVITS 权重，复用 SHA-256 校验。"""
import argparse
import importlib
from pathlib import Path
import time
from concurrent.futures import ThreadPoolExecutor
import requests
from huggingface_hub import HfApi

REPOSITORY = 'lj1995/GPT-SoVITS'
REVISION = '8f8857563c040d542de108a2c18b0425abd6c9c5'
COMMON_FILES = {
    's1v3.ckpt',
    'chinese-hubert-base/config.json', 'chinese-hubert-base/preprocessor_config.json',
    'chinese-hubert-base/pytorch_model.bin', 'chinese-roberta-wwm-ext-large/config.json',
    'chinese-roberta-wwm-ext-large/tokenizer.json', 'chinese-roberta-wwm-ext-large/pytorch_model.bin',
}
VARIANTS = {
    'v5turbo': ({'gsv-v5-pretrained/s2Gv5turbo.pth', 'gsv-v5-pretrained/vocoder.pth'}, 'verified-models.json'),
    'v2Pro': ({'v2Pro/s2Gv2Pro.pth', 'v2Pro/s2Dv2Pro.pth',
               'sv/pretrained_eres2netv2w24s4ep4.ckpt'}, 'verified-models-v2pro.json'),
}


def download(root, item):
    """分段下载并保留断点；代理对大型单连接响应可能长时间不返回。"""
    path = root/item.rfilename
    if path.is_file():
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    partial = path.with_suffix(path.suffix+'.partial')
    url = f'https://huggingface.co/{REPOSITORY}/resolve/{REVISION}/{item.rfilename}'
    offset = partial.stat().st_size if partial.exists() else 0
    while offset < item.size:
        end = min(offset+8*1024*1024, item.size)-1
        for attempt in range(5):
            try:
                response = requests.get(url, headers={'Range':f'bytes={offset}-{end}'}, timeout=(20,60))
                response.raise_for_status()
                if response.status_code == 200 and offset == 0 and len(response.content) == item.size:
                    chunk = response.content
                elif (response.status_code == 206 and
                      response.headers.get('Content-Range') == f'bytes {offset}-{end}/{item.size}' and
                      len(response.content) == end-offset+1):
                    chunk = response.content
                else:
                    raise ValueError('Unexpected download range: '+item.rfilename)
                break
            except (requests.RequestException, ValueError):
                if attempt == 4:
                    raise
                time.sleep(attempt+1)
        with partial.open('ab') as stream:
            stream.write(chunk)
        offset += len(chunk)
    # verify() below checks sizes and the official LFS SHA-256 before use.
    partial.replace(path)
    print('Downloaded',item.rfilename,flush=True)

if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('root', type=Path)
    parser.add_argument('--version', choices=VARIANTS, default='v5turbo')
    args = parser.parse_args()
    root = args.root
    variant_files, record = VARIANTS[args.version]
    files = COMMON_FILES | variant_files
    info = HfApi().model_info(REPOSITORY, revision=REVISION, files_metadata=True, timeout=30)
    with ThreadPoolExecutor(max_workers=3) as pool:
        list(pool.map(lambda item: download(root,item), [f for f in info.siblings if f.rfilename in files]))
    importlib.import_module('voice-models').verify(root, REPOSITORY, REVISION, files, record_name=record)

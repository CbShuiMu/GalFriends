"""两种语音引擎共用的日语台词选择、去重和来源记录规则。"""
from pathlib import Path
import re
from extract import digest


def select_samples(data, speaker, language):
    if language != 'ja' or data.get('original_language') != 'ja':
        raise ValueError('voice.language_mismatch')
    unique, excluded = {}, []
    for row in data['entries']:
        if row['speaker'] != speaker:
            continue
        if row['language'] != language:
            raise ValueError('voice.language_mismatch')
        # 对照固定 WS2 读取器 TextExtractor 的控制串处理，原文保留在映射。
        text = row['text'].replace('%K%P', '').replace('%LF', '').replace('\\n', ' ').replace('\n', ' ').strip()
        # 不把喘息/控制代码或无正文条目当正常对白。原始映射仍完整保留。
        if not re.search(r'[ぁ-んァ-ヶ一-龯]', text) or any(ord(c) < 32 for c in text):
            excluded.append(dict(audio=row['audio'], reason='no_clean_transcript'))
            continue
        sha = digest(Path(row['audio']))
        if sha in unique:
            if unique[sha]['text'] != text:
                raise ValueError('voice.conflicting_transcripts')
            unique[sha]['references'].append(row['source'])
        else:
            unique[sha] = dict(row, text=text, sha256=sha, references=[row['source']])
    if len(unique) < 2:
        raise ValueError('voice.insufficient_samples')
    return list(unique.values()), excluded


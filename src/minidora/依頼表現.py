"""依頼の処理指定と表示指定を分ける。引用内容・資料本文は書き換えない。

全文が既知の構文に一致した場合だけ返す。未対応の条件を削って通さない。
一般語義解消器でも、任意の依頼を同義と認定する機構でもない。
"""
from __future__ import annotations

import re
from .命題句 import 引用を切り出す, 最上位位置

依頼表現版 = 'MINIDORA-依頼表現-v0.2'
_依頼末尾 = r'して(?:ください|くれる|くれますか|もらえる|もらえますか|いただけますか)?'


def _本文(text: str) -> str:
    if type(text) is not str or not text.strip() or len(text) > 8192:
        raise ValueError('依頼原文の型・上限')
    value = text.strip()
    if value[-1:] in ('。', '?', '？'):
        value = value[:-1]
    return value


def _表示(value: str):
    found = re.fullmatch(r'(もう少し)?(短く|詳しく)説明' + _依頼末尾, value)
    if found is None:
        return None
    return {'詳細': found[2] == '詳しく', '相対指定': bool(found[1])}


def 再説明依頼を読む(text: str):
    display = _表示(_本文(text))
    if display is None:
        return None
    return {'行為': '再表現', '原文': text, **display,
            '解釈根拠': {'版': 依頼表現版, '構文': '説明の表示指定', '全文一致': True}}


def _末尾を読む(value: str, verbs: str):
    """処理と表示を別の意味欄へ渡す。未知の第二要求は無視しない。"""
    segments = list(最上位位置(value, ('、', ',')))
    display = None
    if segments:
        if len(segments) != 1:
            return None
        start, marker = segments[0]
        display = _表示(value[start + len(marker):].strip())
        if display is None or display['相対指定']:
            # 元成果のない新規検討では相対的な短縮を約束しない。
            return None
        value = value[:start].rstrip()
    if re.fullmatch(verbs + _依頼末尾, value) is None:
        return None
    return {} if display is None else {'詳細': display['詳細']}


def 検討依頼を読む(text: str):
    value = _本文(text)
    if not value.startswith(('資料「', '資料『')):
        return None
    name, end = 引用を切り出す(value, 2)
    suffix = value[end:]
    kind, changes, display = None, {}, None
    # 読解は全文真偽判定とは別の目的。未解釈を無視した判定へ自動転換しない。
    read_display = _末尾を読む(suffix, r'を(?:要約|読解)')
    if read_display is not None:
        return {'行為': '検討', '原文': text, '種類': '読解', '資料': name,
                '変更': {}, '詳細': False, **read_display,
                '解釈根拠': {'版': 依頼表現版, '構文': '範囲明示の資料読解', '全文一致': True}}
    for connector in ('に基づいて', 'に基づき', 'をもとに', 'から'):
        if not suffix.startswith(connector):
            continue
        rest = suffix[len(connector):]
        if rest.startswith(('「', '『')):
            question, qend = 引用を切り出す(rest, 0)
            display = _末尾を読む(rest[qend:], r'を(?:判定|判断)')
            reading = _末尾を読む(rest[qend:], r'の根拠を説明')
            if reading is not None:
                return {'行為': '検討', '原文': text, '種類': '読解', '資料': name,
                        '変更': {'問い': question}, **reading,
                        '解釈根拠': {'版': 依頼表現版, '構文': '問いの根拠読解', '全文一致': True}}
        else:
            found = re.fullmatch(r'(.+?)と言え(?:る|ますか)', rest)
            if not found:
                return None
            question, display = found[1], {}
        if display is None:
            return None
        kind, changes = '命題', {'問い': question}
        break
    if kind is None:
        for word, domain, verbs in (('で命題', '命題', r'を(?:判定|判断)'),
                                    ('で仮説', '仮説', r'を検討'),
                                    ('で介入', '介入', r'を比較')):
            if suffix.startswith(word):
                display = _末尾を読む(suffix[len(word):], verbs)
                if display is None:
                    return None
                kind = domain
                break
    if kind is None and suffix.startswith('で観測'):
        observation, end2 = 引用を切り出す(suffix, len('で観測'))
        display = _末尾を読む(suffix[end2:], r'を説明する仮説を検討')
        if display is None:
            return None
        # 循環importを避け、登録済みの厳格な命題列分解を再利用する。
        from .監査改善会話解釈 import 命題列
        kind, changes = '仮説', {'観測': 命題列(observation)}
    if kind is None and suffix.startswith(('で「', 'で『')):
        intervention, end2 = 引用を切り出す(suffix, 1)
        display = _末尾を読む(suffix[end2:], r'に介入した結果を比較')
        if display is None:
            return None
        from .監査改善会話解釈 import 真偽割当
        kind, changes = '介入', {'介入': 真偽割当(intervention)}
    if kind is None:
        return None
    return {'行為': '検討', '原文': text, '種類': kind, '資料': name,
            '変更': changes, **display,
            '解釈根拠': {'版': 依頼表現版, '構文': '資料・処理・表示の分離', '全文一致': True}}

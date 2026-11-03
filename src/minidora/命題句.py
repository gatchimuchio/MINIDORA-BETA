"""命題・引用の構成境界。原文位置を保持し、引用内部を外側の接続にしない。"""
from __future__ import annotations

_閉じ = {'(': ')', '（': '）', '「': '」', '『': '』'}


def 最上位位置(本文: str, 区切り: tuple[str, ...]):
    if type(本文) is not str or len(本文) > 32000:
        raise ValueError('構成句の入力型・上限')
    if (type(区切り) is not tuple or not 1 <= len(区切り) <= 256
            or any(type(語) is not str or not 1 <= len(語) <= 256 for 語 in 区切り)):
        raise ValueError('構成句の区切りは空でない文字列のtuple')
    積み = []; i = 0
    while i < len(本文):
        c = 本文[i]
        if c in _閉じ:
            積み.append(_閉じ[c])
            if len(積み) > 32: raise ValueError('引用・括弧の深さ上限')
        elif c in _閉じ.values():
            if not 積み or 積み.pop() != c: raise ValueError('引用・括弧の対応不正')
        elif not 積み:
            for 語 in 区切り:
                if 本文.startswith(語, i):
                    yield i, 語
                    i += len(語) - 1
                    break
        i += 1
    if 積み: raise ValueError('引用・括弧が閉じていない')


def 構成句を分ける(本文: str, 区切り=('。', '\n', '；', ';')):
    開始 = 0
    for 位置, 語 in 最上位位置(本文, 区切り):
        if 本文[開始:位置].strip(): yield 開始, 位置
        開始 = 位置 + len(語)
    if 本文[開始:].strip(): yield 開始, len(本文)


def 引用を切り出す(本文: str, 開始: int):
    if type(本文) is not str or not 0 < len(本文) <= 32000:
        raise ValueError('引用の入力型・上限')
    if type(開始) is not int or not 0 <= 開始 < len(本文) or 本文[開始] not in ('「', '『'):
        raise ValueError('引用の開始不正')
    積み = []
    for i in range(開始, len(本文)):
        c = 本文[i]
        if c in _閉じ: 積み.append(_閉じ[c])
        elif c in _閉じ.values():
            if not 積み or 積み.pop() != c: raise ValueError('引用の対応不正')
            if not 積み: return 本文[開始 + 1:i], i + 1
        if len(積み) > 32: raise ValueError('引用の深さ上限')
    raise ValueError('引用が閉じていない')

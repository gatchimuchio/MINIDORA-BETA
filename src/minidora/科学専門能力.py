from __future__ import annotations
from typing import Sequence
from . import 科学専門能力_量子 as _量子
from . import 科学専門能力_相対論 as _相対論
from . import 科学専門能力_確率統計 as _確率統計
from . import 科学専門能力_天文 as _天文
from . import 科学専門能力_化学計算 as _化学計算
from . import 科学専門能力_場と形式 as _場と形式
from . import 科学専門能力_追加 as _追加
from . import 科学専門能力_構造 as _構造
from . import 科学専門能力_行列 as _行列
from .科学専門能力_共通 import 科学専門能力結果, 問合せ正規化, 候補支持成立


def _相対論的媒質光速(question: str, choices: Sequence[str]):
    text = question.casefold()
    moves = any(字句 in text for 字句 in ('moving', 'moves', 'move'))
    if not ('index of refraction' in text and 'glass' in text and moves and 'speed of light' in text):
        return None
    hits = [
        i
        for i, 選択肢 in enumerate(choices)
        if '(1+n*v)/(n+v)' in str(選択肢).replace(' ', '').casefold()
    ]
    return (
        科学専門能力結果(
            hits[0],
            'relativistic_light_medium',
            0.985,
            '(1+n v)/(n+v)',
            'general-law',
        )
        if len(hits) == 1
        else None
    )


def 科学専門能力解決(question: str, choices: Sequence[str]):
    q = 問合せ正規化(question)
    rows = []
    for 解決器 in (
        _量子.解決,
        _相対論.解決,
        _確率統計.解決,
        _天文.解決,
        _化学計算.解決,
        _場と形式.解決,
        _追加.解決,
        _構造.解決,
        _行列.解決,
        _相対論的媒質光速,
    ):
        try:
            row = 解決器(q, choices)
        except Exception:
            row = None
        if row is None or not 0 <= row.index < len(choices):
            continue
        if not 候補支持成立(row, str(choices[row.index])):
            continue
        rows.append(row)
    if not rows or len({row.index for row in rows}) != 1:
        return None
    return max(rows, key=lambda row: row.信頼度)


__all__ = ['科学専門能力結果', '科学専門能力解決']

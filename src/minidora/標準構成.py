from __future__ import annotations

from .HDS構文化器_v1 import 公開HDSコンパイラ
from .実行系 import ミニドラ

標準構成版 = 'MINIDORA-標準-実行系-v1'


def 標準ミニドラ() -> ミニドラ:
    '公開HDS 構文化器を含む現行正本実行系を一意に構成する。'
    return ミニドラ(HDSコンパイラ_=公開HDSコンパイラ())


__all__ = ["標準ミニドラ", "標準構成版"]

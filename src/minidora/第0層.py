from __future__ import annotations

"""旧Layer-0公開APIの互換窓口。

v0.4以降、旧 ``Layer0`` はLLM模型中核ではなく ``計算実行器`` の旧称である。
新規コードは ``模型.MINIDORA模型核`` と ``計算実行器.計算実行器`` を使う。
"""

from .模型 import (
    LLM成立意味区別,
    LLM成立規定リポジトリ,
    LLM成立規定参照コミット,
    LLM成立規定版,
)
from .計算実行器 import 実行文脈, 計算実行器


Layer0 = 計算実行器

# 既存利用者を壊さないための旧名alias。意味は新上流正本へ向ける。
LAYER0正本リポジトリ = LLM成立規定リポジトリ
LAYER0参照コミット = LLM成立規定参照コミット
LAYER0仕様版 = LLM成立規定版
LAYER0機能責任 = LLM成立意味区別


__all__ = [
    "Layer0",
    "実行文脈",
    "LAYER0正本リポジトリ",
    "LAYER0参照コミット",
    "LAYER0仕様版",
    "LAYER0機能責任",
]

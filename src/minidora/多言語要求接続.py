"""英語の文書依頼を日本語へ変換し、明示的な呼出で既存HDSセッションへ渡す。

翻訳対象の資料をこの入口へ自動接続しない。返答言語の自動変換は別責任。
"""
from __future__ import annotations

from collections.abc import Callable, Mapping
from copy import deepcopy
from dataclasses import dataclass
from typing import TYPE_CHECKING

from .多言語変換 import 対訳を変換
from .製品版.型 import 能力結果

if TYPE_CHECKING:
    from .文脈要求 import 文脈付き要求セッション, 文脈付き応答


@dataclass(frozen=True, slots=True)
class 多言語要求結果:
    要求翻訳: 能力結果
    応答: 文脈付き応答 | None
    理由: str = ""

    @property
    def 成立(self) -> bool:
        return self.要求翻訳.成立 and self.応答 is not None and self.応答.成立


def 外部言語要求を実行(セッション: 文脈付き要求セッション, 原文: str, *,
                       資料: Mapping[str, 能力結果] | None = None,
                       停止要求: Callable[[], bool] | None = None) -> 多言語要求結果:
    """英語入力の原文と日本語化結果を保持する。翻訳保留時はHDSも会話更新も呼ばない。"""
    translated = 対訳を変換(原文, "en", "ja", 種別="文書依頼", 停止要求=停止要求)
    if not translated.成立:
        return 多言語要求結果(translated, None, translated.保留理由)
    from .文脈要求 import 文脈付き要求セッション
    if not isinstance(セッション, 文脈付き要求セッション):
        return 多言語要求結果(translated, None, "既存の文脈付き要求セッションが必要")
    結果 = セッション.応答(translated.本文, deepcopy(資料), 停止要求=停止要求)
    return 多言語要求結果(translated, 結果)

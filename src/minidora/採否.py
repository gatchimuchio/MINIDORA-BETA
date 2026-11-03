from __future__ import annotations

from dataclasses import dataclass
from enum import StrEnum


class 実行状態(StrEnum):
    合格 = "合格"
    保留 = "保留"
    失敗 = "失敗"
    非適用 = "非適用"


@dataclass(frozen=True, slots=True)
class 採否結果:
    状態: 実行状態
    理由: tuple[str, ...]


def 採否(*, 根拠数: int, 矛盾数: int = 0, 危険: bool = False, 非適用: bool = False) -> 採否結果:
    if 非適用:
        return 採否結果(実行状態.非適用, ("宣言範囲外",))
    if 危険:
        return 採否結果(実行状態.失敗, ("境界違反",))
    if 矛盾数:
        return 採否結果(実行状態.保留, ("未解消矛盾",))
    if 根拠数 == 0:
        return 採否結果(実行状態.保留, ("根拠不足", "未解",))
    return 採否結果(実行状態.合格, ("根拠有", "矛盾無",))

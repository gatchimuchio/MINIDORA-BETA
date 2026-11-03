"""契約効果とは別に保持する実行内の期待効果。"""
from __future__ import annotations
from dataclasses import dataclass


@dataclass(frozen=True, slots=True)
class 期待効果:
    追加状態: frozenset[str] = frozenset()
    削除状態: frozenset[str] = frozenset()
    解消残差: frozenset[str] = frozenset()
    追加残差: frozenset[str] = frozenset()
    根拠経験数: int = 0

    def __post_init__(self):
        for name in ("追加状態", "削除状態", "解消残差", "追加残差"):
            values = getattr(self, name)
            if not isinstance(values, frozenset) or any(not isinstance(x, str) or not x for x in values):
                raise TypeError(name + "は空でない文字列のfrozenset")
        if self.追加状態 & self.削除状態 or self.解消残差 & self.追加残差:
            raise ValueError("期待効果に相反する効果がある")
        if type(self.根拠経験数) is not int or self.根拠経験数 < 0:
            raise ValueError("根拠経験数不正")

    @property
    def 空(self) -> bool:
        return not (self.追加状態 or self.削除状態 or self.解消残差 or self.追加残差)

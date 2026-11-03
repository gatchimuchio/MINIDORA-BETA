"""明示された作用効果から未来状態を構成し、途中状態の制約も検査する。

ここでの未来は条件付き予測であり、外界で観測された事実ではない。
"""
from __future__ import annotations
from dataclasses import dataclass
from .値 import 文字, 文字列組


@dataclass(frozen=True, slots=True)
class HDS未来制約:
    ID: str
    維持状態: frozenset[str] = frozenset()
    同時禁止: tuple[frozenset[str], ...] = ()
    版: str = "v1"

    def __post_init__(self):
        文字(self.ID); 文字(self.版)
        if not isinstance(self.維持状態, frozenset) or not isinstance(self.同時禁止, tuple):
            raise TypeError("未来制約の型不一致")
        for group in (self.維持状態, *self.同時禁止):
            if not isinstance(group, frozenset):
                raise TypeError("未来制約は文字集合")
            for x in group:
                文字(x)
        if any(not x for x in self.同時禁止):
            raise ValueError("空の同時禁止条件")

    def 違反(self, 状態):
        return not self.維持状態 <= 状態 or any(x <= 状態 for x in self.同時禁止)


@dataclass(frozen=True, slots=True)
class HDS未来状態:
    作用ID: str
    成立状態: frozenset[str]
    残差: frozenset[str]
    契約版: str
    制約違反: tuple[str, ...] = ()
    区分: str = "条件付き予測"

    def __post_init__(self):
        文字(self.作用ID); 文字(self.契約版)
        for group in (self.成立状態, self.残差):
            if not isinstance(group, frozenset):
                raise TypeError("未来状態は文字frozenset")
            for x in group:
                文字(x)
        文字列組(self.制約違反)
        if self.区分 != "条件付き予測":
            raise ValueError("未来予測を観測事実へ昇格できない")


def 未来列を構成(初期状態, 残差, 作用列, 仕様群, 制約群=()):
    specs = {s.作用ID: s for s in 仕様群}
    s, r, rows = 初期状態, 残差, []
    for k in 作用列:
        a = specs.get(k)
        if a is None:
            raise ValueError("未来効果が未定義: " + k)
        if not a.入力状態 <= s:
            raise ValueError("未来作用の前提が不成立: " + k)
        s, r = (s - a.削除状態) | a.追加状態, (r - a.解消残差) | a.追加残差
        rows.append(HDS未来状態(k, s, r, a.版, tuple(c.ID for c in 制約群 if c.違反(s))))
    return tuple(rows)

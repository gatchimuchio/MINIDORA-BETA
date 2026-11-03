"""全体状態の読取・更新受理を一つのコア境界へ集約する。"""
from __future__ import annotations
from dataclasses import dataclass
from .値 import 文字, 署名


@dataclass(frozen=True, slots=True)
class 状態ビュー:
    版: int
    状態署名: str
    目的: tuple[str, ...]
    要求状態: frozenset[str]
    成立状態: frozenset[str]
    残差: frozenset[str]
    要求認識: frozenset[str]
    再評価待ち: frozenset[str]

    def __post_init__(self):
        if type(self.版) is not int or self.版 < 0:
            raise ValueError("状態版不正")
        文字(self.状態署名, "状態署名")
        if not isinstance(self.目的, tuple):
            raise TypeError("目的はtuple")
        for 群名 in ("要求状態", "成立状態", "残差", "要求認識", "再評価待ち"):
            群 = getattr(self, 群名)
            if not isinstance(群, frozenset) or any(not isinstance(x, str) or not x.strip() for x in 群):
                raise TypeError(群名 + "は空でない文字列のfrozenset")

    @property
    def 未達状態(self) -> frozenset[str]:
        return frozenset(self.要求状態 - self.成立状態)

    @property
    def 意味署名(self) -> str:
        return 署名((self.版, self.目的, self.要求状態, self.成立状態,
                  self.残差, self.要求認識, self.再評価待ち))


def 状態を読む(状態) -> 状態ビュー:
    """実装型を外へ漏らさず、全体状態の判断に必要な共通面だけを読む。"""
    必須 = ("版", "状態署名", "目的", "要求状態", "成立状態", "残差", "要求認識", "再評価待ち")
    if any(not hasattr(状態, name) for name in 必須):
        raise TypeError("状態契約が不足")
    return 状態ビュー(
        状態.版, 状態.状態署名, tuple(状態.目的), frozenset(状態.要求状態),
        frozenset(状態.成立状態), frozenset(状態.残差),
        frozenset(状態.要求認識), frozenset(状態.再評価待ち),
    )


def 状態差を受理(前, 局所結果):
    """局所結果を全体状態へ反映する唯一の共通更新入口。局所部品は直接更新しない。"""
    from ..統合駆動_v2.状態更新 import 状態更新
    return 状態更新(前, 局所結果)


def 閉包を確認(状態) -> bool:
    from ..統合駆動_v2.状態更新 import 閉包可能
    return bool(閉包可能(状態))


def ノード意味署名(状態, ノード: str) -> str:
    文字(ノード, "依存ノード")
    from ..統合駆動_v2.状態更新 import ノード署名
    return ノード署名(状態, ノード)


def ノード意味値(状態, ノード: str):
    文字(ノード, "依存ノード")
    from ..統合駆動_v2.状態更新 import ノード値
    return ノード値(状態, ノード)

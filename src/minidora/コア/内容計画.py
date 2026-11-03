"""根拠・条件・留保を表面表現より前に保持する共通内容計画。"""
from __future__ import annotations
from dataclasses import dataclass
from .値 import 文字, 文字列組, 署名


@dataclass(frozen=True, slots=True)
class 内容単位:
    種別: str
    本文: str
    根拠: tuple[str, ...] = ()
    条件: tuple[str, ...] = ()

    def __post_init__(self):
        文字(self.種別, "内容種別")
        文字(self.本文, "内容本文")
        文字列組(self.根拠, "内容根拠", 一意=False)
        文字列組(self.条件, "内容条件", 一意=False)


@dataclass(frozen=True, slots=True)
class 内容計画:
    単位: tuple[内容単位, ...]
    留保: tuple[str, ...] = ()
    由来: tuple[str, ...] = ()

    def __post_init__(self):
        if not isinstance(self.単位, tuple) or any(not isinstance(x, 内容単位) for x in self.単位):
            raise TypeError("内容計画の単位型不正")
        文字列組(self.留保, "内容留保", 一意=False)
        文字列組(self.由来, "内容由来", 一意=False)

    @property
    def 意味署名(self) -> str:
        return 署名(self)


def 内容計画を構成(本文: str, *, 種別="本文", 根拠=(), 条件=(), 留保=(), 由来=()) -> 内容計画:
    """領域固有の文章生成器が返した明示内容を、共通の内容境界へ載せる。

    ここでは専門内容を発明・要約・翻訳しない。与えられた本文と付随条件を保持するだけである。
    """
    return 内容計画(
        (内容単位(str(種別), 本文, tuple(str(x) for x in 根拠), tuple(str(x) for x in 条件)),),
        tuple(str(x) for x in 留保),
        tuple(str(x) for x in 由来),
    )


def 内容計画を検査(計画: 内容計画, *, 許可根拠: set[str] | frozenset[str] | None = None) -> bool:
    if not isinstance(計画, 内容計画):
        return False
    if not 計画.単位:
        return False
    if 許可根拠 is not None:
        allowed = set(許可根拠)
        if any(ref not in allowed for unit in 計画.単位 for ref in unit.根拠):
            return False
    return all(unit.本文.strip() and all(x.strip() for x in unit.条件) for unit in 計画.単位)


def 内容計画を表現(計画: 内容計画, *, 詳細=False, 最大文字数=100000) -> str:
    """互換入口。内容計画の表現実体は出力系が所有する。"""
    from ..出力系.互換 import 既存内容計画を表現
    return 既存内容計画を表現(計画, 詳細=詳細, 最大文字数=最大文字数)

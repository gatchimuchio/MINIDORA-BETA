"""草案と採用を分離する検証契約。検証器の範囲を越える正しさは主張しない。"""
from __future__ import annotations
from dataclasses import dataclass, field
from typing import Callable
from .値 import 文字, 文字列組, 署名


@dataclass(frozen=True, slots=True)
class HDS検証器:
    ID: str
    検証: Callable = field(repr=False, compare=False, metadata={"意味": False})
    版: str = "v1"

    def __post_init__(self):
        文字(self.ID)
        文字(self.版)
        if not callable(self.検証):
            raise TypeError("明示的な検証関数が必要")


@dataclass(frozen=True, slots=True)
class HDS草案:
    ID: str
    成果: tuple[tuple[str, object], ...]
    検証器ID: tuple[str, ...]
    依存署名: tuple[tuple[str, str], ...] = ()
    追加状態: frozenset[str] = frozenset()
    解消残差: frozenset[str] = frozenset()
    区分: str = "未検証"

    def __post_init__(self):
        文字(self.ID)
        文字列組(self.検証器ID)
        if not self.検証器ID:
            raise ValueError("検証器を持たない草案は採用候補にできない")
        if self.区分 not in ("未検証", "検証済み", "棄却", "失効"):
            raise ValueError("草案区分が不正")
        if not isinstance(self.成果, tuple):
            raise TypeError("草案成果はtuple")
        文字列組(tuple(k for k, _ in self.成果))
        文字列組(tuple(k for k, _ in self.依存署名))
        for k, v in self.依存署名:
            文字(v)
        for 群 in (self.追加状態, self.解消残差):
            if not isinstance(群, frozenset):
                raise TypeError("草案効果はfrozenset")
            for x in 群:
                文字(x)
        署名(self.成果)

    @property
    def 内容署名(self) -> str:
        return 署名((self.ID, self.成果, self.検証器ID, self.依存署名, self.追加状態, self.解消残差))


@dataclass(frozen=True, slots=True)
class HDS検証票:
    草案ID: str
    内容署名: str
    検証契約: tuple[tuple[str, str], ...]
    合格: bool
    理由: tuple[str, ...] = ()

    def __post_init__(self):
        文字(self.草案ID)
        文字(self.内容署名)
        if type(self.合格) is not bool:
            raise TypeError("検証合格はbool")


@dataclass(frozen=True, slots=True)
class HDS先行検証結果:
    採用接頭部: tuple[object, ...]
    巻戻し部分: tuple[object, ...]
    全体成立: bool


def 先行草案を検証(候補列: tuple[object, ...], 検証: Callable[[tuple[object, ...]], bool]) -> HDS先行検証結果:
    for i in range(1, len(候補列) + 1):
        結果 = 検証(候補列[:i])
        if type(結果) is not bool:
            raise TypeError("接頭部検証はboolを返す必要がある")
        if not 結果:
            return HDS先行検証結果(候補列[:i - 1], 候補列[i - 1:], False)
    return HDS先行検証結果(候補列, (), True)

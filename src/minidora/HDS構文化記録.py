from __future__ import annotations

from dataclasses import dataclass
from enum import StrEnum

from .HDS中間表現 import HDSIR
from .HDS構文化記録_v1_1 import (
    HDSチェックリスト項目,
    HDS失敗署名候補,
    HDS状態遷移図,
    HDS暗黙知記録,
    HDS認知世界差分,
    HDS監査参照候補,
)
from .HDS構文化記録_v1_3 import HDS作用差分構造


class HDS監査状態(StrEnum):
    観測 = "観測"
    推定 = "推定"
    要求 = "要求"
    未固定 = "未固定"
    留保 = "留保"


class HDS原理段階(StrEnum):
    未形成 = "未形成"
    影 = "影"
    パターン = "パターン"
    機構候補 = "機構候補"
    原理候補 = "原理候補"


@dataclass(frozen=True, slots=True)
class HDS認知世界断片:
    '公開構文化器が入力表層から観測できた認知世界の有限断片。'

    発話主体: tuple[str, ...] = ()
    作用主体: tuple[str, ...] = ()
    対象: tuple[str, ...] = ()
    時間: tuple[str, ...] = ()
    空間: tuple[str, ...] = ()
    目的: tuple[str, ...] = ()
    機構: tuple[str, ...] = ()
    未固定座標: tuple[str, ...] = ()


@dataclass(frozen=True, slots=True)
class HDS監査項目:
    項目ID: str
    層: str
    種別: str
    内容: str
    状態: HDS監査状態
    由来: str = '公開HDS 構文化器'
    必要情報: tuple[str, ...] = ()
    再開放条件: tuple[str, ...] = ()


@dataclass(frozen=True, slots=True)
class HDS監査要求:
    'HDS判断側へ渡す監査入力要求。構文化器自身は合否を決めない。'

    要求ID: str
    種別: str
    理由: str
    必要情報: tuple[str, ...] = ()
    影響参照: tuple[str, ...] = ()
    次の観測候補: tuple[str, ...] = ()


@dataclass(frozen=True, slots=True)
class HDS原理探索要求:
    段階: HDS原理段階 = HDS原理段階.未形成
    明示語: tuple[str, ...] = ()
    原理質問候補: tuple[str, ...] = ()
    必要監査: tuple[str, ...] = ()
    適用範囲: tuple[str, ...] = ()
    反証条件: tuple[str, ...] = ()
    再開放条件: tuple[str, ...] = ()


@dataclass(frozen=True, slots=True)
class HDS保持契約:
    全座標保持: bool = True
    全関係保持: bool = True
    不確実性保持: bool = True
    残差保持: bool = True
    由来保持: bool = True
    旧解釈保持: bool = True
    不可逆剪定禁止: bool = True
    時間履歴保持: bool = True
    認知世界履歴保持: bool = True
    帰還経路保持: bool = True


@dataclass(frozen=True, slots=True)
class HDS構文化器成果:
    IR: HDSIR
    認知世界: HDS認知世界断片
    監査項目: tuple[HDS監査項目, ...]
    監査要求: tuple[HDS監査要求, ...]
    原理探索: HDS原理探索要求
    保持契約: HDS保持契約 = HDS保持契約()
    状態遷移: HDS状態遷移図 = HDS状態遷移図()
    暗黙知構造: tuple[HDS暗黙知記録, ...] = ()
    失敗署名候補: tuple[HDS失敗署名候補, ...] = ()
    チェックリスト: tuple[HDSチェックリスト項目, ...] = ()
    認知世界差分: HDS認知世界差分 = HDS認知世界差分()
    監査参照候補: tuple[HDS監査参照候補, ...] = ()
    作用差分構造: HDS作用差分構造 = HDS作用差分構造()

    @property
    def 未固定座標(self) -> tuple[str, ...]:
        return self.認知世界.未固定座標

    @property
    def 要求種別(self) -> tuple[str, ...]:
        return tuple(dict.fromkeys(item.種別 for item in self.監査要求))


HDS_構文化器_META_PREFIXES = (
    "監査.",
    "保持.",
    "暫定性.",
    "帰還.",
)


__all__ = [
    "HDS監査状態",
    "HDS原理段階",
    "HDS認知世界断片",
    "HDS監査項目",
    "HDS監査要求",
    "HDS原理探索要求",
    "HDS保持契約",
    'HDS構文化器成果',
    'HDS_構文化器_META_PREFIXES',
]

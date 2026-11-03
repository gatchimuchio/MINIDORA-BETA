from __future__ import annotations

from dataclasses import dataclass, field
from enum import StrEnum
from typing import Any

from .命令 import 手順


class 値状態(StrEnum):
    確定 = "確定"
    推定 = "推定"
    未確定 = "未確定"
    未観測 = "未観測"
    矛盾 = "矛盾"
    留保 = "留保"


_実行阻害値状態 = frozenset({
    値状態.未確定,
    値状態.未観測,
    値状態.矛盾,
    値状態.留保,
})


@dataclass(frozen=True, slots=True)
class HDS座標:
    座標ID: str
    種別: str
    内容: Any
    値状態: 値状態 = 値状態.確定
    原文範囲: tuple[int, int] | None = None
    由来: str = "自然言語入力"
    暫定性: str = "原則暫定"
    再開放条件: tuple[str, ...] = ()


@dataclass(frozen=True, slots=True)
class HDS関係:
    関係ID: str
    始点: tuple[str, ...]
    終点: tuple[str, ...]
    種別: str
    条件: tuple[str, ...] = ()
    値状態: 値状態 = 値状態.確定
    由来: str = "自然言語入力"
    暫定性: str = "原則暫定"


@dataclass(frozen=True, slots=True)
class HDS残差:
    残差ID: str
    種別: str
    原文: str
    理由: str
    影響座標: tuple[str, ...] = ()
    解消条件: tuple[str, ...] = ()


@dataclass(frozen=True, slots=True)
class HDS意味作用:
    作用ID: str
    種別: str
    入力参照: tuple[str, ...]
    出力参照: tuple[str, ...]
    変換: str
    保持構造: tuple[str, ...] = ()
    損失: tuple[str, ...] = ()
    検証: tuple[str, ...] = ()


@dataclass(frozen=True, slots=True)
class HDS実行核:
    """HDS-IRのうち、現行Layer-0へlowering可能な局所閉包。"""

    作用: str | None = None
    入力座標: tuple[str, ...] = ()
    出力座標: str = "結果"
    境界: tuple[str, ...] = ()
    検証: tuple[str, ...] = ()


@dataclass(frozen=True, slots=True)
class HDSIR:
    """自然言語をHDSの意味世界へ射影した、開放・無欠損志向の実行中間表現。"""

    原文: str
    正規化文: str
    認知世界ID: str
    座標: tuple[HDS座標, ...]
    関係: tuple[HDS関係, ...]
    残差: tuple[HDS残差, ...]
    意味作用履歴: tuple[HDS意味作用, ...]
    実行核: HDS実行核
    初期状態: dict[str, Any] = field(default_factory=dict)
    参照必須: bool = False
    種別: str = "一般"
    閉包状態: str = "OPEN"
    表現状態: str = "部分構文化"
    保持状態: str = "全領域有効"
    暫定性状態: str = "原則暫定"
    手順: 手順 | None = None
    入力言語: str = "ja"
    出力言語: str | None = None
    文脈引用: tuple[str, ...] = ()

    def 座標辞書(self) -> dict[str, HDS座標]:
        return {item.座標ID: item for item in self.座標}

    @property
    def 実行阻害理由(self) -> tuple[str, ...]:
        """Layer-0へ昇格できない、IR自身から機械判定可能な理由を返す。"""
        reasons: list[str] = []
        if self.手順 is None:
            reasons.append("実行手順未閉包")
        if any(r.種別 == '意味_loss' for r in self.残差):
            reasons.append('意味_loss残差')

        coordinates = self.座標辞書()
        for coordinate_id in self.実行核.入力座標:
            coordinate = coordinates.get(coordinate_id)
            if coordinate is None:
                reasons.append(f"実行入力座標欠落:{coordinate_id}")
                continue
            if coordinate.値状態 in _実行阻害値状態:
                reasons.append(f"実行入力{coordinate.値状態.value}:{coordinate_id}")
        return tuple(reasons)

    @property
    def 実行可能(self) -> bool:
        return not self.実行阻害理由

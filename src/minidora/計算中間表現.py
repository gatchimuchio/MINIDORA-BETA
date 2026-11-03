from __future__ import annotations

from dataclasses import dataclass, field
from enum import StrEnum
from typing import Any


計算中間表現版 = "計算中間表現-v1"


class 計算値種別(StrEnum):
    即値 = "即値"
    状態値 = "状態値"
    状態住所 = "状態住所"


@dataclass(frozen=True, slots=True)
class 計算値:
    """計算実行境界へ渡す値。

    自然言語やHDS座標の意味は持たず、即値・状態値参照・状態住所を型で分ける。
    """

    種別: 計算値種別
    内容: Any

    def __post_init__(self) -> None:
        if self.種別 in {計算値種別.状態値, 計算値種別.状態住所}:
            if not isinstance(self.内容, str) or not self.内容.strip():
                raise ValueError(f"{self.種別.value}には空でない住所文字列が必要")

    @classmethod
    def 即値(cls, 内容: Any) -> "計算値":
        return cls(計算値種別.即値, 内容)

    @classmethod
    def 状態値(cls, 住所: str) -> "計算値":
        return cls(計算値種別.状態値, 住所)

    @classmethod
    def 状態住所(cls, 住所: str) -> "計算値":
        return cls(計算値種別.状態住所, 住所)


class 計算作用(StrEnum):
    設定 = "設定"
    取得 = "取得"
    抽出 = "抽出"
    加算 = "加算"
    減算 = "減算"
    乗算 = "乗算"
    除算 = "除算"
    比較 = "比較"
    計数 = "計数"
    結合 = "結合"
    交換 = "交換"
    反転 = "反転"
    停止 = "停止"


@dataclass(frozen=True, slots=True)
class 計算命令:
    命令ID: str
    名称: str
    作用: 計算作用
    入力: tuple[計算値, ...] = field(default_factory=tuple)
    対象住所: str | None = None
    出力住所: str | None = None
    根拠: tuple[str, ...] = field(default_factory=tuple)
    境界: tuple[str, ...] = field(default_factory=tuple)

    def __post_init__(self) -> None:
        if not self.命令ID.strip():
            raise ValueError("計算命令IDは空にできない")
        if not self.名称.strip():
            raise ValueError("計算命令名称は空にできない")
        if self.対象住所 is not None and not self.対象住所.strip():
            raise ValueError("対象住所は空文字にできない")
        if self.出力住所 is not None and not self.出力住所.strip():
            raise ValueError("出力住所は空文字にできない")


@dataclass(frozen=True, slots=True)
class 計算中間表現:
    """意味解釈後・実行直前の計算専用中間表現。

    HDS-IR、日本語命令形P、LLM模型中核のいずれとも同一ではない。
    """

    名称: str
    命令列: tuple[計算命令, ...]
    出力住所: str = "結果"
    由来: str = ""
    由来参照: tuple[str, ...] = field(default_factory=tuple)
    境界: tuple[str, ...] = field(default_factory=tuple)
    検証: tuple[str, ...] = field(default_factory=tuple)
    版: str = 計算中間表現版

    def __post_init__(self) -> None:
        if not self.名称.strip():
            raise ValueError("計算中間表現名称は空にできない")
        if not self.出力住所.strip():
            raise ValueError("計算中間表現の出力住所は空にできない")
        ids = tuple(item.命令ID for item in self.命令列)
        if len(ids) != len(set(ids)):
            raise ValueError("計算命令IDは一つの計算中間表現内で一意である必要がある")


@dataclass(frozen=True, slots=True)
class 計算履歴:
    命令ID: str
    名称: str
    作用: 計算作用
    対象住所: str | None
    入力値: tuple[Any, ...]
    結果: Any
    出力住所: str | None
    根拠: tuple[str, ...] = field(default_factory=tuple)


@dataclass(frozen=True, slots=True)
class 計算実行結果:
    状態: dict[str, Any]
    履歴: tuple[計算履歴, ...]
    停止済み: bool
    出力: Any
    中間表現版: str = 計算中間表現版


__all__ = [
    "計算中間表現版",
    "計算値種別",
    "計算値",
    "計算作用",
    "計算命令",
    "計算中間表現",
    "計算履歴",
    "計算実行結果",
]

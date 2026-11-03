from __future__ import annotations

from dataclasses import dataclass, field
from enum import StrEnum
from typing import Any


class 作用(StrEnum):
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
class 命令:
    名称: str
    作用: 作用
    対象: str | None = None
    引数: tuple[Any, ...] = field(default_factory=tuple)
    更新先: str | None = None
    根拠: tuple[str, ...] = field(default_factory=tuple)


@dataclass(frozen=True, slots=True)
class 手順:
    名称: str
    命令列: tuple[命令, ...]
    由来: str = ""

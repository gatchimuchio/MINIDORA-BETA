from __future__ import annotations
from dataclasses import dataclass, field, asdict
from datetime import datetime
from typing import Any
from ..採否 import 実行状態

@dataclass(frozen=True, slots=True)
class 参照資料:
    識別子: str
    題名: str
    出典: str
    URL: str = ""
    公開時刻: datetime | None = None
    本文: str = ""

    def 辞書化(self) -> dict[str, Any]:
        資料 = asdict(self)
        資料["公開時刻"] = self.公開時刻.isoformat() if self.公開時刻 else None
        return 資料

@dataclass(frozen=True, slots=True)
class 能力結果:
    成立: bool
    本文: str
    根拠: tuple[str, ...] = ()
    参照: tuple[参照資料, ...] = ()
    データ: dict[str, Any] = field(default_factory=dict)
    保留理由: str = ""
    # 旧6項目ABIは維持する。Core等の境界だけが必要に応じて明示する。
    採否状態: 実行状態 | None = None

    @property
    def 状態(self) -> 実行状態:
        """製品境界で使う実効状態。旧結果は成立boolから互換的に導出する。"""
        if isinstance(self.採否状態, 実行状態):
            return self.採否状態
        return 実行状態.合格 if self.成立 is True else 実行状態.保留

@dataclass(frozen=True, slots=True)
class 製品応答:
    セッションID: str
    本文: str
    状態: str
    経路: str
    追跡ID: str
    監査ハッシュ: str
    参照: tuple[参照資料, ...] = ()
    能力: tuple[str, ...] = ()
    メタデータ: dict[str, Any] = field(default_factory=dict)

    def 辞書化(self) -> dict[str, Any]:
        return {
            "session_id": self.セッションID,
            "response": self.本文,
            "status": self.状態,
            '経路': self.経路,
            '追跡_id': self.追跡ID,
            '追跡_hash': self.監査ハッシュ,
            "capabilities": list(self.能力),
            "sources": [r.辞書化() for r in self.参照],
            "metadata": self.メタデータ,
        }

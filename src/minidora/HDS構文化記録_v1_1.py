from __future__ import annotations

from dataclasses import dataclass
from enum import StrEnum


class HDS失敗署名状態(StrEnum):
    候補 = "PROBATION"
    有効 = "ACTIVE"
    領域限定 = "DOMAIN_LOCAL"
    廃止候補 = "DEPRECATED"
    統合済み = "MERGED"
    主起動退役 = "RETIRED_FROM_PRIMARY_TRIGGER"


@dataclass(frozen=True, slots=True)
class HDS状態ノード:
    ノードID: str
    名称: str
    種別: str = "状態"
    由来: str = '公開HDS 構文化器'
    暫定性: str = "原則暫定"


@dataclass(frozen=True, slots=True)
class HDS遷移辺:
    遷移ID: str
    始点: str | None
    終点: str | None
    条件: tuple[str, ...] = ()
    作用: tuple[str, ...] = ()
    可逆: bool | None = None
    rollback先: str | None = None
    由来: str = '公開HDS 構文化器'
    暫定性: str = "原則暫定"


@dataclass(frozen=True, slots=True)
class HDS状態遷移図:
    ノード: tuple[HDS状態ノード, ...] = ()
    遷移: tuple[HDS遷移辺, ...] = ()
    未閉包: tuple[str, ...] = ()


@dataclass(frozen=True, slots=True)
class HDS暗黙知記録:
    記録ID: str
    種別: str
    主語: str | None
    内容: str
    分類: str | None = None
    適用範囲: tuple[str, ...] = ()
    不確実性: str | None = None
    由来: str = '公開HDS 構文化器'
    暫定性: str = "原則暫定"
    再開放条件: tuple[str, ...] = ()


@dataclass(frozen=True, slots=True)
class HDS失敗署名候補:
    署名ID: str
    失敗分類: str
    症状: str
    構造原因: str
    起動条件: tuple[str, ...] = ()
    影響範囲: tuple[str, ...] = ()
    非影響範囲: tuple[str, ...] = ()
    違反前提: tuple[str, ...] = ()
    回復: tuple[str, ...] = ()
    次探索軸: tuple[str, ...] = ()
    再利用チェック: tuple[str, ...] = ()
    反復回数: int = 1
    状態: HDS失敗署名状態 = HDS失敗署名状態.候補


@dataclass(frozen=True, slots=True)
class HDSチェックリスト項目:
    項目ID: str
    失敗署名参照: str | None
    監査質問: str
    必要証拠: tuple[str, ...]
    関門対応: tuple[str, ...]
    停止または回復規則: tuple[str, ...]
    次Run確認: tuple[str, ...]
    状態: HDS失敗署名状態 = HDS失敗署名状態.候補


@dataclass(frozen=True, slots=True)
class HDS認知世界差分:
    前回世界参照: str | None = None
    現行世界参照: str | None = None
    追加座標: tuple[str, ...] = ()
    消失座標: tuple[str, ...] = ()
    変更関係: tuple[str, ...] = ()
    再解釈要求: tuple[str, ...] = ()
    旧世界保持: bool = True


@dataclass(frozen=True, slots=True)
class HDS監査参照候補:
    問合せ: str
    種別: str
    関門対応: tuple[str, ...] = ()
    由来要求: str | None = None
    優先度: int = 0


__all__ = [
    "HDS失敗署名状態",
    "HDS状態ノード",
    "HDS遷移辺",
    "HDS状態遷移図",
    "HDS暗黙知記録",
    "HDS失敗署名候補",
    "HDSチェックリスト項目",
    "HDS認知世界差分",
    "HDS監査参照候補",
]

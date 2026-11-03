from __future__ import annotations

from dataclasses import dataclass
from enum import StrEnum

from .HDS構文化記録_v1_1 import HDS失敗署名状態


class HDS改善対象(StrEnum):
    座標生成規則 = "coordinate_generation_rule"
    作用素集合 = "operator_set"
    保持構造 = "retention_structure"
    Domain適合器 = 'domain_適合器'
    IdentityLock = "identity_lock"
    Framework射影 = 'framework_射影'
    Checklist = "checklist"


@dataclass(frozen=True, slots=True)
class HDS失敗観測:
    観測ID: str
    Run参照: str
    候補署名ID: str
    失敗分類: str
    症状: str
    構造原因: str
    起動条件: tuple[str, ...] = ()
    適用範囲: tuple[str, ...] = ()
    由来: str = '公開HDS 構文化器 v1.2'


@dataclass(frozen=True, slots=True)
class HDS失敗署名記録:
    署名ID: str
    失敗分類: str
    構造原因: str
    共通起動条件: tuple[str, ...] = ()
    局所起動条件: tuple[str, ...] = ()
    症状履歴: tuple[str, ...] = ()
    Run履歴: tuple[str, ...] = ()
    影響範囲: tuple[str, ...] = ()
    非影響範囲: tuple[str, ...] = ()
    違反前提: tuple[str, ...] = ()
    回復: tuple[str, ...] = ()
    次探索軸: tuple[str, ...] = ()
    再利用チェック: tuple[str, ...] = ()
    反復回数: int = 1
    独立Run数: int = 1
    状態: HDS失敗署名状態 = HDS失敗署名状態.候補
    由来候補ID: tuple[str, ...] = ()


@dataclass(frozen=True, slots=True)
class HDS抽出規則改善候補:
    候補ID: str
    改善対象: HDS改善対象
    失敗署名参照: tuple[str, ...]
    問題構造: str
    提案: str
    根拠: tuple[str, ...] = ()
    反復回数: int = 1
    独立Run数: int = 1
    状態: HDS失敗署名状態 = HDS失敗署名状態.候補
    昇格可能: bool = False
    自動適用禁止: bool = True
    昇格条件: tuple[str, ...] = (
        "同型失敗の独立反復を確認する",
        "既存正例・負例・境界例への回帰を確認する",
        "HDS本体または権限を持つ上位判断主体が採否する",
    )
    再開放条件: tuple[str, ...] = (
        "新観測・反例・境界変更・回帰で再監査する",
    )


@dataclass(frozen=True, slots=True)
class HDS失敗署名BankSnapshot:
    版: str
    観測数: int
    観測履歴: tuple[HDS失敗観測, ...] = ()
    署名: tuple[HDS失敗署名記録, ...] = ()
    改善候補: tuple[HDS抽出規則改善候補, ...] = ()
    旧記録保持: bool = True
    自動自己改変禁止: bool = True


__all__ = [
    "HDS改善対象",
    "HDS失敗観測",
    "HDS失敗署名記録",
    "HDS抽出規則改善候補",
    "HDS失敗署名BankSnapshot",
]

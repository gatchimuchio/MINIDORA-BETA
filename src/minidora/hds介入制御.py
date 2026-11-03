from __future__ import annotations

from dataclasses import dataclass
from enum import StrEnum
from typing import Protocol, Sequence

from .HDS実行主体 import (
    HDS作用機会 as HDS一般作用機会,
    HDS作用記録 as HDS一般作用記録,
    HDS実行状態 as HDS一般実行状態,
    標準HDS作用選択器,
)


class 既存作用(StrEnum):
    参照取得 = '参照'
    作業再作用 = 'EXISTING_作業_RECONCILE'
    局所再照合 = "EXISTING_LOCAL_REPARSE"
    能力模型照合 = 'EXISTING_能力_模型'
    計算実行 = "EXISTING_COMPUTE_EXECUTOR"


class 既存判定(StrEnum):
    実行中 = "RUNNING"
    承認 = "APPROVE"
    保留 = "SUSPEND"
    失敗 = "FAIL"


class 残差種別(StrEnum):
    観測不足 = "OBSERVATION_SHORTAGE"
    問題意味損失 = 'QUESTION_意味_LOSS'
    候補意味損失 = '候補_意味_LOSS'
    資料意味損失 = '資料_意味_LOSS'
    候補競合 = '候補_CONFLICT'
    候補識別不足 = '候補_DISCRIMINATION_INSUFFICIENT'
    状態差未消費 = '状態_DELTA_UNCONSUMED'
    計算要求 = "COMPUTE_REQUIRED"
    未解残差 = 'UNRESOLVED_残差'


class HDS指令種別(StrEnum):
    不介入 = "NO_INTERVENTION"
    既存作用起動 = 'RUN_EXISTING_作用'
    停止要求 = "REQUEST_STOP"


@dataclass(frozen=True, slots=True)
class HDS監督状態:
    """旧選択問題互換の最小観測面。回答ラベル・候補本文・候補得点は含めない。"""

    既存判定: 既存判定
    出力存在: bool
    直接検証済み: bool
    根拠あり: bool
    参照状態署名: str
    候補状態署名: str
    残差: frozenset[残差種別]


@dataclass(frozen=True, slots=True)
class 既存作用機会:
    """旧選択問題互換でMINIDORA側が公開する作用能力metadata。"""

    作用: 既存作用
    解消対象: frozenset[残差種別]
    作用入力署名: str
    資源負荷: int = 1
    状態変更可能: bool = True
    根拠: tuple[str, ...] = ()


@dataclass(frozen=True, slots=True)
class HDS指令:
    種別: HDS指令種別
    作用: 既存作用 | None = None
    対象残差: tuple[残差種別, ...] = ()
    理由: tuple[str, ...] = ()


@dataclass(frozen=True, slots=True)
class HDS介入記録:
    作用: 既存作用
    作用入力署名: str
    対象残差: tuple[残差種別, ...]
    進展: bool | None = None


@dataclass(frozen=True, slots=True)
class 介入観測:
    状態: HDS監督状態
    作用機会: tuple[既存作用機会, ...]
    HDS介入記録: tuple[HDS介入記録, ...]
    HDS残予算: int


class HDS介入制御(Protocol):
    def 判定(self, 観測: 介入観測) -> HDS指令: ...


class 標準HDS介入制御:
    """MINIDORA30互換の旧HDS監督制御。

    新しいHDS-first coreの主体ではない。GPQA正本・旧実行系互換のため、従来の
    `NO_INTERVENTION / RUN_EXISTING_ACTION / REQUEST_STOP` 契約を保持する。
    """

    def 判定(self, 観測: 介入観測) -> HDS指令:
        状態 = 観測.状態
        if 状態.既存判定 == 既存判定.承認 and 状態.出力存在 and not 状態.残差:
            return HDS指令(HDS指令種別.不介入, 理由=("EXISTING_MINIDORA_APPROVED",))
        if 観測.HDS残予算 <= 0:
            return HDS指令(HDS指令種別.停止要求, 理由=('HDS_INTERVENTION_予算_EXHAUSTED',))
        if not 状態.残差:
            return HDS指令(HDS指令種別.停止要求, 理由=('NO_RECOVERABLE_残差',))

        used = {(row.作用, row.作用入力署名) for row in 観測.HDS介入記録}
        candidates: list[tuple[int, float, int, str, 既存作用機会, tuple[残差種別, ...]]] = []
        for offer in 観測.作用機会:
            if not offer.状態変更可能 or (offer.作用, offer.作用入力署名) in used:
                continue
            targets = tuple(sorted(状態.残差.intersection(offer.解消対象), key=lambda x: x.value))
            if not targets:
                continue
            coverage = len(targets)
            specificity = coverage / max(1, len(offer.解消対象))
            candidates.append((coverage, specificity, max(0, int(offer.資源負荷)), offer.作用.value, offer, targets))

        if not candidates:
            return HDS指令(HDS指令種別.停止要求, 理由=('NO_PRODUCTIVE_EXISTING_作用',))

        candidates.sort(key=lambda row: (-row[0], -row[1], row[2], row[3]))
        _, _, _, _, offer, targets = candidates[0]
        return HDS指令(
            HDS指令種別.既存作用起動,
            offer.作用,
            targets,
            tuple(dict.fromkeys((
                '残差_TO_EXISTING_作用',
                *offer.根拠,
                *(f"TARGET:{item.value}" for item in targets),
            ))),
        )


class 標準HDS一般作用制御:
    """HDS-first core用の常時作用選択境界。

    異常時だけではなく、目的未達・残差・中間状態を見て任意の登録作用から次作用を選ぶ。
    COMMIT自体は`HDS実行主体`が要求状態と残差を検査して行う。
    """

    def __init__(self) -> None:
        self._selector = 標準HDS作用選択器()

    def 選択(
        self,
        状態: HDS一般実行状態,
        作用機会: Sequence[HDS一般作用機会],
        履歴: Sequence[HDS一般作用記録] = (),
    ) -> HDS一般作用機会 | None:
        return self._selector.選択(状態, 作用機会, 履歴)


__all__ = [
    "既存作用",
    "既存判定",
    "残差種別",
    "HDS指令種別",
    "HDS監督状態",
    "既存作用機会",
    "HDS指令",
    "HDS介入記録",
    "介入観測",
    "HDS介入制御",
    "標準HDS介入制御",
    "HDS一般作用機会",
    "HDS一般作用記録",
    "HDS一般実行状態",
    "標準HDS一般作用制御",
]

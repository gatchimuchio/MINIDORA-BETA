from __future__ import annotations

from dataclasses import dataclass
from enum import StrEnum
from typing import Iterable


class 既存提案源(StrEnum):
    K3 = "K3"
    能力模型 = '能力_模型'
    計算 = "COMPUTE"
    直接関係 = 'DIRECT_関係'


class 既存提案状態(StrEnum):
    承認候補 = "APPROVE"
    保留 = "SUSPEND"
    失敗 = "FAIL"


@dataclass(frozen=True, slots=True)
class 既存能力提案:
    """既存MINIDORA部品が形成した候補。HDS由来の候補は受け付けない。"""

    源: 既存提案源
    状態: 既存提案状態
    回答: str | None = None
    根拠成立: bool = False
    一意: bool = False
    直接検証済み: bool = False
    計算完全一致: bool = False
    理由: tuple[str, ...] = ()

    @property
    def 有効(self) -> bool:
        return bool(
            self.状態 == 既存提案状態.承認候補
            and self.回答 is not None
            and self.根拠成立
            and self.一意
        )


@dataclass(frozen=True, slots=True)
class 既存解決結果:
    状態: 既存提案状態
    回答: str | None
    残差: tuple[str, ...]
    採用源: tuple[既存提案源, ...] = ()
    理由: tuple[str, ...] = ()


def _answers(rows: Iterable[既存能力提案]) -> frozenset[str]:
    return frozenset(str(row.回答) for row in rows if row.有効 and row.回答 is not None)


def 既存MINIDORA提案解決(
    提案群: Iterable[既存能力提案],
    *,
    計算要求: bool = False,
) -> 既存解決結果:
    """既存能力同士だけを統合する。HDSは呼ばず、候補の勝者もHDSに選ばせない。"""

    rows = tuple(提案群)
    valid = tuple(row for row in rows if row.有効)

    direct = tuple(row for row in valid if row.直接検証済み or row.源 == 既存提案源.直接関係)
    direct_answers = _answers(direct)
    if len(direct_answers) > 1:
        return 既存解決結果(
            既存提案状態.保留,
            None,
            ('候補_CONFLICT', 'DIRECT_関係_CONFLICT'),
            (),
            ("EXISTING_DIRECT_RELATIONS_DISAGREE",),
        )
    if len(direct_answers) == 1:
        answer = next(iter(direct_answers))
        exact_compute = tuple(
            row for row in valid
            if row.源 == 既存提案源.計算 and row.計算完全一致 and 計算要求
        )
        compute_answers = _answers(exact_compute)
        if compute_answers and compute_answers != {answer}:
            return 既存解決結果(
                既存提案状態.保留,
                None,
                ('候補_CONFLICT', "DIRECT_COMPUTE_CONFLICT"),
                (),
                ("EXISTING_DIRECT_AND_COMPUTE_DISAGREE",),
            )
        selected = tuple(row.源 for row in direct if row.回答 == answer)
        return 既存解決結果(
            既存提案状態.承認候補,
            answer,
            (),
            tuple(dict.fromkeys(selected)),
            ('EXISTING_DIRECT_関係_VERIFIED',),
        )

    if 計算要求:
        exact_compute = tuple(
            row for row in valid
            if row.源 == 既存提案源.計算 and row.計算完全一致
        )
        compute_answers = _answers(exact_compute)
        if len(compute_answers) > 1:
            return 既存解決結果(
                既存提案状態.保留,
                None,
                ('候補_CONFLICT', "COMPUTE_CONFLICT"),
                (),
                ("EXISTING_COMPUTE_RESULTS_DISAGREE",),
            )
        if len(compute_answers) == 1:
            answer = next(iter(compute_answers))
            selected = tuple(row.源 for row in exact_compute if row.回答 == answer)
            return 既存解決結果(
                既存提案状態.承認候補,
                answer,
                (),
                tuple(dict.fromkeys(selected)),
                ("EXISTING_COMPUTE_EXACT_MATCH",),
            )

    answers = _answers(valid)
    if len(answers) > 1:
        return 既存解決結果(
            既存提案状態.保留,
            None,
            ('候補_CONFLICT',),
            (),
            ("EXISTING_CAPABILITIES_DISAGREE",),
        )
    if len(answers) == 1:
        answer = next(iter(answers))
        selected = tuple(row.源 for row in valid if row.回答 == answer)
        return 既存解決結果(
            既存提案状態.承認候補,
            answer,
            (),
            tuple(dict.fromkeys(selected)),
            ("EXISTING_CAPABILITIES_AGREE", f"AGREEING_EXISTING_CAPABILITIES:{len(selected)}"),
        )

    if any(row.状態 == 既存提案状態.失敗 for row in rows):
        return 既存解決結果(
            既存提案状態.失敗,
            None,
            ('EXISTING_能力_FAILURE',),
            (),
            ("NO_VALID_EXISTING_PROPOSAL",),
        )

    return 既存解決結果(
        既存提案状態.保留,
        None,
        ('候補_DISCRIMINATION_INSUFFICIENT',),
        (),
        ("NO_VALID_EXISTING_PROPOSAL",),
    )


__all__ = [
    "既存提案源",
    "既存提案状態",
    "既存能力提案",
    "既存解決結果",
    "既存MINIDORA提案解決",
]

from __future__ import annotations

from dataclasses import dataclass
from typing import Callable, Generic, Iterable, Sequence, TypeVar

from .hds参照計画 import HDS参照計画

T = TypeVar("T")


@dataclass(frozen=True, slots=True)
class HDS多時間尺度政策:
    """安価な局所更新と高価な大域再照合を別作用として扱う運用政策。"""

    局所更新回数: int = 3
    大域周期: int = 4
    参照計画利用上限: int = 4
    並列lane数: int = 4
    先行草案幅: int = 1

    def __post_init__(self) -> None:
        if self.局所更新回数 < 1:
            raise ValueError("局所更新回数は1以上である必要がある")
        if self.大域周期 < 1:
            raise ValueError("大域周期は1以上である必要がある")
        if self.参照計画利用上限 < 1:
            raise ValueError("参照計画利用上限は1以上である必要がある")
        if self.並列lane数 < 1:
            raise ValueError("並列lane数は1以上である必要がある")
        if self.先行草案幅 < 1:
            raise ValueError("先行草案幅は1以上である必要がある")


@dataclass(frozen=True, slots=True)
class HDS再照合判断:
    大域再照合: bool
    理由: tuple[str, ...]


def HDS大域再照合判断(
    cycle: int,
    *,
    参照計画: HDS参照計画 | None = None,
    矛盾数: int = 0,
    新規性: float = 0.0,
    証拠不足: bool = False,
    政策: HDS多時間尺度政策 | None = None,
) -> HDS再照合判断:
    """固定周期だけに依存せず、証拠状態変化でも大域再照合を開く。"""

    policy = 政策 or HDS多時間尺度政策()
    reasons: list[str] = []
    if 参照計画 is None or not 参照計画.有効:
        reasons.append('取得_PLAN_INVALID')
    if int(矛盾数) > 0:
        reasons.append("CONTRADICTION_PRESENT")
    if bool(証拠不足):
        reasons.append('証拠_GAP')
    if float(新規性) >= 0.75:
        reasons.append("NOVELTY_HIGH")
    if max(0, int(cycle)) > 0 and int(cycle) % policy.大域周期 == 0:
        reasons.append("PERIODIC_GLOBAL_RECONCILE")
    return HDS再照合判断(bool(reasons), tuple(reasons))


@dataclass(frozen=True, slots=True)
class HDS阻害回復判断:
    作用: str
    参照計画無効化: bool
    計算量引上げ: bool
    Jへ留保: bool
    理由: tuple[str, ...]


def HDS阻害回復方針(reason_codes: Iterable[str]) -> HDS阻害回復判断:
    """失敗原因に応じて次作用を選ぶ。採否は行わない。"""

    codes = tuple(dict.fromkeys(str(code) for code in reason_codes if str(code)))
    証拠_markers = (
        '証拠',
        '参照',
        "PROVENANCE",
        "CONTRADICTION",
        "OBSERVATION",
        '資料_',
    )
    計算量_markers = ("DEPTH", '予算', "EXHAUST", "SEARCH", "INFERENCE")
    証拠_related = any(any(marker in code for marker in 証拠_markers) for code in codes)
    計算量_related = any(any(marker in code for marker in 計算量_markers) for code in codes)

    if 証拠_related:
        return HDS阻害回復判断(
            'REBUILD_取得_PLAN',
            True,
            計算量_related,
            False,
            codes or ('証拠_状態_CHANGED',),
        )
    if 計算量_related:
        return HDS阻害回復判断('RAISE_計算量_AND_RETRY', False, True, False, codes)
    return HDS阻害回復判断("SUSPEND_TO_J", False, False, True, codes or ("UNCLASSIFIED_BLOCKER",))


@dataclass(frozen=True, slots=True)
class HDS先行草案結果(Generic[T]):
    採用prefix: tuple[T, ...]
    却下位置: int | None
    rollback: bool
    検証回数: int


def HDS先行草案検証(
    draft: Sequence[T],
    検証器: Callable[[tuple[T, ...]], bool],
) -> HDS先行草案結果[T]:
    """先行生成した列をprefix単位で検証し、最初の不成立位置でrollbackする。

    これは生成効率用の補助作用であり、J/HDSの最終採否を代替しない。
    """

    accepted: list[T] = []
    checks = 0
    for index, item in enumerate(draft):
        候補 = tuple((*accepted, item))
        checks += 1
        if not bool(検証器(候補)):
            return HDS先行草案結果(tuple(accepted), index, True, checks)
        accepted.append(item)
    return HDS先行草案結果(tuple(accepted), None, False, checks)


@dataclass(frozen=True, slots=True)
class HDS共通入力表象:
    'modality固有parserの出力を中央処理へ渡す共通境界。\n\n    この構造自体は画像・音声・動画を解釈しない。外部適合器が形成済みの表象を受け取る。\n    '

    種別: str
    出典ID: str
    表象: object
    条件: tuple[tuple[str, str], ...] = ()


def HDS異種入力射影(
    種別: str,
    表象: object,
    *,
    出典ID: str,
    条件: Iterable[tuple[str, str]] = (),
) -> HDS共通入力表象:
    kind = str(種別).strip().casefold()
    if not kind:
        raise ValueError("入力種別は空にできない")
    情報源 = str(出典ID).strip()
    if not 情報源:
        raise ValueError("出典IDは空にできない")
    normalized_conditions = tuple(sorted((str(k), str(v)) for k, v in 条件))
    return HDS共通入力表象(kind, 情報源, 表象, normalized_conditions)


__all__ = [
    "HDS多時間尺度政策",
    "HDS再照合判断",
    "HDS大域再照合判断",
    "HDS阻害回復判断",
    "HDS阻害回復方針",
    "HDS先行草案結果",
    "HDS先行草案検証",
    "HDS共通入力表象",
    "HDS異種入力射影",
]

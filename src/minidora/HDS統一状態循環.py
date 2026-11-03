from __future__ import annotations

from dataclasses import dataclass, field
from enum import StrEnum
from hashlib import sha256
import json
from typing import Iterable, Mapping, Sequence

from .hds並列作業状態 import (
    HDS並列作業状態,
    HDS並列作業状態生成,
    HDS並列状態混合,
)
from .hds参照計画 import (
    HDS参照索引,
    HDS参照計画,
    HDS参照索引圧縮,
    HDS参照計画作成,
    HDS参照計画再利用可能,
    HDS参照計画消費,
    HDS参照計画適用,
    HDS参照計画無効化,
)
from .参照 import 参照記録


def _hash(prefix: str, value: object) -> str:
    raw = json.dumps(value, ensure_ascii=False, sort_keys=True, default=str, separators=(",", ":"))
    return prefix + sha256(raw.encode("utf-8")).hexdigest()[:20]


def _主体_payload(主体: object | None) -> object:
    if 主体 is None:
        return {"主体ID": "MINIDORA", "版": 0}
    if hasattr(主体, "辞書化") and callable(getattr(主体, "辞書化")):
        return getattr(主体, "辞書化")()
    if hasattr(主体, "辞書") and callable(getattr(主体, "辞書")):
        return getattr(主体, "辞書")()
    if hasattr(主体, "__dict__"):
        return dict(getattr(主体, "__dict__"))
    return repr(主体)


def HDS主体署名(主体: object | None) -> str:
    return _hash("SUBJ-", _主体_payload(主体))


def HDS候補状態署名(values: Mapping[str, float] | Iterable[tuple[str, float]] | None) -> str:
    normalized = tuple(sorted((str(k), float(v)) for k, v in dict(values or {}).items()))
    return _hash("CAND-", normalized)


class HDS統一作用(StrEnum):
    参照計画再利用 = 'REUSE_取得_PLAN'
    参照計画再構築 = 'REBUILD_取得_PLAN'
    大域再照合 = "GLOBAL_RECONCILE"
    検査点再活性 = 'REACTIVATE_検査点'
    専門作用 = '経路_SPECIALIST'
    計算量引上げ = 'RAISE_計算量'
    主体整合 = '主体_RECONCILE'
    先行草案検証 = "VERIFY_DRAFT"
    J引渡し = "HANDOFF_TO_J"
    J留保 = "SUSPEND_TO_J"


@dataclass(frozen=True, slots=True)
class HDS統一状態政策:
    """構文由来の作用を一つのrequest内で運用する政策。

    値は構造成立条件ではない。K3/GLMの観測値を初期値として採用し、実測で変更可能にする。
    """

    初期参照上限: int = 8
    参照拡張幅: int = 4
    最大循環: int = 4
    最大候補lane数: int = 4
    参照計画利用上限: int = 4
    索引bucket幅: int = 4
    不一致再照合閾値: float = 0.35

    def __post_init__(self) -> None:
        if self.初期参照上限 < 1:
            raise ValueError("初期参照上限は1以上")
        if self.参照拡張幅 < 1:
            raise ValueError("参照拡張幅は1以上")
        if self.最大循環 < 1:
            raise ValueError("最大循環は1以上")
        if self.最大候補lane数 < 1:
            raise ValueError("最大候補lane数は1以上")
        if self.参照計画利用上限 < 1:
            raise ValueError("参照計画利用上限は1以上")
        if self.索引bucket幅 < 1:
            raise ValueError("索引bucket幅は1以上")
        if not 0.0 <= float(self.不一致再照合閾値) <= 1.0:
            raise ValueError("不一致再照合閾値は0..1")


@dataclass(frozen=True, slots=True)
class HDS統一検査点:
    cycle: int
    段階: str
    参照計画ID: str | None
    参照計画残存利用回数: int
    候補状態署名: str
    主体署名: str
    並列状態ID: str | None
    残差: tuple[str, ...] = ()


@dataclass(frozen=True, slots=True)
class HDS統一状態Snapshot:
    sessionID: str
    cycle: int
    主体署名: str
    正本revision: str
    参照計画ID: str | None
    参照計画BindingID: str | None
    参照上限: int
    参照計画残存利用回数: int
    候補状態署名: str
    lane数: int
    lane不一致度: float
    検査点数: int
    作用履歴: tuple[str, ...]


@dataclass(slots=True)
class HDS統一状態Session:
    'K3/GLM/Llama3/横断構文化を一つのrequest状態循環へ接続する。\n\n    Llama3由来の主体snapshot、K3由来の検査点/再作用、GLM由来のarchive/index/plan分離、\n    横断構文化由来の「存在と実効作用の分離」を同じrequest-local状態で管理する。\n    `参照記録`正本そのものは変更しない。\n    '

    問い: str
    参照正本: tuple[参照記録, ...]
    主体状態: object | None = None
    認知世界ID: str = ""
    政策: HDS統一状態政策 = field(default_factory=HDS統一状態政策)
    cycle: int = 0
    参照上限: int = 0
    索引: HDS参照索引 | None = None
    参照計画: HDS参照計画 | None = None
    計画候補状態署名: str = ""
    計画主体署名: str = ""
    候補lane群: list[dict[str, float]] = field(default_factory=list)
    並列状態: HDS並列作業状態 | None = None
    検査点: list[HDS統一検査点] = field(default_factory=list)
    作用履歴: list[str] = field(default_factory=list)
    最終残差: tuple[str, ...] = ()

    def __post_init__(self) -> None:
        self.参照正本 = tuple(self.参照正本)
        if self.参照上限 <= 0:
            self.参照上限 = min(len(self.参照正本), self.政策.初期参照上限)
        self._索引再構築()
        self._計画再構築("SESSION_START")
        self._検査点("SESSION_START")

    @property
    def sessionID(self) -> str:
        return _hash("SES-", (self.問い, self.認知世界ID, HDS主体署名(self.主体状態)))

    @property
    def 主体署名(self) -> str:
        return HDS主体署名(self.主体状態)

    @property
    def 計画BindingID(self) -> str | None:
        if self.参照計画 is None:
            return None
        return _hash("RPB-", (self.参照計画.計画ID, self.計画候補状態署名, self.計画主体署名))

    @property
    def 候補状態(self) -> dict[str, float]:
        return dict(self.候補lane群[-1]) if self.候補lane群 else {}

    @property
    def 候補署名(self) -> str:
        return HDS候補状態署名(self.候補状態)

    @property
    def 正本revision(self) -> str:
        return self.索引.revision if self.索引 is not None else ""

    def _索引再構築(self) -> None:
        self.索引 = HDS参照索引圧縮(self.参照正本, bucket幅=self.政策.索引bucket幅)

    def _計画再構築(self, reason: str) -> None:
        if self.索引 is None:
            self._索引再構築()
        limit = min(len(self.参照正本), max(0, int(self.参照上限)))
        if limit <= 0 and self.参照正本:
            limit = min(len(self.参照正本), self.政策.初期参照上限)
        self.参照計画 = HDS参照計画作成(
            self.問い,
            self.参照正本,
            索引=self.索引,
            参照上限=limit,
            利用上限=self.政策.参照計画利用上限,
        )
        self.計画候補状態署名 = self.候補署名
        self.計画主体署名 = self.主体署名
        self.作用履歴.append(f"{HDS統一作用.参照計画再構築.value}:{reason}:{limit}")

    def _計画再利用可能(self) -> bool:
        plan = self.参照計画
        return bool(
            plan is not None
            and HDS参照計画再利用可能(plan, self.問い, self.参照正本)
            and self.計画候補状態署名 == self.候補署名
            and self.計画主体署名 == self.主体署名
        )

    def _検査点(self, stage: str, residuals: Iterable[str] = ()) -> None:
        plan = self.参照計画
        self.検査点.append(HDS統一検査点(
            self.cycle,
            str(stage),
            plan.計画ID if plan is not None else None,
            plan.残存利用回数 if plan is not None else 0,
            self.候補署名,
            self.主体署名,
            self.並列状態.状態ID if self.並列状態 is not None else None,
            tuple(dict.fromkeys(str(x) for x in residuals if str(x))),
        ))

    def 参照正本更新(self, references: Sequence[参照記録]) -> bool:
        new_records = tuple(references)
        before = self.正本revision
        self.参照正本 = new_records
        self._索引再構築()
        changed = before != self.正本revision
        if changed:
            if self.参照計画 is not None:
                self.参照計画 = HDS参照計画無効化(self.参照計画, "ARCHIVE_REVISION_CHANGED")
            self.参照上限 = min(len(new_records), max(self.参照上限, self.政策.初期参照上限))
            self._計画再構築("ARCHIVE_REVISION_CHANGED")
            self.cycle += 1
            self._検査点("ARCHIVE_REVISION_CHANGED")
        return changed

    def 主体状態更新(self, 主体: object | None) -> bool:
        before = self.主体署名
        self.主体状態 = 主体
        changed = before != self.主体署名
        if changed:
            if self.参照計画 is not None:
                self.参照計画 = HDS参照計画無効化(self.参照計画, '主体_状態_CHANGED')
            self._計画再構築('主体_状態_CHANGED')
            self.cycle += 1
            self._検査点('主体_状態_CHANGED')
        return changed

    def 選択参照(self) -> tuple[参照記録, ...]:
        if not self._計画再利用可能():
            self._計画再構築("PLAN_BINDING_CHANGED")
        assert self.参照計画 is not None
        selected = HDS参照計画適用(self.参照計画, self.参照正本)
        self.作用履歴.append(f"{HDS統一作用.参照計画再利用.value}:{self.参照計画.計画ID}:{len(selected)}")
        return selected

    def 計画消費(self, 使用回数: int = 1) -> None:
        if self.参照計画 is None:
            return
        for _ in range(max(0, int(使用回数))):
            self.参照計画 = HDS参照計画消費(self.参照計画)
        if self.参照計画 is not None and not self.参照計画.有効:
            self.作用履歴.append('取得_PLAN_LEASE_EXHAUSTED')

    def 参照拡張可能(self) -> bool:
        return self.参照上限 < len(self.参照正本)

    def 参照拡張(self, reason: str) -> bool:
        if not self.参照拡張可能():
            return False
        self.参照上限 = min(len(self.参照正本), self.参照上限 + self.政策.参照拡張幅)
        if self.参照計画 is not None:
            self.参照計画 = HDS参照計画無効化(self.参照計画, reason)
        self._計画再構築(reason)
        self.cycle += 1
        self._検査点('参照_WIDENED', (reason,))
        return True

    def 候補状態記録(self, values: Mapping[str, float] | Iterable[tuple[str, float]], *, stage: str) -> None:
        lane = {str(k): float(v) for k, v in dict(values).items()}
        if lane:
            self.候補lane群.append(lane)
            if len(self.候補lane群) > self.政策.最大候補lane数:
                self.候補lane群[:] = self.候補lane群[-self.政策.最大候補lane数:]
            self._並列状態再構築()
        self.cycle += 1
        self._検査点(stage)

    def _並列状態再構築(self) -> None:
        if not self.候補lane群:
            self.並列状態 = None
            return
        状態 = HDS並列作業状態生成(tuple(self.候補lane群))
        n = 状態.lane数
        self.並列状態 = HDS並列状態混合(状態, [[1.0 for _ in range(n)] for _ in range(n)])

    def lane不一致度(self) -> float:
        lanes = tuple(self.候補lane群)
        if len(lanes) <= 1:
            return 0.0
        tops = []
        for lane in lanes:
            if lane:
                tops.append(sorted(lane.items(), key=lambda kv: (-kv[1], kv[0]))[0][0])
        if len(set(tops)) > 1:
            return 1.0
        keys = sorted({k for lane in lanes for k in lane})
        if not keys:
            return 0.0
        max_diff = 0.0
        for key in keys:
            values = [float(lane.get(key, 0.0)) for lane in lanes]
            scale = max(1.0, max(abs(v) for v in values))
            max_diff = max(max_diff, (max(values) - min(values)) / scale)
        return min(1.0, max_diff)

    def 次作用(
        self,
        *,
        状態: str,
        出力存在: bool,
        理由: Iterable[str] = (),
        検査点利用可能: bool = False,
        専門作用利用可能: bool = False,
        主体競合: bool = False,
    ) -> HDS統一作用:
        reasons = tuple(dict.fromkeys(str(x) for x in 理由 if str(x)))
        joined = "\n".join(reasons)
        self.最終残差 = reasons
        if 状態 == "PROPOSE" and 出力存在 and not 主体競合:
            return HDS統一作用.J引渡し
        if 主体競合:
            return HDS統一作用.主体整合

        証拠_markers = ('証拠', '参照', "PROVENANCE", '資料_', "OBSERVATION", 'NO_候補')
        conflict_markers = ("AMBIGUOUS", "CONTRADICTION", "CONFLICT", "DISCRIMINATION")
        計算量_markers = ("DEPTH", '予算', "EXHAUST", "SEARCH", "INFERENCE")
        状態_delta_unconsumed = 'HDS_作用_DELTA_ATTACHED' in joined and 'HDS_作用_DELTA_CONSUMED' not in joined

        if self.lane不一致度() >= self.政策.不一致再照合閾値:
            return HDS統一作用.大域再照合
        if any(marker in joined for marker in conflict_markers):
            return HDS統一作用.大域再照合
        if any(marker in joined for marker in 証拠_markers):
            return HDS統一作用.参照計画再構築
        if 状態_delta_unconsumed and 検査点利用可能:
            return HDS統一作用.検査点再活性
        if 専門作用利用可能:
            return HDS統一作用.専門作用
        if any(marker in joined for marker in 計算量_markers):
            return HDS統一作用.計算量引上げ
        if self._計画再利用可能():
            return HDS統一作用.参照計画再利用
        return HDS統一作用.J留保

    def 作用記録(self, 作用: HDS統一作用, reasons: Iterable[str] = ()) -> None:
        self.作用履歴.append(作用.value)
        self.cycle += 1
        self._検査点(作用.value, reasons)

    def snapshot(self) -> HDS統一状態Snapshot:
        plan = self.参照計画
        return HDS統一状態Snapshot(
            self.sessionID,
            self.cycle,
            self.主体署名,
            self.正本revision,
            plan.計画ID if plan is not None else None,
            self.計画BindingID,
            self.参照上限,
            plan.残存利用回数 if plan is not None else 0,
            self.候補署名,
            len(self.候補lane群),
            self.lane不一致度(),
            len(self.検査点),
            tuple(self.作用履歴),
        )


def HDS結果候補得点(結果: object) -> dict[str, float]:
    模型 = getattr(結果, "MINIDORA模型結果", None)
    if 模型 is not None:
        mapping = getattr(模型, "候補辞書", None)
        if callable(mapping):
            try:
                return {str(k): float(v) for k, v in dict(mapping()).items()}
            except (TypeError, ValueError):
                pass
        rows = getattr(模型, "候補差", ())
        out: dict[str, float] = {}
        for row in rows:
            cid = getattr(row, "候補ID", None)
            score = getattr(row, "合計", getattr(row, "得点", None))
            if cid is not None and score is not None:
                out[str(cid)] = float(score)
        if out:
            return out
    label = getattr(結果, "回答ラベル", None)
    if label is not None:
        return {str(label): 1.0}
    return {}


__all__ = [
    "HDS統一作用",
    "HDS統一状態政策",
    'HDS統一検査点',
    "HDS統一状態Snapshot",
    "HDS統一状態Session",
    "HDS主体署名",
    "HDS候補状態署名",
    "HDS結果候補得点",
]

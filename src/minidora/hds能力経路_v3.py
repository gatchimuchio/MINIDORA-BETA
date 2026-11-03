from __future__ import annotations

from dataclasses import replace
from typing import Mapping, TYPE_CHECKING

from .HDS中間表現 import HDSIR
from .hds多時間尺度 import HDS多時間尺度政策, HDS阻害回復方針
from .hds能力経路_v2 import HDS能力経路V2候補提案実行
from .HDS統一状態循環 import (
    HDS統一作用,
    HDS統一状態Session,
    HDS統一状態政策,
    HDS結果候補得点,
)
if TYPE_CHECKING:
    from .K3機能 import K3相当能力核
from .模型 import MINIDORA模型核
from .参照 import 参照記録


def HDS能力経路V3候補提案実行(
    question_ir: HDSIR,
    references: tuple[参照記録, ...],
    *,
    コンパイル,
    基礎能力核: K3相当能力核 | None,
    候補意味IR: Mapping[str, HDSIR] | None = None,
    最大コンパイル並列: int = 4,
    模型核: MINIDORA模型核 | None = None,
    最大局所Window数: int = 12,
    政策: HDS多時間尺度政策 | None = None,
    統一政策: HDS統一状態政策 | None = None,
    統一session: HDS統一状態Session | None = None,
    主体状態: object | None = None,
):
    'V2の安全境界を保ったまま、構文化由来の統一状態循環を前段へ接続する。\n\n    - 取得 planを一回限り変数ではなくrequest-local sessionで保持する。\n    - archive/index/取得-plan/作業 状態/主体 snapshotを別寿命として扱う。\n    - 証拠不足・候補不一致時は参照上限を段階的に広げる。\n    - 各評価passの候補状態を意味役割を固定しないlaneとして保持する。\n    - K3 補助器は正式模型核が与えられる場合は必須ではない。\n    - J/HDSのCOMMIT/SUSPEND権限は変更しない。ここはPROPOSEまで。\n    '

    timescale = 政策 or HDS多時間尺度政策()
    unified_policy = 統一政策 or HDS統一状態政策(
        参照計画利用上限=timescale.参照計画利用上限,
        最大候補lane数=timescale.並列lane数,
    )
    query = question_ir.正規化文 or question_ir.原文
    session = 統一session or HDS統一状態Session(
        str(query),
        tuple(references),
        主体状態=主体状態,
        認知世界ID=str(getattr(question_ir, "認知世界ID", "") or ""),
        政策=unified_policy,
    )
    session.主体状態更新(主体状態)
    session.参照正本更新(tuple(references))

    結果 = None
    last_作用 = HDS統一作用.J留保
    attempts = 0

    for attempts in range(1, unified_policy.最大循環 + 1):
        実行系_references = session.選択参照()
        結果 = HDS能力経路V2候補提案実行(
            question_ir,
            tuple(実行系_references),
            コンパイル=コンパイル,
            基礎能力核=基礎能力核,
            候補意味IR=候補意味IR,
            最大コンパイル並列=最大コンパイル並列,
            模型核=模型核,
            最大局所Window数=最大局所Window数,
        )

        scores = HDS結果候補得点(結果)
        session.候補状態記録(scores, stage=f"EVALUATION_PASS_{attempts}")
        internal_uses = 1 + max(0, int(getattr(結果, "局所再照合数", 0)))
        internal_uses += max(0, int(getattr(結果, '検査点再活性数', 0)))
        session.計画消費(internal_uses)

        last_作用 = session.次作用(
            状態=str(getattr(結果, "状態", "SUSPEND")),
            出力存在=bool(getattr(結果, "回答ラベル", None) and getattr(結果, "回答内容", None)),
            理由=getattr(結果, "理由", ()),
            検査点利用可能=bool(getattr(結果, '検査点数', 0)),
            専門作用利用可能=False,
            主体競合=False,
        )
        session.作用記録(last_作用, getattr(結果, "理由", ()))

        if last_作用 == HDS統一作用.J引渡し:
            break

        if last_作用 in {HDS統一作用.参照計画再構築, HDS統一作用.大域再照合}:
            if session.参照拡張('UNIFIED_状態_RECONCILE'):
                continue

        # 同一状態・同一参照集合の決定論的再実行は行わない。
        break

    if 結果 is None:
        raise RuntimeError("HDS能力経路V3が評価を一度も実行しなかった")

    recovery = HDS阻害回復方針(getattr(結果, "理由", ())) if 結果.状態 != "PROPOSE" else None
    snap = session.snapshot()
    reasons = list(結果.理由)
    reasons.extend((
        'UNIFIED_状態_CYCLE_V1',
        'K3_GLM_LLAMA3_CROSS_模型_REDUCTION',
        'ARCHIVE_INDEX_PLAN_作業_主体_LIFETIMES_SEPARATED',
        f"UNIFIED_SESSION_ID:{snap.sessionID}",
        f"UNIFIED_EVALUATION_ATTEMPTS:{attempts}",
        f"UNIFIED_REFERENCE_LIMIT:{snap.参照上限}",
        f"UNIFIED_RETRIEVAL_PLAN_ID:{snap.参照計画ID or 'NONE'}",
        f"UNIFIED_RETRIEVAL_PLAN_BINDING_ID:{snap.参照計画BindingID or 'NONE'}",
        f"UNIFIED_RETRIEVAL_PLAN_LEASE_REMAINING:{snap.参照計画残存利用回数}",
        f"UNIFIED_PARALLEL_LANES:{snap.lane数}",
        f"UNIFIED_PARALLEL_DIVERGENCE:{snap.lane不一致度:.6f}",
        f"UNIFIED_LAST_ACTION:{last_作用.value}",
        f"LOCAL_GLOBAL_TIMESCALE:{timescale.局所更新回数}:{timescale.大域周期}",
    ))
    if recovery is not None:
        reasons.append("BLOCKER_RECOVERY_POLICY:" + recovery.作用)
        if recovery.参照計画無効化:
            reasons.append('取得_PLAN_INVALIDATION_ELIGIBLE')
        if recovery.計算量引上げ:
            reasons.append('計算量_ESCALATION_AVAILABLE')
        if recovery.Jへ留保:
            reasons.append("BLOCKER_DEFERRED_TO_J")

    return replace(結果, 理由=tuple(dict.fromkeys(reasons)))


__all__ = ["HDS能力経路V3候補提案実行"]

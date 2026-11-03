from __future__ import annotations

from dataclasses import replace

from .HDS中間表現 import HDSIR
from .hds統合判断主体 import HDS作用種別, MINIDORAHDS判断主体
from .HDS統一状態循環 import HDS統一状態Session, HDS統一状態政策
from .hds適応候補調停 import HDS適応候補提案実行
from .模型 import MINIDORA模型核
from .参照 import 参照記録


def HDS統一選択評価(
    question_ir: HDSIR,
    references: tuple[参照記録, ...],
    *,
    コンパイル,
    模型核: MINIDORA模型核,
    統一session: HDS統一状態Session,
    統一政策: HDS統一状態政策 | None = None,
    主体状態: object | None = None,
):
    'K3/GLM/Llama3由来の能力循環を通した後、J/HDSだけが最終採否する。\n\n    この関数が標準MINIDORAの選択評価入口になる。旧K3 補助器は要求しない。\n    '
    proposal = HDS適応候補提案実行(
        question_ir,
        tuple(references),
        コンパイル=コンパイル,
        基礎能力核=None,
        模型核=模型核,
        統一session=統一session,
        統一政策=統一政策,
        主体状態=主体状態,
    )

    judge = MINIDORAHDS判断主体()
    world = judge.開始(
        question_ir,
        参照利用可能=bool(references),
        参照必須=bool(getattr(question_ir, "参照必須", False)),
    )
    if references:
        world = judge.参照帰還(
            world,
            参照数=len(references),
            理由=('UNIFIED_参照_OBSERVED', f"REFERENCE_COUNT:{len(references)}"),
        )
    world = judge.評価帰還(world, proposal)
    request = judge.次作用(world)

    if request.作用 == HDS作用種別.確定:
        judge.確定(world)
        reasons = tuple(dict.fromkeys(tuple(proposal.理由) + request.理由 + (
            'UNIFIED_状態_TO_HDS_J',
            'HDS_JUDGEMENT_主体_COMMIT',
        )))
        return replace(proposal, 状態="APPROVE", 理由=reasons)

    if request.作用 == HDS作用種別.留保:
        judge.留保(world, request.理由)
        reasons = tuple(dict.fromkeys(tuple(proposal.理由) + request.理由 + (
            'UNIFIED_状態_TO_HDS_J',
            'HDS_JUDGEMENT_主体_SUSPEND',
        )))
        return replace(proposal, 状態="SUSPEND", 回答ラベル=None, 回答内容=None, 理由=reasons)

    reasons = tuple(dict.fromkeys(tuple(proposal.理由) + request.理由 + (
        'UNIFIED_J_UNEXPECTED_作用:' + request.作用.value,
        'HDS_JUDGEMENT_主体_SUSPEND',
    )))
    return replace(proposal, 状態="SUSPEND", 回答ラベル=None, 回答内容=None, 理由=reasons)


__all__ = ["HDS統一選択評価"]

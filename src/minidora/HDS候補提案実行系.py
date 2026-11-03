from __future__ import annotations

from dataclasses import dataclass
from typing import Mapping, Sequence, TYPE_CHECKING

from .HDS選択実行系 import (
    HDS選択実行結果,
    _choices,
    _一括コンパイル,
    _参照作用差分群,
    _専門作用起動数,
    _正式模型候補群,
    _独立コンパイル入口,
    _suspend,
)
from .HDS構文化記録_v1_3 import HDS作用差分構造
from .HDS中間表現 import HDSIR, 値状態
from .HDS候補意味正本 import HDS候補意味IR辞書
from .HDS模型射影 import (
    HDS内部言語状態,
    HDS能力作用構造射影,
    _対象言語体系,
    _文脈条件,
)
from .HDS実行系射影 import HDSK資料射影, HDSK候補射影, HDSK質問射影
from .hds判断参照境界 import HDS判断資料整列
if TYPE_CHECKING:
    from .K3機能 import K3相当能力核
from .模型 import MINIDORA模型核, 成立候補, 模型結果
from .能力状態差循環 import MINIDORA能力状態差模型核, 標準能力模型核
from .参照 import 参照記録
from .hds判断主体 import MINIDORA出力, MINIDORA出力化


_BLOCKING = {値状態.未確定, 値状態.未観測, 値状態.矛盾, 値状態.留保}


@dataclass(frozen=True, slots=True)
class MINIDORA候補提案結果:
    """計算主体C_execがJ_hdsへ返す候補提案。

    `PROPOSE` は採用ではない。候補生成系にはCOMMIT権限を与えない。
    """

    模型結果: 模型結果
    状態: str
    回答ラベル: str | None
    MINIDORA出力: MINIDORA出力
    理由: tuple[str, ...]


def MINIDORA候補提案評価(
    question_ir: HDSIR,
    候補_irs: Mapping[str, HDSIR],
    資料_irs: Sequence[HDSIR],
    *,
    模型核: MINIDORA模型核 | None = None,
    参照識別子: Sequence[str] | None = None,
    作用差分構造群: Sequence[HDS作用差分構造] = (),
) -> MINIDORA候補提案結果:
    """HDS判断を行わず、MINIDORA能力核の候補提案だけを形成する。"""

    模型核 = 模型核 or 標準能力模型核()
    target = _対象言語体系(question_ir)
    question = HDS内部言語状態(question_ir, 識別子="question", 言語体系=target)
    候補_internal = {
        str(label): HDS内部言語状態(
            ir,
            識別子='候補:' + str(label),
            言語体系=target,
        )
        for label, ir in sorted(候補_irs.items())
    }
    candidates = tuple(成立候補(label, 状態) for label, 状態 in 候補_internal.items())
    ids = tuple(参照識別子 or tuple(f"reference:{i}" for i in range(len(資料_irs))))
    if len(ids) != len(資料_irs):
        raise ValueError('参照識別子は資料 IRと同数である必要がある')
    ref_internal = tuple(
        HDS内部言語状態(
            ir,
            識別子=ids[i],
            言語体系=target,
            証拠境界=True,
        )
        for i, ir in enumerate(資料_irs)
    )

    ability_structures = tuple(HDS能力作用構造射影(item) for item in 作用差分構造群)
    if isinstance(模型核, MINIDORA能力状態差模型核):
        結果 = 模型核.評価言語状態(
            question,
            candidates,
            条件=_文脈条件(question_ir),
            参照状態=ref_internal,
            作用構造群=ability_structures,
        )
    else:
        結果 = 模型核.評価言語状態(
            question,
            candidates,
            条件=_文脈条件(question_ir),
            参照状態=ref_internal,
        )

    模型_output = MINIDORA出力化(結果)
    answer = 模型_output.候補ID if 模型_output.状態 == "OUTPUT" else None
    状態 = "PROPOSE" if answer is not None else "SUSPEND"
    reasons: list[str] = [
        'MINIDORA_候補_PROPOSAL_境界',
        '候補_GENERATION_HAS_NO_COMMIT_AUTHORITY',
        '能力_射影_V1',
        '能力_状態_DELTA_V1',
    ]
    if answer is None:
        reasons.append("MINIDORA_OUTPUT_ABSENT")
    if ability_structures:
        reasons.append('HDS_作用_DELTA_ATTACHED')
    if any(
        contribution.関係名.startswith("候補共同参照:状態差連結")
        for row in 結果.候補差
        for contribution in row.寄与
    ):
        reasons.append('HDS_作用_DELTA_CONSUMED')
    if 結果.統計.検査点再活性数:
        reasons.append('状態_DELTA_REACTION')
    if 結果.統計.候補横断更新数:
        reasons.append('状態_DELTA_CROSS_UPDATE')

    return MINIDORA候補提案結果(
        結果,
        状態,
        answer,
        模型_output,
        tuple(dict.fromkeys(reasons)),
    )


def HDS候補提案実行(
    question_ir: HDSIR,
    references: tuple[参照記録, ...],
    *,
    コンパイル,
    基礎能力核: K3相当能力核 | None,
    候補意味IR: Mapping[str, HDSIR] | None = None,
    最大コンパイル並列: int = 4,
    模型核: MINIDORA模型核 | None = None,
) -> HDS選択実行結果:
    '既存HDS 構文化器/能力核をworkerとして使い、候補をPROPOSEまで形成する。'

    choices = _choices(question_ir)
    if len(choices) < 2:
        return _suspend('HDS_選択肢_SET_INCOMPLETE')
    labels = [label for label, _, _ in choices]
    if len(set(labels)) != len(labels):
        return _suspend('HDS_選択肢_LABEL_DUPLICATE')
    if any(状態 in _BLOCKING for _, _, 状態 in choices):
        return _suspend('HDS_選択肢_UNRESOLVED')
    if any(残差.種別 == '意味_loss' for 残差 in question_ir.残差):
        return _suspend('HDS_QUESTION_意味_LOSS')

    compile_isolated, parallel_safe = _独立コンパイル入口(コンパイル)
    worker_count = (
        min(max(1, int(最大コンパイル並列)), max(1, len(choices), len(references)))
        if parallel_safe
        else 1
    )

    正本候補 = HDS候補意味IR辞書(候補意味IR, labels)
    if 正本候補 is None:
        選択肢_payloads = _一括コンパイル(
            compile_isolated,
            [content for _, content, _ in choices],
            parallel=parallel_safe,
            max_workers=worker_count,
        )
        候補_irs: dict[str, HDSIR] = {}
        for (label, _, _), compiled in zip(choices, 選択肢_payloads):
            if isinstance(compiled, Exception):
                return _suspend(
                    'HDS_選択肢_COMPILE_FAILED',
                    候補_count=len(候補_irs),
                    parallel=parallel_safe,
                    workers=worker_count,
                )
            if any(残差.種別 == '意味_loss' for 残差 in compiled.残差):
                return _suspend(
                    'HDS_選択肢_意味_LOSS',
                    候補_count=len(候補_irs) + 1,
                    parallel=parallel_safe,
                    workers=worker_count,
                )
            候補_irs[label] = compiled
    else:
        候補_irs = 正本候補
        for label in labels:
            compiled = 候補_irs[label]
            if any(残差.種別 == '意味_loss' for 残差 in compiled.残差):
                return _suspend(
                    'HDS_選択肢_意味_LOSS',
                    候補_count=len(候補_irs),
                    parallel=parallel_safe,
                    workers=worker_count,
                )

    k_question_ir = HDSK質問射影(question_ir)
    if any(残差.種別 == '意味_loss' for 残差 in k_question_ir.残差):
        return _suspend(
            'HDS_K_QUESTION_意味_LOSS',
            候補_count=len(候補_irs),
            parallel=parallel_safe,
            workers=worker_count,
        )

    k_候補_irs = {label: HDSK候補射影(候補_ir) for label, 候補_ir in 候補_irs.items()}
    formal_候補_irs = _正式模型候補群(k_question_ir, 候補_irs, k_候補_irs)

    資料_payloads = _一括コンパイル(
        compile_isolated,
        [record.内容 for record in references],
        parallel=parallel_safe,
        max_workers=worker_count,
    )
    資料_bundle = HDS判断資料整列(references, 資料_payloads, HDSK資料射影)
    資料_irs = list(資料_bundle.IR群)
    資料_compiled = len(資料_bundle.IR群)
    資料_failed = 資料_bundle.失敗数

    作用_structures, 作用_failed = _参照作用差分群(
        コンパイル,
        資料_bundle.成功記録群,
        parallel=parallel_safe,
        max_workers=worker_count,
    )
    attached_模型_模型核 = 模型核 or getattr(基礎能力核, '_minidora_模型_模型核', None)
    proposal = MINIDORA候補提案評価(
        k_question_ir,
        formal_候補_irs,
        tuple(資料_irs),
        模型核=attached_模型_模型核,
        参照識別子=資料_bundle.出典ID群,
        作用差分構造群=作用_structures,
    )
    選択肢_map = {label: content for label, content, _ in choices}
    content = 選択肢_map.get(proposal.回答ラベル) if proposal.回答ラベル is not None else None
    reasons = list(proposal.理由)
    reasons.append('FORMAL_模型_模型核_PROPOSAL_ONLY')
    if 資料_failed:
        reasons.append(f"DATA_COMPILE_PARTIAL:{資料_failed}")
    if 作用_failed:
        reasons.append(f"ACTION_DELTA_COMPILE_PARTIAL:{作用_failed}")
    stats = proposal.模型結果.統計
    specialist_count = _専門作用起動数(proposal.模型結果)

    return HDS選択実行結果(
        proposal.状態,
        proposal.回答ラベル,
        content,
        tuple(dict.fromkeys(reasons)),
        None,
        len(候補_irs),
        資料_compiled,
        資料_failed,
        0,
        0,
        0,
        parallel_safe,
        worker_count,
        0,
        0,
        0,
        0,
        len(proposal.模型結果.検査点),
        int(stats.検査点再活性数),
        int(stats.大域再照合数),
        int(stats.候補横断更新数),
        specialist_count,
        int(proposal.状態 != "PROPOSE"),
        0,
        0,
        0,
        0,
        0,
        0,
        proposal.模型結果,
    )


__all__ = [
    "MINIDORA候補提案結果",
    "MINIDORA候補提案評価",
    "HDS候補提案実行",
]

from __future__ import annotations

from dataclasses import replace
from typing import Callable, Mapping, TYPE_CHECKING

from .HDS選択実行系 import HDS選択実行結果, HDS選択推論実行
from .HDS中間表現 import HDSIR
from .hds能力経路_v3 import HDS能力経路V3候補提案実行
from .HDS統一状態循環 import HDS統一状態Session, HDS統一状態政策
if TYPE_CHECKING:
    from .K3機能 import K3相当能力核
from .模型 import MINIDORA模型核
from .参照 import 参照記録


HDS候補worker = Callable[[HDSIR, tuple[参照記録, ...]], HDS選択実行結果]


def _基礎提案化(結果: HDS選択実行結果) -> HDS選択実行結果:
    """既存workerのAPPROVEを採用権限なしのPROPOSEへ落とす。"""
    if 結果.状態 != "APPROVE" or 結果.回答ラベル is None or 結果.回答内容 is None:
        return 結果
    return replace(
        結果,
        状態="PROPOSE",
        理由=tuple(dict.fromkeys(tuple(結果.理由) + (
            "HDS_ADAPTIVE_BASE_SELECTED",
            '候補_GENERATION_HAS_NO_COMMIT_AUTHORITY',
        ))),
    )


def _能力経路優先可能(結果: HDS選択実行結果) -> bool:
    """raw候補横断更新ではなく、実観測変化または専門作用消費だけを根拠にする。"""
    if 結果.状態 != "PROPOSE" or 結果.回答ラベル is None or 結果.回答内容 is None:
        return False
    reasons = set(結果.理由)
    return bool(
        結果.専門作用起動数 > 0
        or 'HDS_作用_DELTA_CONSUMED' in reasons
        or "C_LOCAL_VIEW_RECHECK_SELECTED" in reasons
        or 'NEW_参照_状態_CONSUMED' in reasons
        or any(str(x).startswith("UNIFIED_EVALUATION_ATTEMPTS:") and not str(x).endswith(":1") for x in reasons)
    )


def HDS適応候補調停(
    能力提案: HDS選択実行結果,
    基礎提案: HDS選択実行結果,
) -> HDS選択実行結果:
    """観測状態の実変化に基づいて二つの候補workerを調停する。

    V3統一状態循環が参照集合を実際に変更して形成したPROPOSEも、
    `UNIFIED_EVALUATION_ATTEMPTS>1` として実観測変化に含める。COMMIT権限は持たない。
    """
    if _能力経路優先可能(能力提案):
        return replace(
            能力提案,
            理由=tuple(dict.fromkeys(tuple(能力提案.理由) + (
                "HDS_ADAPTIVE_PRIMARY_SELECTED",
                'OBSERVATION_状態_CHANGE_SUPPORTED',
            ))),
        )

    base = _基礎提案化(基礎提案)
    if base.状態 == "PROPOSE":
        return base

    reasons = tuple(dict.fromkeys(
        tuple(能力提案.理由)
        + tuple(基礎提案.理由)
        + (
            "HDS_ADAPTIVE_NO_COMMITTABLE_PROPOSAL",
            "PRIMARY_WITHOUT_OBSERVATION_CHANGE_NOT_COMMITTED",
            "PRIMARY_WITHOUT_SECOND_ORDER_SUPPORT_NOT_COMMITTED",
        )
    ))
    return replace(
        能力提案,
        状態="SUSPEND",
        回答ラベル=None,
        回答内容=None,
        理由=reasons,
    )


def HDS適応候補提案実行(
    question_ir: HDSIR,
    references: tuple[参照記録, ...],
    *,
    コンパイル,
    基礎能力核: K3相当能力核 | None = None,
    候補意味IR: Mapping[str, HDSIR] | None = None,
    模型核: MINIDORA模型核 | None = None,
    統一session: HDS統一状態Session | None = None,
    統一政策: HDS統一状態政策 | None = None,
    主体状態: object | None = None,
) -> HDS選択実行結果:
    '同一HDS-IR・同一資料で能力v3 workerと基礎workerを生成し、一般規則で調停する。\n\n    正式MINIDORA模型核が与えられる場合、旧K3 補助器は必須ではない。\n    '
    primary = HDS能力経路V3候補提案実行(
        question_ir,
        references,
        コンパイル=コンパイル,
        基礎能力核=基礎能力核,
        候補意味IR=候補意味IR,
        模型核=模型核,
        統一session=統一session,
        統一政策=統一政策,
        主体状態=主体状態,
    )
    base = HDS選択推論実行(
        question_ir,
        references,
        コンパイル=コンパイル,
        基礎能力核=基礎能力核,
        候補意味IR=候補意味IR,
        模型核=模型核,
        作業再作用=False,
        局所再照合=False,
    )
    return HDS適応候補調停(primary, base)


__all__ = [
    "HDS候補worker",
    "HDS適応候補調停",
    "HDS適応候補提案実行",
]

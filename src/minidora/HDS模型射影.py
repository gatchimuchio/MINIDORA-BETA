from __future__ import annotations

from dataclasses import dataclass
from typing import Mapping, Sequence

from .HDS構文化記録_v1_3 import HDS作用差分構造
from .HDS中間表現 import HDSIR, 値状態
from .意味字句 import 意味語
from .言語構造 import 言語関係構造, _意味集合
from .模型 import MINIDORA模型核, 成立候補, 言語状態, 模型結果
from .能力状態差循環 import (
    MINIDORA能力状態差模型核,
    標準能力模型核,
    能力作用構造,
    能力作用記録,
    能力状態差記録,
    能力後続利用記録,
)

_BLOCKING_ENDPOINT = {値状態.未確定, 値状態.未観測, 値状態.矛盾, 値状態.留保}
_範囲_KEYS = frozenset({"様相", "量化", '条件範囲', '範囲', "条件作用"})


def _条件値(関係, key):
    prefix = key + "="
    for raw in 関係.条件:
        value = str(raw)
        if value.startswith(prefix):
            return value[len(prefix):].strip()
    return ""


def _端点意味(ir, ids):
    coords = ir.座標辞書()
    out = set()
    for cid in ids:
        coord = coords.get(cid)
        if coord is None or coord.値状態 in _BLOCKING_ENDPOINT:
            continue
        out.update(_意味集合(coord.内容))
    return frozenset(out)


def _述語意味(ir, 関係):
    surface = _条件値(関係, "検索述語")
    if not surface:
        vals = [
            str(c.内容)
            for c in ir.座標
            if str(c.種別) == "関係.述語" and str(c.内容).strip()
        ]
        surface = " ".join(vals) or str(関係.種別)
    return 意味語(surface)


def _関係構造(ir, 関係):
    start = _端点意味(ir, 関係.始点)
    end = _端点意味(ir, 関係.終点)
    if not start and not end:
        return None
    極性 = _条件値(関係, "極性") != "否定"
    conditions = []
    seen = set()
    for raw in 関係.条件:
        key, sep, payload = str(raw).partition("=")
        if not sep or key.strip() not in _範囲_KEYS or not payload.strip():
            continue
        sem = 意味語(payload)
        sig = tuple(sorted(sem))
        if sem and sig not in seen:
            seen.add(sig)
            conditions.append(sem)
    return 言語関係構造(
        str(関係.種別),
        start,
        end,
        極性,
        tuple(conditions),
        _述語意味(ir, 関係),
    )


def _対象言語体系(ir):
    lang = str(getattr(ir, "入力言語", "en") or "en").casefold()
    return "自然言語:ja" if lang.startswith("ja") else "自然言語:en"


def _残差証拠境界(ir):
    情報源_blocked = any(str(item.種別) == '意味_loss' for item in ir.残差)
    impacted = set()
    for 残差 in ir.残差:
        impacted.update(str(x) for x in 残差.影響座標)
    return 情報源_blocked, frozenset(impacted)


def HDS内部言語状態(ir, *, 識別子="", 言語体系=None, 証拠境界=False):
    情報源_blocked, impacted = _残差証拠境界(ir) if 証拠境界 else (False, frozenset())
    relations = []
    coords = ir.座標辞書()
    blocked_endpoints = set()
    if 証拠境界:
        for 関係 in ir.関係:
            for cid in (*関係.始点, *関係.終点):
                coord = coords.get(cid)
                if coord is None or coord.値状態 in _BLOCKING_ENDPOINT:
                    blocked_endpoints.add(cid)
    for 関係 in ir.関係:
        if 情報源_blocked:
            continue
        if impacted and any(str(cid) in impacted for cid in (*関係.始点, *関係.終点)):
            continue
        if blocked_endpoints.intersection((*関係.始点, *関係.終点)):
            continue
        structure = _関係構造(ir, 関係)
        if structure is not None:
            relations.append(structure)
    ls = str(言語体系 or _対象言語体系(ir)).strip()
    return 言語状態(
        str(ir.正規化文 or ir.原文),
        ls,
        識別子,
        tuple(relations),
        # 棄却済みの原文を内部化時の再解析・語彙照合で証拠へ戻さない。
        # 原文は監査用に保持し、問題のない局所IRだけを継続利用する。
        表層再解析可=not (情報源_blocked or impacted or blocked_endpoints),
        証拠利用可=not 情報源_blocked,
    )


def _文脈条件(question_ir):
    out = []
    for 関係 in question_ir.関係:
        intent = _条件値(関係, "選択意図")
        if intent:
            out.append("選択意図=" + intent)
    for coord in question_ir.座標:
        if str(coord.種別) == "制御.選択意図" and str(coord.内容).strip():
            out.append("選択意図=" + str(coord.内容).strip())
    return tuple(dict.fromkeys(out))


def HDS能力作用構造射影(structure: HDS作用差分構造) -> 能力作用構造:
    'HDS 構文化器成果をMINIDORA能力核の日本語内部型へ有限射影する。'
    return 能力作用構造(
        作用=tuple(
            能力作用記録(
                item.作用ID,
                item.種別,
                item.入力状態,
                item.出力状態,
                tuple(item.条件),
            )
            for item in structure.作用
        ),
        状態差=tuple(
            能力状態差記録(
                item.差分ID,
                item.原因作用ID,
                item.前状態,
                item.後状態,
                item.変化有無,
            )
            for item in structure.状態差
        ),
        後続利用=tuple(
            能力後続利用記録(
                item.原因差分ID,
                item.成立状態,
                item.後続作用ID,
                tuple(item.追加条件),
                item.状態条件充足,
            )
            for item in structure.後続利用
        ),
    )


@dataclass(frozen=True, slots=True)
class HDSMINIDORA射影結果:
    """名称は互換維持。終端採否はMINIDORA能力核自身が形成する。"""

    模型結果: 模型結果
    状態: str
    回答ラベル: str | None
    理由: tuple[str, ...]
    HDS判断: object | None = None
    MINIDORA出力: object | None = None


def _能力核終端(結果: 模型結果) -> tuple[str, str | None, list[str]]:
    """後段HDSを使わず、能力核の参照由来差だけで通常MINIDORAを閉じる。"""
    answer = 結果.参照最有力候補ID
    if answer is not None:
        return (
            "APPROVE",
            answer,
            [
                'MINIDORA_模型_模型核_SELECTED',
                '参照_CONTRIBUTION_PRESENT',
                '参照_DIFFERENCE_SELECTED',
            ],
        )

    ref_scores = 結果.参照候補辞書()
    if not any(ref_scores.values()):
        return (
            "SUSPEND",
            None,
            ['MINIDORA_模型_模型核_NO_参照_CONTRIBUTION', "NO_GUESS"],
        )
    return (
        "SUSPEND",
        None,
        [
            'MINIDORA_模型_模型核_NO_UNIQUE_POSITIVE_DIFFERENCE',
            '参照_DIFFERENCE_NOT_UNIQUE',
        ],
    )


def HDSMINIDORA模型評価(
    question_ir: HDSIR,
    候補_irs: Mapping[str, HDSIR],
    資料_irs: Sequence[HDSIR],
    *,
    模型核: MINIDORA模型核 | None = None,
    判断主体: object | None = None,
    参照識別子: Sequence[str] | None = None,
    参照信頼: Sequence[float] | None = None,
    作用差分構造群: Sequence[HDS作用差分構造] = (),
):
    'HDS 構文化器成果をMINIDORA能力核へ渡し、通常MINIDORA自身で候補差を閉じる。\n\n    ``判断主体`` は旧API互換の受取口として残すがactive pathでは使用しない。\n    HDSの実体はこの能力評価内部には置かず、外側のHDS監督介入層だけに置く。\n    '
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
    if 参照信頼 is not None and len(tuple(参照信頼)) != len(資料_irs):
        raise ValueError('参照信頼は資料 IRと同数である必要がある')
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

    実行系_状態, answer, reasons = _能力核終端(結果)
    reasons.extend(('MINIDORA_能力_模型核_TERMINAL', '能力_射影_V1', '能力_状態_DELTA_V1'))
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

    return HDSMINIDORA射影結果(
        結果,
        実行系_状態,
        answer,
        tuple(dict.fromkeys(reasons)),
        None,
        None,
    )


__all__ = [
    "HDS内部言語状態",
    "HDS能力作用構造射影",
    "HDSMINIDORA射影結果",
    "HDSMINIDORA模型評価",
]

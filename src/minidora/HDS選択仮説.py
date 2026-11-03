from __future__ import annotations

from dataclasses import replace
from typing import Mapping

from .HDS中間表現 import HDSIR, HDS座標, HDS関係, 値状態


_BLOCKING = {値状態.未確定, 値状態.未観測, 値状態.矛盾, 値状態.留保}
_STRUCTURAL_KEYS = frozenset({
    "検索述語",
    "英日意味射影",
    "受動態",
    "選択意図",
    "様相",
    "量化",
    '条件範囲',
    '範囲',
    "条件作用",
    "極性",
})


def _条件値(関係: HDS関係, key: str) -> str:
    prefix = key + "="
    for raw in 関係.条件:
        value = str(raw)
        if value.startswith(prefix):
            return value[len(prefix):].strip()
    return ""


def _候補表層(ir: HDSIR) -> str:
    return " ".join(str(ir.正規化文 or ir.原文).split()).strip()


def _質問関係(question_ir: HDSIR) -> tuple[HDS関係, ...]:
    """英日意味正本関係があれば、表層由来の局所関係より優先する。"""
    canonical = tuple(
        関係
        for 関係 in question_ir.関係
        if _条件値(関係, "英日意味射影")
        and _条件値(関係, "不足位置") in {"始点", "終点"}
    )
    if canonical:
        return canonical
    return tuple(
        関係
        for 関係 in question_ir.関係
        if _条件値(関係, "不足位置") in {"始点", "終点"}
    )


def _仮説条件(関係: HDS関係, missing: str) -> tuple[str, ...]:
    '候補代入で問いの述語・極性・範囲を落とさない。'
    out = ["由来=候補代入", f"不足位置={missing}"]
    for raw in 関係.条件:
        key, sep, payload = str(raw).partition("=")
        key = key.strip()
        payload = payload.strip()
        if not sep or key == "不足位置" or key not in _STRUCTURAL_KEYS or not payload:
            continue
        out.append(f"{key}={payload}")
    return tuple(dict.fromkeys(out))


def HDS候補代入仮説(question_ir: HDSIR, label: str, 候補_ir: HDSIR) -> HDSIR:
    """問いの未知関係端点へ候補を代入した比較専用仮説を候補IRへ追加する。

    仮説は `推定` のまま候補IRにだけ存在し、canonical Kへは昇格しない。
    """
    候補 = _候補表層(候補_ir)
    if not 候補:
        return 候補_ir

    qcoords = question_ir.座標辞書()
    coords = list(候補_ir.座標)
    relations = list(候補_ir.関係)
    existing_ids = {coord.座標ID for coord in coords}
    existing_関係_ids = {関係.関係ID for 関係 in relations}
    added = 0

    def add_coord(kind: str, content: str, suffix: str) -> str:
        base = f"hyp:{label}:{suffix}"
        cid = base
        serial = 1
        while cid in existing_ids:
            cid = f"{base}:{serial}"
            serial += 1
        existing_ids.add(cid)
        coords.append(
            HDS座標(
                cid,
                kind,
                content,
                値状態.推定,
                由来="HDS候補代入仮説",
                暫定性='候補_SUBSTITUTION_HYPOTHESIS',
            )
        )
        return cid

    for qrelation in _質問関係(question_ir):
        missing = _条件値(qrelation, "不足位置")
        if missing not in {"始点", "終点"}:
            continue

        known_starts = [
            qcoords[cid]
            for cid in qrelation.始点
            if cid in qcoords and qcoords[cid].値状態 not in _BLOCKING
        ]
        known_ends = [
            qcoords[cid]
            for cid in qrelation.終点
            if cid in qcoords and qcoords[cid].値状態 not in _BLOCKING
        ]

        if missing == "終点":
            if not known_starts:
                continue
            pairs = ((known, None) for known in known_starts)
        else:
            if not known_ends:
                continue
            pairs = ((None, known) for known in known_ends)

        for index, (known_start, known_end) in enumerate(pairs):
            if missing == "終点":
                assert known_start is not None
                sid = add_coord("対象.仮説既知端点", str(known_start.内容), f"known-start:{added}:{index}")
                oid = add_coord("目的.候補代入", 候補, f"candidate-end:{added}:{index}")
            else:
                assert known_end is not None
                sid = add_coord("目的.候補代入", 候補, f"candidate-start:{added}:{index}")
                oid = add_coord("対象.仮説既知端点", str(known_end.内容), f"known-end:{added}:{index}")

            rid_base = f"hyp-rel:{label}:{added}:{index}"
            rid = rid_base
            serial = 1
            while rid in existing_関係_ids:
                rid = f"{rid_base}:{serial}"
                serial += 1
            existing_関係_ids.add(rid)
            relations.append(
                HDS関係(
                    rid,
                    (sid,),
                    (oid,),
                    str(qrelation.種別),
                    条件=_仮説条件(qrelation, missing),
                    値状態=値状態.推定,
                    由来="HDS候補代入仮説",
                    暫定性='候補_SUBSTITUTION_HYPOTHESIS',
                )
            )
            added += 1

    if not added:
        return 候補_ir
    return replace(候補_ir, 座標=tuple(coords), 関係=tuple(relations))


def HDS候補代入仮説群(question_ir: HDSIR, candidates: Mapping[str, HDSIR]) -> dict[str, HDSIR]:
    return {
        str(label): HDS候補代入仮説(question_ir, str(label), 候補_ir)
        for label, 候補_ir in candidates.items()
    }


__all__ = ["HDS候補代入仮説", "HDS候補代入仮説群"]

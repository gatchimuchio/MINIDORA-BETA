from __future__ import annotations

from dataclasses import replace
import re

from .HDS中間表現 import HDSIR, HDS座標, HDS関係, 値状態
from .意味字句 import 意味語
from .言語基底 import 言語基底P, 標準言語基底P
from .言語基底_英語 import 英語関係一致
from .言語基底_英日意味強化 import 英語明示述語関係抽出


_BLOCKING = {値状態.未確定, 値状態.未観測, 値状態.矛盾, 値状態.留保}
_QUESTION_START = re.compile(r"^\s*(?:which|what|who|where|when|why|how)\b", re.I)
_DERIVED_ACTIVE_FALSE = re.compile(r"\b(?:is|are|was|were)\s*$", re.I)


def _norm(value: object) -> str:
    return " ".join(str(value).split()).strip(" ,;:。！？?.").casefold()


def _条件値(関係: HDS関係, key: str) -> str:
    prefix = key + "="
    for raw in 関係.条件:
        value = str(raw)
        if value.startswith(prefix):
            return value[len(prefix):].strip()
    return ""


def _関係署名(kind: str, 主体: object, object_: object, predicate: str = "", conditions: tuple[str, ...] = ()) -> tuple[object, ...]:
    condition_map: dict[str, str] = {}
    for raw in conditions:
        key, sep, payload = str(raw).partition("=")
        if sep:
            condition_map[key.strip()] = payload.strip()
    極性 = condition_map.get("極性", "肯定") or "肯定"
    範囲 = tuple(
        (key, condition_map[key])
        for key in ("様相", "量化", '条件範囲', '範囲', "条件作用")
        if condition_map.get(key)
    )
    predicate_key = _norm(predicate) if str(kind) == "開放述語" else ""
    return (str(kind), _norm(主体), _norm(object_), predicate_key, 極性, 範囲)


def _existing_signatures(ir: HDSIR) -> set[tuple[object, ...]]:
    coords = ir.座標辞書()
    out: set[tuple[object, ...]] = set()
    for 関係 in ir.関係:
        if 関係.値状態 in _BLOCKING:
            continue
        starts = [coords[cid] for cid in 関係.始点 if cid in coords and coords[cid].値状態 not in _BLOCKING]
        ends = [coords[cid] for cid in 関係.終点 if cid in coords and coords[cid].値状態 not in _BLOCKING]
        for start in starts:
            for end in ends:
                predicate = _条件値(関係, "検索述語")
                out.add(_関係署名(str(関係.種別), start.内容, end.内容, predicate, tuple(str(x) for x in 関係.条件)))
    return out


def HDS英語基底関係射影(ir: HDSIR, 言語基底: 言語基底P | None = None) -> HDSIR:
    """英語に表層明示された関係を、有限関係族＋開放述語としてHDSへ補完する。"""
    言語 = str(getattr(ir, "入力言語", "") or "").casefold()
    if not 言語.startswith("en"):
        return ir

    text = str(ir.正規化文 or ir.原文)
    if "?" in text or _QUESTION_START.search(text):
        return ir

    言語_p = 言語基底 or 標準言語基底P
    syntaxes = 言語_p.英語関係構文()
    coords = list(ir.座標)
    relations = list(ir.関係)
    existing_ids = {coord.座標ID for coord in coords}
    existing_関係_ids = {関係.関係ID for 関係 in relations}
    signatures = _existing_signatures(ir)
    added = 0

    def add_coord(kind: str, content: str, suffix: str) -> str:
        base = f"language:{suffix}:{added}"
        cid = base
        serial = 1
        while cid in existing_ids:
            cid = f"{base}:{serial}"
            serial += 1
        existing_ids.add(cid)
        coords.append(HDS座標(cid, kind, content, 値状態.確定, 由来="共有言語基底P"))
        return cid

    def 関係を追加(kind: str, 主体: str, object_: str, predicate: str, *, conditions: tuple[str, ...] = ()) -> None:
        nonlocal added
        主体 = " ".join(主体.split()).strip(" ,;:()[]")
        object_ = " ".join(object_.split()).strip(" ,;:()[]")
        predicate = " ".join(predicate.split()).strip()
        if not 主体 or not object_ or not predicate:
            return
        if not 意味語(主体) or not 意味語(object_):
            return
        if _norm(主体) == _norm(object_):
            return
        signature = _関係署名(kind, 主体, object_, predicate, conditions)
        if signature in signatures:
            return
        sid = add_coord("対象.始点", 主体, '主体')
        oid = add_coord("対象.終点", object_, "object")
        rid_base = f"language-rel:{added}"
        rid = rid_base
        serial = 1
        while rid in existing_関係_ids:
            rid = f"{rid_base}:{serial}"
            serial += 1
        existing_関係_ids.add(rid)
        関係_conditions = [f"検索述語={predicate}", "由来=共有言語基底P"]
        関係_conditions.extend(conditions)
        relations.append(
            HDS関係(
                rid,
                (sid,),
                (oid,),
                kind,
                条件=tuple(dict.fromkeys(関係_conditions)),
                値状態=値状態.確定,
                由来="共有言語基底P",
            )
        )
        signatures.add(signature)
        added += 1

    for syntax in syntaxes:
        for match in 英語関係一致(syntax, text):
            主体 = " ".join(match.group("s").split()).strip(" ,;:()[]")
            object_ = " ".join(match.group("o").split()).strip(" ,;:()[]")
            predicate = " ".join(match.group("v").split()).strip()
            if not 主体 or not object_ or not predicate:
                continue
            if not syntax.反転 and object_.casefold().startswith("by "):
                continue
            if predicate.casefold() == "derived from" and _DERIVED_ACTIVE_FALSE.search(主体):
                continue
            if syntax.反転:
                主体, object_ = object_, 主体
            関係を追加(syntax.種別, 主体, object_, predicate)

    for item in 英語明示述語関係抽出(text):
        # 既存17関係族は上の共有言語基底P構文が正本。第二passは有限語彙外だけを補完する。
        if item.種別 != "開放述語":
            continue
        conditions = []
        if item.極性 == "否定":
            conditions.append("極性=否定")
        conditions.extend(f"{key}={value}" for key, value in item.修飾)
        関係を追加(item.種別, item.始点, item.終点, item.検索述語, conditions=tuple(conditions))

    if not added:
        return ir
    return replace(ir, 座標=tuple(coords), 関係=tuple(relations))


__all__ = ["HDS英語基底関係射影"]

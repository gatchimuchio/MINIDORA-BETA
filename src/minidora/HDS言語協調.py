from __future__ import annotations

from dataclasses import replace
import re

from .HDS中間表現 import HDSIR, HDS座標, HDS関係, 値状態


_VERSION = "v0.9-clean"
_GENERIC = {"意味原子→節", "談話順序", "節→述語", "候補→集合", "問い×候補→選択目的", "共参照", "数量単位"}
_AND = re.compile(r"\s+and\s+", re.I)


def _norm(value: object) -> str:
    return " ".join(str(value).split()).strip(" ,;:()[]")


def _単純AND(value: object) -> tuple[str, str] | None:
    text = _norm(value)
    parts = _AND.split(text)
    if len(parts) != 2:
        return None
    left, right = (_norm(parts[0]), _norm(parts[1]))
    if not left or not right or left.casefold() == right.casefold():
        return None
    lt, rt = left.split(), right.split()
    if len(lt) > 6 or len(rt) > 6:
        return None
    # 高確度な単純並列だけ。単語同士、または共通headを明示する `Protein A and Protein B` 型。
    if len(lt) == 1 and len(rt) == 1:
        return left, right
    if len(lt) >= 2 and len(rt) >= 2 and lt[0].casefold() == rt[0].casefold():
        return left, right
    return None


def HDS英語AND展開(ir: HDSIR) -> HDSIR:
    """明示ANDで結ばれた単純な関係端点だけを複数の有向関係へ展開する。

    ORは論理的に両方成立とは限らないため扱わない。未知端点・複雑な名詞句・長いcoordinationも
    推測せずそのまま残す。
    """
    言語 = str(getattr(ir, "入力言語", "") or "").casefold()
    if not 言語.startswith("en"):
        return ir

    coords = list(ir.座標)
    coord_map = ir.座標辞書()
    relations: list[HDS関係] = []
    existing_ids = {coord.座標ID for coord in coords}
    expanded = 0

    def add_coord(kind: str, content: str, base: str) -> str:
        for coord in coords:
            if str(coord.種別) == kind and _norm(coord.内容).casefold() == _norm(content).casefold() and coord.値状態 == 値状態.確定:
                return coord.座標ID
        cid = base
        serial = 1
        while cid in existing_ids:
            cid = f"{base}:{serial}"
            serial += 1
        existing_ids.add(cid)
        coords.append(HDS座標(cid, kind, content, 値状態.確定, 由来="共有言語基底P", 暫定性="EXPLICIT_AND_COORDINATION"))
        return cid

    for 関係 in ir.関係:
        if 関係.値状態 != 値状態.確定 or str(関係.種別) in _GENERIC:
            relations.append(関係)
            continue
        starts = [coord_map[cid] for cid in 関係.始点 if cid in coord_map]
        ends = [coord_map[cid] for cid in 関係.終点 if cid in coord_map]
        if len(starts) != 1 or len(ends) != 1:
            relations.append(関係)
            continue
        start, end = starts[0], ends[0]
        if start.値状態 != 値状態.確定 or end.値状態 != 値状態.確定:
            relations.append(関係)
            continue

        split_start = _単純AND(start.内容)
        split_end = _単純AND(end.内容)
        if split_start is None and split_end is None:
            relations.append(関係)
            continue

        start_values = split_start or (_norm(start.内容),)
        end_values = split_end or (_norm(end.内容),)
        local = 0
        for sindex, svalue in enumerate(start_values):
            sid = add_coord("対象.始点", svalue, f"lang-and:start:{expanded}:{sindex}") if split_start else start.座標ID
            for oindex, ovalue in enumerate(end_values):
                oid = add_coord("対象.終点", ovalue, f"lang-and:end:{expanded}:{oindex}") if split_end else end.座標ID
                conditions = tuple(dict.fromkeys((*関係.条件, f"AND展開={_VERSION}")))
                relations.append(
                    HDS関係(
                        f"lang-and:relation:{expanded}:{local}",
                        (sid,),
                        (oid,),
                        str(関係.種別),
                        条件=conditions,
                        値状態=関係.値状態,
                        由来="共有言語基底P",
                        暫定性="EXPLICIT_AND_COORDINATION",
                    )
                )
                local += 1
        expanded += 1

    if not expanded:
        return ir
    return replace(ir, 座標=tuple(coords), 関係=tuple(relations))


__all__ = ["HDS英語AND展開"]

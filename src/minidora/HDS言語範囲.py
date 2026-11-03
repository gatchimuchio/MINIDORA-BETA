from __future__ import annotations

from dataclasses import replace
import re
import unicodedata

from .HDS中間表現 import HDSIR, HDS座標, HDS関係


_MODAL = {
    "can": "可能",
    "could": "可能",
    "may": "可能",
    "might": "可能",
    "would": "可能",
    "must": "必要",
    "should": "必要",
}
_AUX_TAIL = re.compile(
    r"^(?P<base>.+?)\s+"
    r"(?:(?P<do>does|do|did)\s+(?P<do_not>not)|"
    r"(?P<modal>can|could|may|might|would|must|should)(?:\s+(?P<modal_not>not))?(?:\s+be)?|"
    r"(?P<be>is|are|was|were)\s+(?P<be_not>not))$",
    re.I,
)


def _norm(value: object) -> str:
    return " ".join(unicodedata.normalize("NFKC", str(value)).split()).strip(" ,;:。！？?.")


def _条件値(関係: HDS関係, key: str) -> str:
    prefix = key + "="
    for raw in 関係.条件:
        value = str(raw)
        if value.startswith(prefix):
            return value[len(prefix):].strip()
    return ""


def _条件追加(関係: HDS関係, *items: str) -> HDS関係:
    conditions = list(関係.条件)
    for item in items:
        if item and item not in conditions:
            conditions.append(item)
    return replace(関係, 条件=tuple(conditions))


def _端点範囲(content: str) -> tuple[str, tuple[str, ...]]:
    """関係regexが実体へ吸収した助動/否定だけを実体から分離する。"""
    value = _norm(content)
    match = _AUX_TAIL.fullmatch(value)
    if match is None:
        return value, ()

    base = _norm(match.group("base"))
    if not base:
        return value, ()

    conditions: list[str] = []
    if match.group("do_not") or match.group("modal_not") or match.group("be_not"):
        conditions.append("極性=否定")
    modal_surface = (match.group("modal") or "").casefold()
    if modal_surface:
        modal = _MODAL.get(modal_surface)
        if modal:
            conditions.extend((f"様相={modal}", f"様相表層={modal_surface}"))
    return base, tuple(conditions)


def _条件範囲(text: str, start: str, end: str, predicate: str) -> tuple[str, ...]:
    """同じ文の関係節より前にある明示条件句だけを関係へ結ぶ。"""
    if not start or not end or not predicate:
        return ()
    start_p = re.escape(start)
    end_p = re.escape(end)
    pred_head = re.escape(predicate.split()[0])
    関係 = re.search(
        rf"\b{start_p}\b[^.;!?]{{0,120}}\b{pred_head}(?:s|ed|ing)?\b[^.;!?]{{0,160}}\b{end_p}\b",
        text,
        re.I,
    )
    if 関係 is None:
        return ()
    prefix = text[: 関係.start()]
    condition = re.search(
        r"(?:^|[.;]\s*)(?P<c>(?:if|when|under|given|assuming)\b[^.;,]{1,180})\s*,\s*$",
        prefix,
        re.I,
    )
    if condition is None:
        return ()
    return ('条件範囲=' + _norm(condition.group("c")),)


def HDS英語関係範囲射影(ir: HDSIR) -> HDSIR:
    '英語関係の実体端点と作用範囲を構文化器内で分離・結合する。\n\n    新しい世界関係は作らない。既存関係の汚染端点を正規化し、明示された極性・様相・\n    条件を関係.条件へ移す。汚染された元端点は完全IRから消さず `表層.端点原形` へ\n    降格し、意味対象として下流へ再流入しないようにする。実行系は自然言語を再解析しない。\n    疑問文は質問意味構文化器へ委ねる。\n    '
    if not str(getattr(ir, "入力言語", "")).casefold().startswith("en"):
        return ir
    raw = str(ir.正規化文 or ir.原文)
    if not raw or "?" in raw:
        return ir
    text = _norm(raw)

    coords = list(ir.座標)
    coord_map = ir.座標辞書()
    existing = {(str(c.種別), _norm(c.内容)): c.座標ID for c in coords}
    counter = 0
    demote_ids: set[str] = set()

    def endpoint_coord(original_id: str, content: str) -> str:
        nonlocal counter
        original = coord_map.get(original_id)
        if original is None:
            return original_id
        normalized = _norm(content)
        if normalized == _norm(original.内容):
            return original_id

        # 元端点は観測表層として保持するが、正本の意味対象からは外す。
        demote_ids.add(original_id)
        key = (str(original.種別), normalized)
        if key in existing:
            return existing[key]
        cid = f"scope:endpoint:{counter}"
        counter += 1
        while any(c.座標ID == cid for c in coords):
            cid = f"scope:endpoint:{counter}"
            counter += 1
        coords.append(
            HDS座標(
                cid,
                str(original.種別),
                normalized,
                original.値状態,
                原文範囲=original.原文範囲,
                由来='公開HDS 構文化器.範囲分離',
                暫定性=original.暫定性,
                再開放条件=original.再開放条件,
            )
        )
        existing[key] = cid
        return cid

    changed = False
    relations: list[HDS関係] = []
    for 関係 in ir.関係:
        if len(関係.始点) != 1 or len(関係.終点) != 1:
            relations.append(関係)
            continue
        sid, oid = 関係.始点[0], 関係.終点[0]
        start_coord = coord_map.get(sid)
        end_coord = coord_map.get(oid)
        if start_coord is None or end_coord is None:
            relations.append(関係)
            continue

        start, start_範囲 = _端点範囲(str(start_coord.内容))
        end, end_範囲 = _端点範囲(str(end_coord.内容))
        範囲 = tuple(dict.fromkeys((*start_範囲, *end_範囲)))
        predicate = _条件値(関係, "検索述語")
        範囲 = tuple(dict.fromkeys((*範囲, *_条件範囲(text, start, end, predicate))))

        new_sid = endpoint_coord(sid, start)
        new_oid = endpoint_coord(oid, end)
        updated = 関係
        if (new_sid, new_oid) != (sid, oid):
            updated = replace(updated, 始点=(new_sid,), 終点=(new_oid,))
        if 範囲:
            updated = _条件追加(updated, *範囲, '範囲結合=構文化器')
        changed = changed or updated != 関係
        relations.append(updated)

    if demote_ids:
        coords = [
            replace(
                coord,
                種別="表層.端点原形",
                由来='公開HDS 構文化器.範囲分離',
                暫定性='SURFACE_ENDPOINT_BEFORE_範囲_SEPARATION',
            )
            if coord.座標ID in demote_ids
            else coord
            for coord in coords
        ]
        changed = True

    if not changed:
        return ir
    return replace(ir, 座標=tuple(coords), 関係=tuple(relations))


__all__ = ['HDS英語関係範囲射影']

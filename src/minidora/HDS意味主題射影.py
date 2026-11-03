from __future__ import annotations

from dataclasses import replace

from .HDS中間表現 import HDSIR, HDS座標, 値状態
from .意味字句 import 意味語


_意味所有役割 = (
    "対象.",
    "実体.",
    "関係.",
    "条件.",
    "状態.",
    "属性.",
    "値.",
)


def HDS問い主題射影(ir: HDSIR, *, 上限: int = 48) -> HDSIR:
    '`目的.検索焦点` が意味主題の生成を抑制しないよう役割を分離する。\n\n    検索焦点はR/J向けの目的座標であり、そこに語が存在するだけでは「その語の意味役割が\n    既に表現済み」とはみなさない。一方、対象・関係・条件・状態・属性・値として既に\n    構造化された語は主題語へ重複させない。\n\n    ここでは検索表層をKへ転用するのではなく、構文化器内部の焦点から未所有の意味語を\n    `対象.主題語` へ別役割として射影する。元の検索焦点も完全IRに保持する。\n    '
    limit = max(0, int(上限))
    if limit <= 0:
        return ir

    focuses = tuple(
        coord
        for coord in ir.座標
        if str(coord.種別) == "目的.検索焦点"
        and coord.値状態 not in {値状態.未確定, 値状態.未観測, 値状態.矛盾, 値状態.留保}
        and str(coord.内容).strip()
    )
    if not focuses:
        return ir

    owned: set[str] = set()
    existing_topics: set[str] = set()
    for coord in ir.座標:
        kind = str(coord.種別)
        if kind == "対象.主題語":
            existing_topics.update(意味語(coord.内容))
            continue
        if kind.startswith(_意味所有役割):
            owned.update(意味語(coord.内容))

    candidates: list[str] = []
    seen: set[str] = set(existing_topics)
    for focus in focuses:
        for term in sorted(意味語(focus.内容)):
            key = str(term).casefold()
            if not term or key in seen or term in owned:
                continue
            seen.add(key)
            candidates.append(str(term))
            if len(candidates) >= limit:
                break
        if len(candidates) >= limit:
            break

    if not candidates:
        return ir

    coords = list(ir.座標)
    existing_ids = {coord.座標ID for coord in coords}
    for index, term in enumerate(candidates):
        base = f"semantic-topic:{index}"
        cid = base
        serial = 1
        while cid in existing_ids:
            cid = f"{base}:{serial}"
            serial += 1
        existing_ids.add(cid)
        coords.append(
            HDS座標(
                cid,
                "対象.主題語",
                term,
                値状態.確定,
                由来='公開HDS 構文化器',
                暫定性='意味_TOPIC_役割_SEPARATION',
            )
        )

    return replace(ir, 座標=tuple(coords))


__all__ = ["HDS問い主題射影"]

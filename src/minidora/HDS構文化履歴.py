from __future__ import annotations

from dataclasses import replace
from hashlib import sha256

from .HDS構文化記録 import HDS_構文化器_META_PREFIXES
from .HDS構文化記録_v1_1 import HDS認知世界差分
from .HDS中間表現 import HDSIR, HDS座標, 値状態


def _meta(kind: str) -> bool:
    return str(kind).startswith(HDS_構文化器_META_PREFIXES)


def _coord_signature(ir: HDSIR) -> tuple[str, ...]:
    values: list[str] = []
    for coord in ir.座標:
        if _meta(str(coord.種別)):
            continue
        values.append(f"{coord.種別}={coord.内容}|{coord.値状態.value}")
    return tuple(sorted(dict.fromkeys(values)))


def _関係署名(ir: HDSIR) -> tuple[str, ...]:
    coords = ir.座標辞書()
    values: list[str] = []
    for 関係 in ir.関係:
        starts = tuple(str(coords[cid].内容) for cid in 関係.始点 if cid in coords)
        ends = tuple(str(coords[cid].内容) for cid in 関係.終点 if cid in coords)
        values.append(f"{関係.種別}:{starts}->{ends}|{tuple(関係.条件)}|{関係.値状態.value}")
    return tuple(sorted(dict.fromkeys(values)))


def HDS認知世界署名(ir: HDSIR) -> str:
    payload = "\n".join((*_coord_signature(ir), "--relations--", *_関係署名(ir)))
    return "cw:" + sha256(payload.encode("utf-8")).hexdigest()[:16]


def HDS認知世界差分生成(current: HDSIR, history: tuple[HDSIR, ...]) -> HDS認知世界差分:
    current_ref = HDS認知世界署名(current)
    if not history:
        return HDS認知世界差分(None, current_ref, 追加座標=_coord_signature(current), 旧世界保持=True)

    previous = history[-1]
    previous_ref = HDS認知世界署名(previous)
    current_coords = set(_coord_signature(current))
    previous_coords = set(_coord_signature(previous))
    current_rel = set(_関係署名(current))
    previous_rel = set(_関係署名(previous))

    added = tuple(sorted(current_coords - previous_coords))
    removed = tuple(sorted(previous_coords - current_coords))
    changed_rel = tuple(sorted((current_rel - previous_rel) | (previous_rel - current_rel)))
    reinterpretation: list[str] = []
    if removed:
        reinterpretation.append("旧座標の消失を削除ではなく再解釈候補として保持する")
    if changed_rel:
        reinterpretation.append("関係変更が主体・対象・因果・射程へ与える影響を再監査する")
    if added:
        reinterpretation.append("新座標が旧Recordの意味を変えるか監査する")

    return HDS認知世界差分(previous_ref, current_ref, added, removed, changed_rel, tuple(reinterpretation), True)


def HDS認知世界差分IR射影(ir: HDSIR, diff: HDS認知世界差分) -> HDSIR:
    coords = list(ir.座標)
    existing = {(str(coord.種別), str(coord.内容)) for coord in coords}

    def add(kind: str, content: str, 状態: 値状態 = 値状態.推定) -> None:
        key = (kind, content)
        if key in existing:
            return
        coords.append(HDS座標(f"archv11:history:{len(coords):03d}", kind, content, 状態, 由来='公開HDS 構文化器 v1.1', 再開放条件=("次観測・時点変更・版変更で再評価する",)))
        existing.add(key)

    if diff.前回世界参照:
        add("帰還.前回CognitiveWorld", diff.前回世界参照)
    if diff.現行世界参照:
        add("帰還.現行CognitiveWorld", diff.現行世界参照)
    for value in diff.再解釈要求:
        add("帰還.再解釈要求", value, 値状態.留保)
    return replace(ir, 座標=tuple(coords))


__all__ = ["HDS認知世界署名", "HDS認知世界差分生成", "HDS認知世界差分IR射影"]

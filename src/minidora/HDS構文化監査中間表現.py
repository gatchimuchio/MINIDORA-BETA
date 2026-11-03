from __future__ import annotations

from dataclasses import replace

from .HDS構文化記録_v1_1 import HDS監査参照候補
from .HDS中間表現 import HDSIR, HDS座標, 値状態


def HDS監査参照IR射影(ir: HDSIR, candidates: tuple[HDS監査参照候補, ...]) -> HDSIR:
    if not candidates:
        return ir
    coords = list(ir.座標)
    existing = {(str(coord.種別), str(coord.内容)) for coord in coords}
    for 候補 in candidates:
        key = ("監査.R_query", 候補.問合せ)
        if key in existing:
            continue
        coords.append(
            HDS座標(
                f"archv11:audit-r:{len(coords):03d}",
                "監査.R_query",
                候補.問合せ,
                値状態.留保,
                由来='公開HDS 構文化器 v1.1',
                再開放条件=('主検索結果・証拠取得・関門状態更新で再評価する',),
            )
        )
        existing.add(key)
    return replace(ir, 座標=tuple(coords))


__all__ = ["HDS監査参照IR射影"]

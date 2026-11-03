from __future__ import annotations

from dataclasses import replace

from .HDS中間表現 import HDSIR
from .命令計算降下 import 命令計算降下
from .計算中間表現 import 計算中間表現


def HDS計算降下(ir: HDSIR) -> 計算中間表現:
    '現行HDS意味IRを計算中間表現へ降下する互換境界。\n\n    自然言語を再解析しない。現行HDS-IRで明示的に閉包済みの ``手順`` と\n    ``実行核`` だけを読み、意味座標は由来参照として保持する。\n\n    HDS 構文化器再設計後は ``HDSIR.手順`` を意味 frontendの責任から外し、\n    意味 IRから本降下器が計算作用を形成する構造へ更新する。\n    '

    if not ir.実行可能:
        reasons = ",".join(ir.実行阻害理由) or "実行不能"
        raise ValueError(f"HDS-IRを計算中間表現へ降下できない:{reasons}")
    if ir.手順 is None:
        raise ValueError("HDS-IRに閉包済み互換手順がない")

    lowered = 命令計算降下(ir.手順)
    provenance = []
    for item in (ir.認知世界ID, *ir.実行核.入力座標, ir.実行核.出力座標):
        if item and item not in provenance:
            provenance.append(item)

    return replace(
        lowered,
        名称=ir.種別 or lowered.名称,
        由来=f"HDS-IR:{ir.認知世界ID}",
        由来参照=tuple(provenance),
        境界=tuple(ir.実行核.境界),
        検証=tuple(ir.実行核.検証),
    )


__all__ = ["HDS計算降下"]

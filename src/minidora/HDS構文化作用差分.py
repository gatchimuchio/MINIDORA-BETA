from __future__ import annotations

from .HDS構文化記録_v1_1 import HDS状態遷移図, HDS遷移辺
from .HDS構文化記録_v1_3 import (
    HDS作用差分構造,
    HDS作用記録,
    HDS状態差記録,
    HDS後続利用記録,
)


_作用正規化 = {
    "transition": "遷移",
    "conditional transition": "条件遷移",
    "rollback": "巻戻し",
    "roll back": "巻戻し",
    "revert": "巻戻し",
    "undo": "巻戻し",
}


def _日本語作用(raw: str) -> str:
    value = " ".join(str(raw).split()).strip()
    return _作用正規化.get(value.casefold(), value or "状態遷移")


def _作用種別(edge: HDS遷移辺) -> str:
    values = tuple(dict.fromkeys(_日本語作用(value) for value in edge.作用 if str(value).strip()))
    if not values:
        return "状態遷移"
    return "・".join(values)


def HDS作用差分構造生成(関係図: HDS状態遷移図) -> HDS作用差分構造:
    '状態遷移図から作用→状態差→後続利用の有限射影を生成する。\n\n    後続利用は「後状態が次作用の入力状態に一致する」ことだけを表す。\n    次作用の追加条件充足・採用・実行済みは確定しない。\n    '

    actions: list[HDS作用記録] = []
    deltas: list[HDS状態差記録] = []
    downstream: list[HDS後続利用記録] = []
    unresolved = list(関係図.未閉包)

    for index, edge in enumerate(関係図.遷移):
        作用_id = f"作用:{index:03d}"
        actions.append(
            HDS作用記録(
                作用_id,
                _作用種別(edge),
                edge.始点,
                edge.終点,
                tuple(edge.条件),
                edge.可逆,
                edge.rollback先,
                edge.遷移ID,
            )
        )
        if edge.始点 is None or edge.終点 is None:
            unresolved.append(f"{edge.遷移ID}:作用前後状態未閉包")
            continue
        deltas.append(
            HDS状態差記録(
                f"状態差:{len(deltas):03d}",
                作用_id,
                edge.始点,
                edge.終点,
                edge.始点 != edge.終点,
                edge.遷移ID,
            )
        )

    by_input: dict[str, list[HDS作用記録]] = {}
    for 作用 in actions:
        if 作用.入力状態 is not None:
            by_input.setdefault(作用.入力状態, []).append(作用)

    for delta in deltas:
        if not delta.変化有無:
            continue
        for 作用 in by_input.get(delta.後状態, ()):  # 到達状態が次作用の入力条件になる。
            if 作用.作用ID == delta.原因作用ID:
                continue
            downstream.append(
                HDS後続利用記録(
                    f"後続利用:{len(downstream):03d}",
                    delta.差分ID,
                    delta.後状態,
                    作用.作用ID,
                    tuple(作用.条件),
                    True,
                )
            )

    return HDS作用差分構造(
        tuple(actions),
        tuple(deltas),
        tuple(downstream),
        tuple(dict.fromkeys(unresolved)),
    )


__all__ = ["HDS作用差分構造生成"]

from __future__ import annotations

from dataclasses import dataclass
from hashlib import sha256
import json
from typing import Callable, Mapping, Any


def _sig(value: object) -> str:
    raw = json.dumps(value, ensure_ascii=False, sort_keys=True, default=str, separators=(",", ":"))
    return sha256(raw.encode("utf-8")).hexdigest()


@dataclass(frozen=True, slots=True)
class HDS作用差分観測:
    変種: str
    状態変更: bool
    参照集合変更: bool
    経路変更: bool
    計算量変更: bool
    出力変更: bool
    全体署名変更: bool

    @property
    def 実効差あり(self) -> bool:
        return any((
            self.状態変更,
            self.参照集合変更,
            self.経路変更,
            self.計算量変更,
            self.出力変更,
            self.全体署名変更,
        ))


@dataclass(frozen=True, slots=True)
class HDS作用実効監査結果:
    作用名: str
    基準署名: str
    差分: tuple[HDS作用差分観測, ...]

    @property
    def 実効作用(self) -> bool:
        return any(row.実効差あり for row in self.差分)


def _view(payload: Mapping[str, Any] | object) -> dict[str, Any]:
    if isinstance(payload, Mapping):
        資料 = dict(payload)
    elif hasattr(payload, "辞書") and callable(getattr(payload, "辞書")):
        資料 = dict(getattr(payload, "辞書")())
    elif hasattr(payload, "辞書化") and callable(getattr(payload, "辞書化")):
        資料 = dict(getattr(payload, "辞書化")())
    elif hasattr(payload, "__dict__"):
        資料 = dict(getattr(payload, "__dict__"))
    else:
        資料 = {"output": repr(payload)}
    return {
        '状態': 資料.get('状態', 資料.get("状態")),
        "references": 資料.get("references", 資料.get("参照集合", 資料.get("参照"))),
        "path": 資料.get("path", 資料.get("経路", 資料.get("作用履歴"))),
        "compute": 資料.get("compute", 資料.get("計算量", 資料.get("計算回数"))),
        "output": 資料.get("output", 資料.get("出力", 資料.get("結果"))),
        "raw": 資料,
    }


def HDS作用実効監査(
    作用名: str,
    *,
    基準実行: Callable[[], object],
    変種実行: Mapping[str, Callable[[], object]],
) -> HDS作用実効監査結果:
    '状態の存在ではなく、除去/固定/置換で後続が実際に変化するかを監査する。\n\n    変種名は自由だが、推奨は `removed`, `fixed`, `replaced`, `経路_fixed`, `参照_fixed`。\n    '
    baseline = _view(基準実行())
    base_sig = _sig(baseline["raw"])
    rows: list[HDS作用差分観測] = []
    for name, runner in 変種実行.items():
        variant = _view(runner())
        rows.append(HDS作用差分観測(
            str(name),
            baseline['状態'] != variant['状態'],
            baseline["references"] != variant["references"],
            baseline["path"] != variant["path"],
            baseline["compute"] != variant["compute"],
            baseline["output"] != variant["output"],
            base_sig != _sig(variant["raw"]),
        ))
    return HDS作用実効監査結果(str(作用名), base_sig, tuple(rows))


__all__ = ["HDS作用差分観測", "HDS作用実効監査結果", "HDS作用実効監査"]

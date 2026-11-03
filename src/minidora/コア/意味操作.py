"""領域型を潰さずに共有する対象束縛・具体化操作。"""
from __future__ import annotations
from dataclasses import dataclass
from typing import Callable
from .値 import 署名, 文字


@dataclass(frozen=True, slots=True)
class 意味座標:
    対象: str
    関係: str
    範囲: str = "未指定"
    時点: str = "未指定"

    def __post_init__(self):
        for name in ("対象", "関係", "範囲", "時点"):
            文字(getattr(self, name), name)

    @property
    def 署名(self) -> str:
        return 署名((self.対象, self.関係, self.範囲, self.時点))


def _項目群(値) -> tuple[str, ...]:
    必須 = ("対象", "関係", "値", "範囲", "時点")
    if any(not hasattr(値, name) for name in 必須):
        raise TypeError("束縛対象は対象・関係・値・範囲・時点を持つ必要がある")
    return 必須


def 束縛を試す(型, 値, 初期=None):
    """?で始まる明示変数だけを束縛する。候補不一致はNoneで返す。"""
    fields = _項目群(型)
    _項目群(値)
    束縛 = dict(初期 or {})
    for name in fields:
        pattern, actual = getattr(型, name), getattr(値, name)
        if isinstance(pattern, str) and pattern.startswith("?"):
            if pattern in 束縛 and 署名(束縛[pattern]) != 署名(actual):
                return None
            束縛[pattern] = actual
        elif 署名(pattern) != 署名(actual):
            return None
    return 束縛


def 具体化する(型, 束縛, *, 生成: Callable | None = None):
    """束縛された明示変数を具体化する。未束縛変数はNoneで保留する。"""
    fields = _項目群(型)
    values = []
    for name in fields:
        value = getattr(型, name)
        if isinstance(value, str) and value.startswith("?"):
            if value not in 束縛:
                return None
            value = 束縛[value]
        values.append(value)
    maker = 生成 or type(型)
    return maker(*values)


def 座標を取る(値) -> 意味座標:
    _項目群(値)
    return 意味座標(str(値.対象), str(値.関係), str(値.範囲), str(値.時点))

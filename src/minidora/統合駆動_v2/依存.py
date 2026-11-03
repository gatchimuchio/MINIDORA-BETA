"""根拠→認識→成果→成立状態の依存。周期を含めて有限に失効伝播する。"""
from __future__ import annotations
from dataclasses import dataclass
from collections import deque
from .値 import 文字


@dataclass(frozen=True, slots=True, order=True)
class HDS依存辺:
    前提: str
    後続: str

    def __post_init__(self):
        for x in (self.前提, self.後続):
            文字(x, "依存ノード")
            if ":" not in x or x.split(":", 1)[0] not in {"認識", "仮説", "資料", "成果", "状態", "草案", "形成", "主体", "枝"}:
                raise ValueError("依存ノードは認識:/仮説:/資料:/成果:/状態:/草案:/形成:で指定する")
            文字(x.split(":", 1)[1])
        if self.前提 == self.後続:
            raise ValueError("自己依存は無効")


def 下流集合(起点: set[str] | frozenset[str], 辺: tuple[HDS依存辺, ...]) -> frozenset[str]:
    隣接: dict[str, set[str]] = {}
    for x in 辺:
        隣接.setdefault(x.前提, set()).add(x.後続)
    待ち = deque(sorted(起点))
    済 = set(起点)
    影響: set[str] = set()
    while 待ち:
        n = 待ち.popleft()
        for 子 in sorted(隣接.get(n, ())):
            影響.add(子)
            if 子 not in 済:
                済.add(子)
                待ち.append(子)
    return frozenset(影響)


def 上流集合(対象: set[str] | frozenset[str], 辺: tuple[HDS依存辺, ...]) -> frozenset[str]:
    return 下流集合(対象, tuple(HDS依存辺(x.後続, x.前提) for x in 辺))

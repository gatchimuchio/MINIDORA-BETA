from __future__ import annotations

from dataclasses import dataclass
from hashlib import sha256
import json
from typing import Iterable, Mapping, Sequence


def _stable(prefix: str, value: object) -> str:
    raw = json.dumps(value, ensure_ascii=False, sort_keys=True, default=str)
    return prefix + sha256(raw.encode("utf-8")).hexdigest()[:18]


def _values(mapping: Mapping[str, float] | Iterable[tuple[str, float]]) -> tuple[tuple[str, float], ...]:
    情報源 = dict(mapping)
    return tuple(sorted((str(key), float(value)) for key, value in 情報源.items()))


@dataclass(frozen=True, slots=True)
class HDS状態Lane:
    """意味役割を固定しない独立状態lane。"""

    laneID: str
    値: tuple[tuple[str, float], ...] = ()

    def 辞書(self) -> dict[str, float]:
        return dict(self.値)


@dataclass(frozen=True, slots=True)
class HDS並列作業状態:
    """一つの中間状態へ早期上書きせず、複数laneを並行保持する。"""

    状態ID: str
    lane群: tuple[HDS状態Lane, ...]
    revision: int = 0

    @property
    def lane数(self) -> int:
        return len(self.lane群)


def HDS並列作業状態生成(
    lane_values: Sequence[Mapping[str, float] | Iterable[tuple[str, float]]],
) -> HDS並列作業状態:
    lanes = tuple(HDS状態Lane(f"lane:{index}", _values(values)) for index, values in enumerate(lane_values))
    if not lanes:
        raise ValueError("並列作業状態には1つ以上のlaneが必要")
    return HDS並列作業状態(_stable("PS-", tuple(lane.値 for lane in lanes)), lanes, 0)


def _matrix(raw: Sequence[Sequence[float]], n: int) -> list[list[float]]:
    if len(raw) != n or any(len(row) != n for row in raw):
        raise ValueError("混合行列はlane数と同じ正方行列である必要がある")
    out: list[list[float]] = []
    for row in raw:
        values = [max(0.0, float(value)) for value in row]
        out.append(values)
    if all(sum(row) == 0.0 for row in out):
        return [[1.0 if i == j else 0.0 for j in range(n)] for i in range(n)]
    return out


def HDS制約混合行列(
    raw: Sequence[Sequence[float]],
    *,
    反復回数: int = 20,
    epsilon: float = 1e-12,
) -> tuple[tuple[float, ...], ...]:
    """非負行列を決定論的に行・列交互正規化する。

    これはmHCのSinkhorn制約を非ニューラル状態混合へ射影した構造類似であり、
    GLM内部weightや意味laneを再現するものではない。
    """

    n = len(raw)
    if n <= 0:
        raise ValueError("混合行列は空にできない")
    matrix = _matrix(raw, n)
    eps = max(float(epsilon), 1e-18)

    for _ in range(max(1, int(反復回数))):
        for i in range(n):
            total = sum(matrix[i])
            if total <= eps:
                matrix[i] = [1.0 if i == j else 0.0 for j in range(n)]
            else:
                matrix[i] = [value / total for value in matrix[i]]
        for j in range(n):
            total = sum(matrix[i][j] for i in range(n))
            if total <= eps:
                for i in range(n):
                    matrix[i][j] = 1.0 if i == j else 0.0
            else:
                for i in range(n):
                    matrix[i][j] /= total

    return tuple(tuple(float(value) for value in row) for row in matrix)


def HDS並列状態混合(
    状態: HDS並列作業状態,
    raw_matrix: Sequence[Sequence[float]],
    *,
    反復回数: int = 20,
) -> HDS並列作業状態:
    matrix = HDS制約混合行列(raw_matrix, 反復回数=反復回数)
    情報源 = [lane.辞書() for lane in 状態.lane群]
    keys = tuple(sorted({key for lane in 情報源 for key in lane}))
    lanes: list[HDS状態Lane] = []

    for target_index in range(状態.lane数):
        mixed: dict[str, float] = {}
        for 情報源_index in range(状態.lane数):
            weight = matrix[target_index][情報源_index]
            if weight == 0.0:
                continue
            for key in keys:
                mixed[key] = mixed.get(key, 0.0) + weight * 情報源[情報源_index].get(key, 0.0)
        lanes.append(HDS状態Lane(f"lane:{target_index}", _values(mixed)))

    revision = 状態.revision + 1
    payload = (状態.状態ID, revision, tuple(lane.値 for lane in lanes), matrix)
    return HDS並列作業状態(_stable("PS-", payload), tuple(lanes), revision)


def HDS並列状態読書混合(
    状態: HDS並列作業状態,
    読取行列: Sequence[Sequence[float]],
    書戻行列: Sequence[Sequence[float]],
    *,
    反復回数: int = 20,
) -> HDS並列作業状態:
    """read-mixとwrite-mixを分離した二段の制約混合。"""

    read_状態 = HDS並列状態混合(状態, 読取行列, 反復回数=反復回数)
    return HDS並列状態混合(read_状態, 書戻行列, 反復回数=反復回数)


__all__ = [
    "HDS状態Lane",
    "HDS並列作業状態",
    "HDS並列作業状態生成",
    "HDS制約混合行列",
    "HDS並列状態混合",
    "HDS並列状態読書混合",
]

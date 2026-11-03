from __future__ import annotations

import re
from typing import Sequence

from .科学専門能力_共通 import _結果


def _entry(raw: str) -> complex | None:
    s = raw.strip().replace(' ', '').casefold().replace('−', '-')
    if re.fullmatch(r'[+-]?i', s):
        return complex(0, -1 if s.startswith('-') else 1)
    match = re.fullmatch(r'([+-]?\d+(?:\.\d+)?)i', s)
    if match:
        return complex(0, float(match.group(1)))
    try:
        return complex(float(s), 0)
    except ValueError:
        return None


def _matrix(question: str, label: str):
    match = re.search(rf'\b{re.escape(label)}\s*(?:=|-)\s*\(([^()]*)\)', question)
    if not match:
        return None
    rows = []
    for raw_row in match.group(1).split(';'):
        row = []
        for raw in raw_row.split(','):
            value = _entry(raw)
            if value is None:
                return None
            row.append(value)
        rows.append(row)
    if len(rows) != 3 or any(len(row) != 3 for row in rows):
        return None
    return rows


def _close(a: complex, b: complex, tol: float = 1e-9) -> bool:
    return abs(a - b) <= tol * max(1.0, abs(a), abs(b))


def _antihermitian(a) -> bool:
    return all(
        _close(a[j][i].conjugate(), -a[i][j])
        for i in range(3)
        for j in range(3)
    )


def _hermitian(a) -> bool:
    return all(
        _close(a[j][i].conjugate(), a[i][j])
        for i in range(3)
        for j in range(3)
    )


def _det3(a):
    return (
        a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1])
        - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
        + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0])
    )


def _density(a) -> bool:
    if not _hermitian(a):
        return False
    追跡 = sum(a[i][i] for i in range(3))
    if not _close(追跡, 1):
        return False
    if any(a[i][i].real < -1e-9 or abs(a[i][i].imag) > 1e-9 for i in range(3)):
        return False
    for i, j in ((0, 1), (0, 2), (1, 2)):
        minor = a[i][i] * a[j][j] - a[i][j] * a[j][i]
        if minor.real < -1e-9 or abs(minor.imag) > 1e-8:
            return False
    determinant = _det3(a)
    return determinant.real >= -1e-9 and abs(determinant.imag) <= 1e-8


def 解決(question: str, choices: Sequence[str]):
    s = question.casefold()
    if 'quantum mechanics' not in s or 'matrices' not in s:
        return None
    if not any('e^x' in str(選択肢).casefold().replace(' ', '') for 選択肢 in choices):
        return None
    x = _matrix(question, 'X')
    y = _matrix(question, 'Y')
    if x is None or y is None or not _antihermitian(x) or not _density(y):
        return None
    hits = []
    for i, 選択肢 in enumerate(choices):
        c = str(選択肢).casefold().replace(' ', '')
        if 'e^x' in c and 'y' in c and 'e^{-x}' in c and ('quantumstate' in c or '状態' in c):
            hits.append(i)
    return _結果(
        hits[0] if len(hits) == 1 else None,
        'quantum_matrix_unitary_similarity',
        'exp(X)Yexp(-X)',
        'anti-Hermitian generator preserves density matrix',
    )


__all__ = ['解決']

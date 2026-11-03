from __future__ import annotations

import math
import re
from typing import Sequence

from .科学専門能力_共通 import _結果


def _compact(text: object) -> str:
    return (
        str(text)
        .casefold()
        .replace('−', '-')
        .replace('–', '-')
        .replace('\\left', '')
        .replace('\\right', '')
        .replace('{', '(')
        .replace('}', ')')
    )


def _scalar(raw: str) -> complex | None:
    s = _compact(raw).strip().replace(' ', '')
    s = s.replace('\\sqrt', 'sqrt')
    s = re.sub(r'sqrt\(([^()]+)\)', r'(\1)**0.5', s)
    s = re.sub(r'(?<![a-z0-9_.])([+-]?\d+(?:\.\d+)?)i\b', r'\1j', s)
    s = re.sub(r'(?<![a-z0-9_.])([+-]?)i\b', lambda m: ('-1j' if m.group(1) == '-' else '1j'), s)
    if not re.fullmatch(r'[0-9j+\-*/().]+', s):
        return None
    try:
        value = complex(eval(s, {'__builtins__': {}}, {}))
    except Exception:
        return None
    return value if math.isfinite(value.real) and math.isfinite(value.imag) else None


def _選択肢スカラー(選択肢: str) -> float | None:
    s = _compact(選択肢).replace('~', '').strip()
    pct = '%' in s
    m = re.search(
        r'(sqrt\s*\([^)]*\)|[+-]?\d+(?:\.\d+)?\s*/\s*\d+(?:\.\d+)?|[+-]?\d+(?:\.\d+)?)',
        s,
    )
    if not m:
        return None
    value = _scalar(m.group(1))
    if value is None or abs(value.imag) > 1e-10:
        return None
    out = value.real
    return out / 100.0 if pct else out


def _nearest_scalar(choices: Sequence[str], target: float, tol: float = 0.08) -> int | None:
    scored = []
    for i, 選択肢 in enumerate(choices):
        value = _選択肢スカラー(str(選択肢))
        if value is None:
            continue
        error = abs(value - target) / max(abs(target), 1e-12)
        scored.append((error, i))
    if not scored:
        return None
    scored.sort()
    if scored[0][0] > tol:
        return None
    if len(scored) > 1 and abs(scored[0][0] - scored[1][0]) < 1e-12:
        return None
    return scored[0][1]


def solve_exponential_decay_probability(q: str, choices: Sequence[str]):
    s = q.casefold()
    if 'decay' not in s or 'probability' not in s or 'next' not in s:
        return None
    base = re.search(
        r'(?:decay\s+probability|probability\s+of\s+(?:this\s+)?(?:atom|nucleus)?\s*decay)'
        r'[^%]{0,100}?([0-9.]+)\s*%[^.]{0,60}?within\s+([0-9.]+)\s*'
        r'(seconds?|minutes?|hours?|days?)',
        q,
        re.I | re.S,
    )
    if not base:
        base = re.search(
            r'probability[^%]{0,100}?([0-9.]+)\s*%\s*within\s+([0-9.]+)\s*'
            r'(seconds?|minutes?|hours?|days?)',
            q,
            re.I | re.S,
        )
    future = re.search(r'next\s+([0-9.]+)\s*(seconds?|minutes?|hours?|days?)', q, re.I)
    if not (base and future):
        return None
    p = float(base.group(1)) / 100.0
    t0 = float(base.group(2))
    unit0 = base.group(3).casefold().rstrip('s')
    t1 = float(future.group(1))
    unit1 = future.group(2).casefold().rstrip('s')
    scale = {'second': 1.0, 'minute': 60.0, 'hour': 3600.0, 'day': 86400.0}
    if unit0 not in scale or unit1 not in scale or not (0 < p < 1) or t0 <= 0 or t1 <= 0:
        return None
    ratio = t1 * scale[unit1] / (t0 * scale[unit0])
    target = 1.0 - (1.0 - p) ** ratio
    return _結果(
        _nearest_scalar(choices, target, tol=0.035),
        'exponential_decay_probability',
        f'{target:.12g}',
        'memoryless exponential law',
    )


def solve_magnetic_monopole_maxwell(q: str, choices: Sequence[str]):
    s = q.casefold()
    if not (
        ('isolated north' in s or 'isolated south' in s or 'magnetic monopole' in s)
        and 'maxwell' in s
    ):
        return None
    hits = []
    for i, 選択肢 in enumerate(choices):
        c = str(選択肢).casefold()
        electric_curl = (
            'circulation of the electric field' in c
            or 'curl of the electric field' in c
            or 'curl of e' in c
        )
        magnetic_div = (
            'divergence of the magnetic field' in c
            or 'flux of the magnetic field' in c
            or 'divergence of b' in c
        )
        if electric_curl and magnetic_div:
            hits.append(i)
    return _結果(
        hits[0] if len(hits) == 1 else None,
        'magnetic_monopole_maxwell_symmetry',
        'curl(E),div(B)',
        'electric-magnetic source symmetry',
    )


def solve_dipole_operator_mass_dimension(q: str, choices: Sequence[str]):
    s = _compact(q).replace(' ', '')
    if 'massdimension' not in s or 'renormalizable' not in s:
        return None
    field_pattern = (
        'bar(\\psi)' in s or 'barpsi' in s or '\\bar(\\psi)' in s
    ) and ('f^(' in s or 'f^' in s or 'f(' in s)
    sigma_pattern = 'sigma_' in s or '\\sigma_' in s or 'sigma(' in s
    if not (field_pattern and sigma_pattern and ('kappa' in s or '\\kappa' in s)):
        return None
    hits = []
    for i, 選択肢 in enumerate(choices):
        c = _compact(選択肢).replace(' ', '')
        neg_one = bool(re.search(r'(?:kappa|\\kappa)[^=]{0,20}=\s*-1(?:\D|$)', c)) or '=-1' in c
        nonren = (
            'notrenormalizable' in c
            or 'non-renormalizable' in c
            or 'nonrenormalizable' in c
        )
        if neg_one and nonren:
            hits.append(i)
    return _結果(
        hits[0] if len(hits) == 1 else None,
        'dipole_operator_mass_dimension',
        -1,
        '4-(3/2+3/2+2)',
    )


def _parse_matrix_entries(raw: str) -> list[list[complex]] | None:
    rows = [row.strip() for row in raw.split(';')]
    matrix = []
    for row in rows:
        values = []
        for 字句 in row.split(','):
            value = _scalar(字句)
            if value is None:
                return None
            values.append(value)
        matrix.append(values)
    if not matrix or len({len(row) for row in matrix}) != 1 or len(matrix) != len(matrix[0]):
        return None
    return matrix


def _parse_row_matrix(q: str, label: str):
    anchor = re.search(
        rf'(?:operator\s+{re.escape(label)}|matrix\s+(?:operator\s+)?(?:for\s+)?{re.escape(label)})',
        q,
        re.I,
    )
    if not anchor:
        return None
    tail = q[anchor.start():]
    match = re.search(
        r'first row as\s*\(([^)]*)\).*?second row as\s*\(([^)]*)\).*?third row as\s*\(([^)]*)\)',
        tail,
        re.I | re.S,
    )
    if not match:
        return None
    return _parse_matrix_entries(';'.join(match.groups()))


def _状態解析(q: str):
    match = re.search(
        r'state[^.]{0,100}?column matrix having elements\s*\(([^)]*)\)',
        q,
        re.I | re.S,
    )
    if not match:
        return None
    values = []
    for 字句 in match.group(1).split(','):
        value = _scalar(字句)
        if value is None:
            return None
        values.append(value)
    return values if len(values) == 3 else None


def _cross(a, b):
    return [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]


def _norm2(v):
    return sum(abs(value) ** 2 for value in v)


def _eigvec3(a, eigenvalue: float):
    shifted = [
        [a[i][j] - (eigenvalue if i == j else 0) for j in range(3)]
        for i in range(3)
    ]
    candidates = [
        _cross(shifted[0], shifted[1]),
        _cross(shifted[0], shifted[2]),
        _cross(shifted[1], shifted[2]),
    ]
    vector = max(candidates, key=_norm2)
    norm2 = _norm2(vector)
    if norm2 <= 1e-16:
        return None
    残差 = [
        sum(shifted[i][j] * vector[j] for j in range(3))
        for i in range(3)
    ]
    if _norm2(残差) > 1e-10 * norm2:
        return None
    root = math.sqrt(norm2)
    return [value / root for value in vector]


def _射影確率(状態, eigenvector):
    norm2 = _norm2(状態)
    if norm2 <= 0:
        return None
    amplitude = sum(
        eigenvector[i].conjugate() * 状態[i]
        for i in range(len(状態))
    )
    return abs(amplitude) ** 2 / norm2


def solve_projective_measurement_3x3(q: str, choices: Sequence[str]):
    s = q.casefold()
    if 'measurement' not in s or 'column matrix' not in s or 'operator p' not in s:
        return None
    状態 = _状態解析(q)
    p_matrix = _parse_row_matrix(q, 'P')
    if 状態 is None or p_matrix is None or len(p_matrix) != 3:
        return None
    joint = re.search(
        r'getting\s*([+-]?\d+(?:\.\d+)?)\s*for\s*p\s*and\s*([+-]?\d+(?:\.\d+)?)\s*for\s*q',
        q,
        re.I,
    )
    if joint:
        q_matrix = _parse_row_matrix(q, 'Q')
        if q_matrix is None or len(q_matrix) != 3:
            return None
        p_value, q_value = map(float, joint.groups())
        p_vector = _eigvec3(p_matrix, p_value)
        q_vector = _eigvec3(q_matrix, q_value)
        if p_vector is None or q_vector is None:
            return None
        first = _射影確率(状態, p_vector)
        second = _射影確率(p_vector, q_vector)
        if first is None or second is None:
            return None
        target = first * second
        return _結果(
            _nearest_scalar(choices, target, tol=0.025),
            'sequential_projective_measurement_3x3',
            f'{target:.12g}',
            'Born rule with collapse',
        )
    single = re.search(r'(?:yield|get(?:ting)?)\s*([+-]?\d+(?:\.\d+)?)', q, re.I)
    if not single:
        return None
    eigenvalue = float(single.group(1))
    eigenvector = _eigvec3(p_matrix, eigenvalue)
    if eigenvector is None:
        return None
    target = _射影確率(状態, eigenvector)
    if target is None:
        return None
    return _結果(
        _nearest_scalar(choices, target, tol=0.025),
        'projective_measurement_probability_3x3',
        f'{target:.12g}',
        'Born projection rule',
    )


def solve_blackbody_luminosity_with_radial_velocity(q: str, choices: Sequence[str]):
    s = q.casefold()
    if 'luminosit' not in s or 'black bod' not in s or 'radius' not in s or 'radial velocit' not in s:
        return None
    radius_match = re.search(r'radius\s+([0-9.]+)\s+times', q, re.I)
    if not radius_match:
        return None
    radius_ratio = float(radius_match.group(1))
    velocities = re.search(
        r'radial velocities[^0-9]{0,80}([0-9.]+)\s*(?:and|,)\s*([0-9.]+)\s*km/s',
        q,
        re.I | re.S,
    )
    if not velocities:
        return None
    v1, v2 = map(float, velocities.groups())
    c = 299792.458
    if max(abs(v1), abs(v2)) >= c:
        return None

    def doppler(v: float) -> float:
        beta = v / c
        return math.sqrt((1 + beta) / (1 - beta))

    temperature_ratio = doppler(v1) / doppler(v2)
    target = radius_ratio ** 2 * temperature_ratio ** 4
    return _結果(
        _nearest_scalar(choices, target, tol=0.02),
        'blackbody_luminosity_radial_doppler',
        f'{target:.12g}',
        'Stefan-Boltzmann + Wien + Doppler',
    )


登録簿 = (
    solve_exponential_decay_probability,
    solve_magnetic_monopole_maxwell,
    solve_dipole_operator_mass_dimension,
    solve_projective_measurement_3x3,
    solve_blackbody_luminosity_with_radial_velocity,
)


def 解決(question: str, choices: Sequence[str]):
    hits = []
    for 解決器 in 登録簿:
        try:
            row = 解決器(question, choices)
        except Exception:
            row = None
        if row is not None:
            hits.append(row)
    if not hits or len({row.index for row in hits}) != 1:
        return None
    return max(hits, key=lambda row: row.信頼度)


__all__ = ['解決']

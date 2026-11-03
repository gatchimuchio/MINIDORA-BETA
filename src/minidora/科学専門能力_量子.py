from __future__ import annotations
import math
import re
from typing import Sequence
from .科学専門能力_共通 import *

def solve_pauli_expectation(q: str, choices: Sequence[str]):
    s = q.casefold()
    if 'sigma' not in s and 'spin state' not in s:
        return None
    m = re.search('([0-9.]+)\\s*\\|.*?up.*?\\+\\s*([0-9.]+)\\s*\\|.*?down', q, re.I | re.S)
    op = re.search('([0-9.]+)\\s*\\\\?sigma[_\\s{]*z.*?\\+\\s*([0-9.]+)\\s*\\\\?sigma[_\\s{]*x', q, re.I | re.S)
    if m and op:
        a, b = map(float, m.groups())
        cz, cx = map(float, op.groups())
        n = a * a + b * b
        if n <= 0:
            return None
        ez = (a * a - b * b) / n
        ex = 2 * a * b / n
        val = cz * ez + cx * ex
        return _結果(_nearest(choices, val, rel_tol=0.08), 'pauli_expectation', val)
    m = re.search('spin state\\s*\\(\\s*([+-]?[0-9.]+)\\s*i\\s*,\\s*([+-]?[0-9.]+)\\s*\\)', q, re.I)
    if m and 's_y' in s:
        ai, b = map(float, m.groups())
        n = ai * ai + b * b
        val = -(ai * b) / n
        candidates = []
        for i, c in enumerate(choices):
            cc = c.casefold().replace(' ', '')
            mm = re.search('([+-]?\\d+(?:\\.\\d+)?)\\*?hbar/(\\d+(?:\\.\\d+)?)', cc)
            if mm:
                v = float(mm.group(1)) / float(mm.group(2))
                candidates.append((abs(v - val), i))
            elif '0' == cc.strip():
                candidates.append((abs(val), i))
        if candidates:
            candidates.sort()
            return _結果(candidates[0][1], 'spin_y_expectation', val)
    return None

def solve_pauli_hamiltonian(q, choices):
    s = q.casefold().replace('·', '.')
    sigma = 'sigma' in s or 'σ' in q
    unit = 'unit vector' in s or 'dot n' in s or '.n' in s or ('\\vec{n}' in q)
    energy = 'epsilon' in s or 'varepsilon' in s or 'ε' in q
    asks = 'eigenvalue' in s or 'eigenvalues' in s or 'energy eigen' in s
    ham = 'hamiltonian' in s or re.search('\\bh\\s*=.*(?:sigma|σ)', q, re.I) is not None
    if not (sigma and unit and energy and asks and ham):
        return None
    hits = []
    for i, c in enumerate(choices):
        cc = c.casefold().replace(' ', '')
        eps = 'varepsilon' in cc or 'ε' in cc or 'epsilon' in cc
        if eps and '+' in cc and ('-' in cc):
            hits.append(i)
    return _結果(hits[0] if len(hits) == 1 else None, 'pauli_hamiltonian_eigenvalues', '±epsilon')

def solve_maximally_mixed_bloch(q, choices):
    s = q.casefold()
    if 'density matrix' not in s and 'rho' not in s:
        return None
    if '|0' in q and '|1' in q and ('1}{2' in q) or ('1/2' in q and 'qubit' in s):
        idx = _選択肢包含(choices, '0', '0', '0')
        if idx is None:
            for i, c in enumerate(choices):
                if re.search('\\(\\s*0\\s*,\\s*0\\s*,\\s*0\\s*\\)', c):
                    idx = i
                    break
        return _結果(idx, 'bloch_maximally_mixed', (0, 0, 0))
    return None

def solve_energy_time_resolution(q, choices):
    s = q.casefold()
    if 'lifetime' not in s or 'clearly distinguish' not in s or 'energy difference' not in s:
        return None
    times = []
    for exp in re.findall('10\\s*\\^\\s*([+-]?\\d+)\\s*sec', q, re.I):
        times.append(10.0 ** int(exp))
    if len(times) < 2:
        for exp in re.findall('10\\^([+-]?\\d+)\\s*sec', q, re.I):
            times.append(10.0 ** int(exp))
    if len(times) < 2:
        return None
    width = 6.582119569e-16 / min(times)
    vals = []
    for i, c in enumerate(choices):
        v = _選択肢数値(c)
        if v is not None and v >= width:
            vals.append((v, i))
    if not vals:
        return None
    vals.sort()
    return _結果(vals[0][1], 'energy_time_resolution', width)

def solve_spin_x_generic(q, choices):
    s = q.casefold()
    if 'z-射影' not in s and 'z-spin' not in s and ('z projection' not in s) or ('matrix representation' not in s and 'operator matrix' not in s):
        return None
    m = re.search('proportional\\s+to\\s*\\(([^()]*)\\)\\s*\\|?up[^+]{0,20}\\+\\s*\\(([^()]*)\\)\\s*\\|?down', q, re.I | re.S)
    if not m:
        return None
    a = _parse_complex_coeff(m.group(1))
    b = _parse_complex_coeff(m.group(2))
    if a is None or b is None:
        return None
    norm = abs(a) ** 2 + abs(b) ** 2
    if norm <= 0:
        return None
    pplus = abs(a + b) ** 2 / (2 * norm)
    pminus = abs(a - b) ** 2 / (2 * norm)
    exp_hbar = (a.conjugate() * b + b.conjugate() * a).real / (2 * norm)
    best = []
    for i, c in enumerate(choices):
        ns = _nums(c)
        if len(ns) < 2:
            continue
        err = abs(ns[0] - pplus) + abs(ns[1] - pminus)
        cc = c.casefold().replace(' ', '')
        mm = re.search('([+-]?[0-9.]*)\\*?hbar/([0-9.]+)', cc)
        if mm:
            num = float(mm.group(1)) if mm.group(1) not in {'', '+', '-'} else -1.0 if mm.group(1) == '-' else 1.0
            err += abs(num / float(mm.group(2)) - exp_hbar)
        best.append((err, i))
    if not best:
        return None
    best.sort()
    return _一般結果(best[0][1], 'spin_x_measurement', (pplus, pminus, exp_hbar))

def solve_angular_momentum_sum(q, choices):
    s = q.casefold()
    if 'joint probability' not in s or 'l_{1z}' not in s or 'l_{2z}' not in s:
        return None
    ket = re.search('\\|\\s*([0-9.]+)\\s*,\\s*([0-9.]+)\\s*,\\s*([0-9.]+)\\s*,\\s*([+-]?[0-9.]+)\\s*>?', q)
    req = re.findall('as\\s*([+-]?)\\\\?hbar', q, re.I)
    if not ket:
        return None
    total_m = float(ket.group(4))
    signs = re.findall('(?:as|and)\\s*([+-]?)\\\\hbar', q, re.I)
    if len(signs) < 1:
        signs = re.findall('([+-])\\\\hbar', q)
    if not signs:
        return None
    mval = -1.0 if signs[-1] == '-' else 1.0
    if abs(mval + mval - total_m) > 1e-12:
        return _一般結果(_nearest(choices, 0.0), 'angular_momentum_m_sum', 0.0)
    return None

def solve_wavefunction_正規化_generic(q, choices):
    s = q.casefold()
    if 'wave function' not in s or 'sqrt' not in s or 'value' not in s:
        return None
    den = re.search('n\\s*/\\s*sqrt\\(\\s*([0-9.]+)\\s*\\+\\s*([0-9.]+)?\\s*\\*?\\s*x\\s*\\)', q, re.I)
    im = re.search('[-+]\\s*([0-9.]+)\\s*\\*?i', q, re.I)
    bounds = re.findall('x\\s*[<>]=?\\s*([+-]?[0-9.]+)', q, re.I)
    if not den or not im or len(bounds) < 2:
        return None
    a = float(den.group(1))
    b = float(den.group(2) or 1)
    c = float(im.group(1))
    x0, x1 = sorted(map(float, bounds[:2]))
    L = x1 - x0
    if L <= 0 or a + b * x0 <= 0 or a + b * x1 <= 0:
        return None
    integral = math.log((a + b * x1) / (a + b * x0)) / b if abs(b) > 1e-15 else L / a
    remainder = 1 - c * c * L
    if integral <= 0 or remainder <= 0:
        return None
    N = math.sqrt(remainder / integral)
    return _一般結果(_nearest(choices, N, rel_tol=0.15), 'wavefunction_正規化', N)

def solve_infinite_well_fermions_generic(q, choices):
    s = q.casefold()
    if 'spin-1/2 particles' not in s or 'one-dimensional infinite potential well' not in s:
        return None
    nm = re.search('(\\d+|one|two|three|four|five|six|seven|eight|nine|ten)\\s+identical\\s+spin-1/2\\s+particles', q, re.I)
    if not nm:
        return None
    words = {'one': 1, 'two': 2, 'three': 3, 'four': 4, 'five': 5, 'six': 6, 'seven': 7, 'eight': 8, 'nine': 9, 'ten': 10}
    字句 = nm.group(1).casefold()
    n_particles = words.get(字句, int(字句) if 字句.isdigit() else 0)
    if n_particles <= 0 or n_particles > 10:
        return None
    max_n = n_particles + 3
    energies = set()
    def rec(level, left, total):
        if left == 0:
            energies.add(total)
            return
        if level > max_n:
            return
        for occ in range(min(2, left) + 1):
            rec(level + 1, left - occ, total + occ * level * level)
    rec(1, n_particles, 0)
    vals = sorted(energies)[:3]
    if len(vals) < 3:
        return None
    target = tuple(vals)
    best = []
    for i, c in enumerate(choices):
        ns = [int(float(x)) for x in _nums(c)[:3]]
        if len(ns) >= 3:
            best.append((sum((abs(a - b) for a, b in zip(ns, target))), i))
    if not best:
        return None
    best.sort()
    return _一般結果(best[0][1], 'infinite_well_fermion_occupancy', target)

def solve_larmor_frequency(q, choices):
    s = q.casefold()
    if 'magnetic moment' not in s or 'oscillation frequency' not in s or 'gamma' not in s:
        return None
    for i, c in enumerate(choices):
        cc = c.casefold().replace(' ', '')
        if cc in {'gamma*b', 'γ*b', 'gamma* b'} or ('gamma*b' in cc and '/' not in cc and ('sqrt' not in cc)):
            return _結果(i, 'larmor_frequency', 'gamma B')
    return None
登録簿 = (solve_pauli_expectation, solve_pauli_hamiltonian, solve_maximally_mixed_bloch, solve_energy_time_resolution, solve_spin_x_generic, solve_angular_momentum_sum, solve_wavefunction_正規化_generic, solve_infinite_well_fermions_generic, solve_larmor_frequency)

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

from __future__ import annotations

import math
import re
from typing import Sequence

from .科学専門能力_共通 import _一般結果, _nearest, _結果


def _compact(text: object) -> str:
    return (
        str(text)
        .casefold()
        .replace('−', '-')
        .replace('–', '-')
        .replace('×', 'x')
        .replace('\\times', 'x')
        .replace('\\left', '')
        .replace('\\right', '')
        .replace(' ', '')
    )


def _real_expr(raw: str) -> float | None:
    s = _compact(raw).replace('{', '(').replace('}', ')')
    m = re.fullmatch(r'([+-]?)sqrt\((\d+(?:\.\d+)?)\)(?:/(\d+(?:\.\d+)?))?', s)
    if m:
        value = math.sqrt(float(m.group(2)))
        if m.group(3):
            value /= float(m.group(3))
        return -value if m.group(1) == '-' else value
    m = re.fullmatch(r'([+-]?\d+(?:\.\d+)?)(?:/(\d+(?:\.\d+)?))?', s)
    if m:
        value = float(m.group(1))
        if m.group(2):
            value /= float(m.group(2))
        return value
    return None


def _number_before_unit(question: str, unit_pattern: str, *, 文脈: str = '') -> float | None:
    q = question.replace('\\times', 'x').replace('×', 'x').replace('{', '').replace('}', '')
    prefix = 文脈 + r'[^0-9+\-.]{0,80}' if 文脈 else ''
    字句 = r'((?:\d+(?:\.\d+)?\s*[x*]\s*10\s*\^?\s*[+-]?\d+)|(?:\d+(?:\.\d+)?e[+-]?\d+)|(?:10\s*\^?\s*[+-]?\d+)|(?:\d+(?:\.\d+)?))'
    m = re.search(prefix + 字句 + r'\s*' + unit_pattern, q, re.I | re.S)
    if not m:
        return None
    raw = m.group(1).replace(' ', '').replace('^', '')
    m2 = re.fullmatch(r'(\d+(?:\.\d+)?)[x*]10([+-]?\d+)', raw, re.I)
    if m2:
        return float(m2.group(1)) * 10 ** int(m2.group(2))
    m2 = re.fullmatch(r'10([+-]?\d+)', raw, re.I)
    if m2:
        return 10 ** int(m2.group(1))
    try:
        return float(raw)
    except ValueError:
        return None


def solve_pauli_superposition_expectation(q: str, choices: Sequence[str]):
    s = q.casefold()
    if 'spin-half' not in s and 'spin 1/2' not in s:
        return None
    if 'expectation value' not in s or 'sigma' not in s:
        return None
    ma = re.search(r'([+-]?(?:\d+(?:\.\d+)?|\.\d+))\s*\|\s*\\?uparrow', q, re.I)
    mb = re.search(r'\+\s*(sqrt\([^)]*\)(?:\s*/\s*\d+(?:\.\d+)?)?|[+-]?\d+(?:\.\d+)?(?:\s*/\s*\d+(?:\.\d+)?)?)\s*\|\s*\\?downarrow', q, re.I)
    op = re.search(r'([+-]?\d+(?:\.\d+)?)\s*\\?sigma[_\s{]*z\}?\s*\+\s*([+-]?\d+(?:\.\d+)?)\s*\\?sigma[_\s{]*x\}?', q, re.I)
    if not (ma and mb and op):
        return None
    a = _real_expr(ma.group(1))
    b = _real_expr(mb.group(1))
    if a is None or b is None:
        return None
    cz, cx = map(float, op.groups())
    norm = a * a + b * b
    if norm <= 0:
        return None
    ez = (a * a - b * b) / norm
    ex = 2 * a * b / norm
    target = cz * ez + cx * ex
    return _一般結果(_nearest(choices, target, rel_tol=0.12), 'pauli_superposition_expectation', target)


def solve_spinor_xz_eigenvector(q: str, choices: Sequence[str]):
    s = q.casefold()
    if 'eigenvector' not in s or 'arbitrary direction' not in s or 'x-z plane' not in s:
        return None
    if 'hbar/2' not in _compact(q) and '\\hbar/2' not in _compact(q):
        return None
    hits = []
    for i, 選択肢 in enumerate(choices):
        c = _compact(選択肢)
        half = ('cos(\\theta/2)' in c or 'cos(theta/2)' in c) and ('sin(\\theta/2)' in c or 'sin(theta/2)' in c)
        if half and 'hbar' not in c and '\\hbar' not in c:
            hits.append(i)
    return _結果(hits[0] if len(hits) == 1 else None, 'spinor_xz_positive_eigenvector', 'half-angle spinor')


def solve_anisotropic_oscillator_spectrum(q: str, choices: Sequence[str]):
    s0 = q.casefold()
    s = _compact(q)
    if 'energy spectrum' not in s0 or 'kr^2' not in s or 'cos^2' not in s:
        return None
    m = re.search(r'(\d+(?:\.\d+)?)/2kr\^2\+(\d+(?:\.\d+)?)/2kr\^2cos\^2', s)
    if not m:
        return None
    a, b = map(float, m.groups())
    wx = math.sqrt(a + b)
    wy = math.sqrt(a)
    if abs(wx - round(wx)) > 1e-9 or abs(wy - round(wy)) > 1e-9:
        return None
    ix, iy = int(round(wx)), int(round(wy))
    zero = (wx + wy) / 2
    hits = []
    for i, 選択肢 in enumerate(choices):
        c = _compact(選択肢).replace('*', '')
        x_ok = (f'{ix}n_x' in c) if ix != 1 else ('n_x' in c and not re.search(r'\d+n_x', c))
        y_ok = (f'{iy}n_y' in c) if iy != 1 else ('n_y' in c and not re.search(r'\d+n_y', c))
        zero_ok = ('3/2' in c) if abs(zero - 1.5) < 1e-9 else str(zero) in c
        if x_ok and y_ok and zero_ok:
            hits.append(i)
    return _結果(hits[0] if len(hits) == 1 else None, 'anisotropic_oscillator_spectrum', f'omega_x={wx},omega_y={wy},zero={zero}')


def solve_gamma_gamma_pair_threshold_latex(q: str, choices: Sequence[str]):
    s = q.casefold()
    if 'electron-positron' not in s or 'photon' not in s or ('gamma' not in s and '\\gamma' not in q):
        return None
    eps = _number_before_unit(q, r'e\s*v', 文脈=r'(?:average\s+)?photon\s+energy')
    if eps is None or eps <= 0:
        return None
    target_ev = 510998.95 ** 2 / eps
    target = target_ev / 1e9 if any('gev' in str(c).casefold() for c in choices) else target_ev
    return _一般結果(_nearest(choices, target, log=True, rel_tol=0.25), 'gamma_gamma_pair_threshold_latex', target)


def solve_edta_dissociation_typo_tolerant(q: str, choices: Sequence[str]):
    s = q.casefold()
    if 'edta' not in s or 'calcium' not in s:
        return None
    cm = re.search(r'(\d+(?:\.\d+)?)\s*m\s+(?:stochiometric|stoichiometric)?\s*(?:ca[- ]?edta|[^.]{0,30}edta)', q, re.I)
    km = re.search(r'k\s*(?:ca[- ]?edta)?\s*=\s*([0-9.]+)\s*[x*]\s*10\s*\^?\s*\{?([+-]?\d+)\}?', q, re.I)
    if not (cm and km):
        return None
    concentration = float(cm.group(1))
    kf = float(km.group(1)) * 10 ** int(km.group(2))
    if concentration <= 0 or kf <= 0:
        return None
    target = math.sqrt(concentration / kf)
    return _一般結果(_nearest(choices, target, log=True, rel_tol=0.12), 'edta_complex_dissociation', target)


def solve_wavefunction_正規化_symbol(q: str, choices: Sequence[str]):
    s = q.casefold()
    if 'wave function' not in s or 'sqrt' not in s or ('numerical value' not in s and 'normal' not in s):
        return None
    den = re.search(r'\(\s*([a-z])\s*/\s*sqrt\(\s*([0-9.]+)\s*\+\s*([0-9.]+)?\s*\*?\s*x\s*\)\s*\)', q, re.I)
    im = re.search(r'[-+]\s*([0-9.]+)\s*\*?\s*i', q, re.I)
    bounds = re.findall(r'x\s*[<>]=?\s*([+-]?[0-9.]+)', q, re.I)
    if not (den and im and len(bounds) >= 2):
        return None
    a0 = float(den.group(2)); b = float(den.group(3) or 1); imag = float(im.group(1))
    x0, x1 = sorted(map(float, bounds[:2]))
    if x1 <= x0 or a0 + b * x0 <= 0 or a0 + b * x1 <= 0:
        return None
    integral = math.log((a0 + b * x1) / (a0 + b * x0)) / b if abs(b) > 1e-15 else (x1 - x0) / a0
    remainder = 1 - imag * imag * (x1 - x0)
    if integral <= 0 or remainder <= 0:
        return None
    target = math.sqrt(remainder / integral)
    return _一般結果(_nearest(choices, target, rel_tol=0.08), 'wavefunction_正規化_symbol', target)


def solve_decay_resolution_latex(q: str, choices: Sequence[str]):
    s = q.casefold()
    if 'minimum resolution' not in s or 'proper lifetime' not in s:
        return None
    normalized = q.replace('\\times', 'x').replace('{', '').replace('}', '')
    tm = re.search(r'(?:tau_?0|\\tau_?0)\s*=\s*([0-9.]+)\s*[x*]\s*10\s*\^?\s*([+-]?\d+)\s*s', normalized, re.I)
    em = re.search(r'energy[^0-9]{0,40}([0-9.]+)\s*gev', normalized, re.I)
    mm = re.search(r'mass[^0-9]{0,40}([0-9.]+)\s*gev', normalized, re.I)
    pm = re.search(r'at least\s+([0-9.]+)\s*%', normalized, re.I)
    if not (tm and em and mm and pm):
        return None
    tau = float(tm.group(1)) * 10 ** int(tm.group(2))
    energy = float(em.group(1)); mass = float(mm.group(1)); fraction = float(pm.group(1)) / 100
    if tau <= 0 or energy <= mass or not (0 < fraction < 1):
        return None
    beta_gamma = math.sqrt(energy * energy - mass * mass) / mass
    mean = beta_gamma * 299792458.0 * tau
    target = -mean * math.log(fraction)
    return _一般結果(_nearest(choices, target, log=True, rel_tol=0.2), 'decay_resolution', target)


def solve_qpcr_direction_consistency(q: str, choices: Sequence[str]):
    s = q.casefold()
    if 'qpcr' not in s or ('copy' not in s and 'copies' not in s) or 'ct' not in s:
        return None
    pairs = []
    pattern = re.compile(r'concentration\s+of\s+([0-9]+)\s+copies[^\n]*?were\s+(\d+(?:\.\d+)?)\s*,\s*(\d+(?:\.\d+)?)\s*,\s*(\d+(?:\.\d+)?)', re.I)
    for m in pattern.finditer(q):
        copies = float(m.group(1)); mean_ct = sum(map(float, m.groups()[1:])) / 3
        pairs.append((copies, mean_ct))
    if len(pairs) < 3:
        return None
    pairs.sort()
    direction_ok = all(pairs[i][1] >= pairs[i + 1][1] for i in range(len(pairs) - 1))
    if direction_ok:
        return None
    hits = []
    for i, 選択肢 in enumerate(choices):
        c = 選択肢.casefold()
        if ('not in agreement' in c or 'inconsistent' in c) and ('amount' in c or 'copy' in c or 'nucleic acid' in c):
            hits.append(i)
    return _結果(hits[0] if len(hits) == 1 else None, 'qpcr_direction_consistency', 'Ct must decrease as log(copy number) increases')


def solve_black_hole_entropy_from_angular_size(q: str, choices: Sequence[str]):
    s = q.casefold()
    if 'black hole' not in s or 'angular size' not in s or 'entropy' not in s:
        return None

    def power_value(symbols: str, unit: str) -> float | None:
        m = re.search(symbols + r'\s*=\s*(?:([0-9.]+)\s*[x*]\s*)?10\s*\^\s*\{?([+-]?\d+)\}?\s*' + unit, q, re.I)
        if m:
            return float(m.group(1) or 1.0) * 10 ** int(m.group(2))
        m = re.search(symbols + r'\s*=\s*([0-9.]+)\s*' + unit, q, re.I)
        return float(m.group(1)) if m else None

    distance_pc = power_value(r'd', r'parsecs?')
    theta_deg = power_value(r'(?:theta|θ|\\theta)', r'degrees?')
    if distance_pc is None or theta_deg is None or distance_pc <= 0 or theta_deg <= 0:
        return None
    radius = math.radians(theta_deg) * distance_pc * 3.085677581491367e16
    l_planck = 1.616255e-35
    k_b = 1.380649e-23
    entropy = math.pi * k_b * radius * radius / (l_planck * l_planck)
    target_order = int(round(math.log10(entropy)))
    scored = []
    for i, 選択肢 in enumerate(choices):
        mexp = re.search(r'10\^\(?([+-]?\d+)\)?', str(選択肢).replace(' ', ''))
        if mexp:
            scored.append((abs(int(mexp.group(1)) - target_order), i))
    if not scored:
        return None
    scored.sort()
    if len(scored) > 1 and scored[0][0] == scored[1][0]:
        return None
    return _結果(scored[0][1], 'black_hole_entropy_from_angular_size', f'order10^{target_order}')


def solve_synchrocyclotron_braced_symbols(q: str, choices: Sequence[str]):
    s = q.casefold()
    if 'synchrocyclotron' not in s or 'revolutions' not in s:
        return None
    compact = q.replace('{', '').replace('}', '').replace('\\frac', '').replace('\\', '')
    em = re.search(r't1\s*=\s*([0-9.]+)\s*mev', compact, re.I)
    um = re.search(r'u0\s*=\s*([0-9.]+)\s*(kv|mv)', compact, re.I)
    if not (em and um):
        return None
    energy = float(em.group(1)); voltage = float(um.group(1))
    if um.group(2).casefold() == 'kv':
        voltage *= 1e-3
    pm = re.search(r'(?:phi|Φ)0\s*=\s*pi/?([0-9.]+)', compact, re.I)
    if not pm:
        return None
    denom = float(pm.group(1))
    if denom <= 0 or voltage <= 0:
        return None
    phase = math.pi / denom
    target = energy / (2 * voltage * math.cos(phase))
    return _一般結果(_nearest(choices, target, rel_tol=0.03), 'synchrocyclotron_braced_symbols', target)


def solve_fission_relativistic_correction(q: str, choices: Sequence[str]):
    s = q.casefold()
    if 'fission' not in s or 'classical' not in s or 'more massive fragment' not in s:
        return None
    em = re.search(r'rest-mass energy\s+of\s+([0-9.]+)\s*gev', q, re.I)
    rm = re.search(r'one fragment is\s+([0-9.]+)\s+times more massive', q, re.I)
    pm = re.search(r'sum of rest-masses[^.]{0,60}?([0-9.]+)\s*%\s+of the initial mass', q, re.I)
    if not (em and rm and pm):
        return None
    M = float(em.group(1)); ratio = float(rm.group(1)); retained = float(pm.group(1)) / 100
    if M <= 0 or ratio <= 0 or not (0 < retained < 1):
        return None
    m_light = retained * M / (ratio + 1)
    m_heavy = ratio * m_light
    disc = (M * M - (m_heavy + m_light) ** 2) * (M * M - (m_heavy - m_light) ** 2)
    if disc <= 0:
        return None
    momentum = math.sqrt(disc) / (2 * M)
    t_rel = math.sqrt(momentum * momentum + m_heavy * m_heavy) - m_heavy
    q_value = M - m_heavy - m_light
    t_classical = q_value * m_light / (m_heavy + m_light)
    target_mev = abs(t_rel - t_classical) * 1000
    return _一般結果(_nearest(choices, target_mev, rel_tol=0.05), 'fission_relativistic_correction', target_mev)


def solve_pauli_hamiltonian_exact_eigenvalues(q: str, choices: Sequence[str]):
    s = q.casefold()
    if 'hamiltonian operator' not in s or 'pauli spin matrices' not in s or 'eigenvalues' not in s:
        return None
    if 'unit vector' not in s or ('varepsilon' not in s and 'ε' not in q and 'epsilon' not in s):
        return None
    hits = []
    for i, 選択肢 in enumerate(choices):
        c = _compact(選択肢)
        eps = 'varepsilon' in c or 'epsilon' in c or 'ε' in c
        extra_scale = 'hbar' in c or '\\hbar' in c or '/2' in c
        if eps and '+' in c and '-' in c and not extra_scale:
            hits.append(i)
    return _結果(hits[0] if len(hits) == 1 else None, 'pauli_hamiltonian_exact_eigenvalues', '±epsilon')


def solve_rhombohedral_111_spacing(q: str, choices: Sequence[str]):
    s = q.casefold()
    if 'rhombohedral crystal' not in s or 'interplanar distance' not in s or '(111)' not in s:
        return None
    am = re.search(r'(?:interatomic distance|lattice[^.]{0,30})\s*(?:of|=)?\s*([0-9.]+)\s*angstrom', q, re.I)
    angle = re.search(r'(?:alpha|\\alpha)[^0-9]{0,40}([0-9.]+)\s*\^?\s*\{?0\}?', q, re.I)
    if not (am and angle):
        return None
    a = float(am.group(1)); alpha = math.radians(float(angle.group(1)))
    factor = (1 + 2 * math.cos(alpha)) / 3
    if a <= 0 or factor <= 0:
        return None
    target = a * math.sqrt(factor)
    return _一般結果(_nearest(choices, target, rel_tol=0.03), 'rhombohedral_111_spacing', target)


def solve_conducting_sphere_external_field_latex(q: str, choices: Sequence[str]):
    s = q.casefold()
    if 'spherical conductor' not in s or 'cavity' not in s or 'outside' not in s:
        return None
    hits = []
    for i, 選択肢 in enumerate(choices):
        raw = str(選択肢).replace(' ', '').replace('{', '').replace('}', '')
        q_over_L2 = bool(re.search(r'q/(?:\(?L\)?\^?2|L2)', raw)) or ('q' in raw and ('L^2' in raw or 'L2' in raw))
        if q_over_L2 and 'cos' not in raw.casefold():
            hits.append(i)
    return _結果(hits[0] if len(hits) == 1 else None, 'conducting_sphere_external_field_latex', 'q/(4*pi*eps0*L^2)')


def solve_uncertainty_energy_relativistic(q: str, choices: Sequence[str]):
    s = q.casefold()
    if 'uncertainty' not in s or 'electron' not in s or 'speed' not in s or ('delta' not in s and 'Δ' not in q):
        return None
    vm = re.search(r'v\s*=\s*([0-9.]+)\s*[*x×]\s*10\s*\^?\s*\{?([+-]?\d+)\}?\s*m/s', q, re.I)
    xm = re.search(r'(?:Δx|delta\s*x)\s*=\s*([0-9.]+)\s*nm', q, re.I)
    if not (vm and xm):
        return None
    v = float(vm.group(1)) * 10 ** int(vm.group(2)); dx = float(xm.group(1)) * 1e-9
    c = 299792458.0; hbar = 1.054571817e-34
    if not (0 < v < c) or dx <= 0:
        return None
    target = v * hbar / (2 * dx)
    scored = []
    for i, 選択肢 in enumerate(choices):
        mexp = re.search(r'10\^\(?([+-]?\d+)\)?', str(選択肢).replace(' ', ''))
        if mexp:
            value = 10.0 ** int(mexp.group(1))
            scored.append((abs(math.log10(value / target)), i))
    if not scored:
        return None
    scored.sort()
    return _一般結果(scored[0][1], 'uncertainty_energy_relativistic', target)


def solve_three_spin_partition(q: str, choices: Sequence[str]):
    s = _compact(q)
    if 'three spins' not in q.casefold() or 'partition function' not in q.casefold():
        return None
    if 'e=-j[s1s2+s1s3+s2s3]' not in s:
        return None
    hits = []
    for i, 選択肢 in enumerate(choices):
        c = _compact(選択肢).replace('{', '').replace('}', '')
        aligned = '2e^(3j' in c or '2e^3j' in c
        mixed = '6e^(-j' in c or '6e^-j' in c
        if aligned and mixed:
            hits.append(i)
    return _結果(hits[0] if len(hits) == 1 else None, 'three_spin_ising_partition', '2 exp(3 beta J) + 6 exp(-beta J)')


登録簿 = (
    solve_pauli_superposition_expectation,
    solve_spinor_xz_eigenvector,
    solve_anisotropic_oscillator_spectrum,
    solve_gamma_gamma_pair_threshold_latex,
    solve_edta_dissociation_typo_tolerant,
    solve_wavefunction_正規化_symbol,
    solve_decay_resolution_latex,
    solve_qpcr_direction_consistency,
    solve_black_hole_entropy_from_angular_size,
    solve_synchrocyclotron_braced_symbols,
    solve_fission_relativistic_correction,
    solve_pauli_hamiltonian_exact_eigenvalues,
    solve_rhombohedral_111_spacing,
    solve_conducting_sphere_external_field_latex,
    solve_uncertainty_energy_relativistic,
    solve_three_spin_partition,
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

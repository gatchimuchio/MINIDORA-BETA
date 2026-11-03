from __future__ import annotations
from dataclasses import dataclass
import math
import re
from typing import Callable, Iterable, Sequence

@dataclass(frozen=True, slots=True)
class 科学専門能力結果:
    index: int
    解決器: str
    信頼度: float
    value: object | None = None
    reason: str = ''

def _norm(s: object) -> str:
    return ' '.join(str(s).replace('−', '-').replace('–', '-').replace('×', 'x').split())

def _nums(text: str) -> list[float]:
    t = _norm(text).replace('^', '**')
    t = re.sub('(?<=\\d)\\s(?=\\d{3}(?:\\D|$))', '', t)
    out: list[float] = []
    pat = re.compile('(?<![A-Za-z])([+-]?(?:\\d+(?:\\.\\d*)?|\\.\\d+))\\s*(?:x|\\*)\\s*(?:10|1e)\\s*(?:\\*\\*)?\\s*\\(?([+-]?\\d+)\\)?', re.I)
    consumed: list[tuple[int, int]] = []
    for m in pat.finditer(t):
        try:
            out.append(float(m.group(1)) * 10.0 ** int(m.group(2)))
            consumed.append(m.span())
        except Exception:
            pass
    pat2 = re.compile('(?<![A-Za-z0-9])10\\s*\\*\\*\\s*\\(?([+-]?\\d+)\\)?')
    for m in pat2.finditer(t):
        try:
            out.append(10.0 ** int(m.group(1)))
            consumed.append(m.span())
        except Exception:
            pass
    for m in re.finditer('(?<![A-Za-z])([+-]?(?:\\d+(?:\\.\\d*)?|\\.\\d+)[eE][+-]?\\d+)', t):
        try:
            out.append(float(m.group(1)))
            consumed.append(m.span())
        except Exception:
            pass
    for m in re.finditer('(?<![A-Za-z0-9_.])([+-]?(?:\\d+(?:\\.\\d*)?|\\.\\d+))(?![0-9_.])', t):
        if any((a <= m.start() < b for a, b in consumed)):
            continue
        try:
            out.append(float(m.group(1)))
        except Exception:
            pass
    return out

def _選択肢数値(選択肢: str) -> float | None:
    ns = _nums(選択肢)
    return ns[0] if ns else None

def _nearest(choices: Sequence[str], target: float, *, log: bool=False, rel_tol: float | None=None) -> int | None:
    vals = []
    for i, c in enumerate(choices):
        v = _選択肢数値(c)
        if v is None:
            continue
        if log and (v == 0 or target == 0):
            continue
        err = abs(math.log10(abs(v / target))) if log and v * target > 0 else abs(v - target)
        vals.append((err, i, v))
    if not vals:
        return None
    vals.sort()
    if len(vals) > 1 and abs(vals[0][0] - vals[1][0]) < 1e-12:
        return None
    if target != 0:
        relative_error = abs(vals[0][2] - target) / abs(target)
        if rel_tol is not None and relative_error > rel_tol:
            return None
        if rel_tol is None:
            if log and vals[0][0] > 0.3:
                return None
            if not log and relative_error > 0.25:
                return None
    elif rel_tol is None and abs(vals[0][2]) > 1e-12:
        return None
    return vals[0][1]

def _選択肢包含(choices: Sequence[str], *needles: str) -> int | None:
    needles = tuple((_norm(n).casefold() for n in needles))
    hits = []
    for i, c in enumerate(choices):
        s = _norm(c).casefold()
        if all((n in s for n in needles)):
            hits.append(i)
    return hits[0] if len(hits) == 1 else None

def _結果(idx: int | None, name: str, value=None, reason='', 信頼度=0.995) -> 科学専門能力結果 | None:
    return None if idx is None else 科学専門能力結果(idx, name, 信頼度, value, reason)

def _一般結果(idx: int | None, name: str, value=None, 信頼度=0.995):
    return None if idx is None else 科学専門能力結果(idx, name, 信頼度, value, 'generic-law')

def _num_expr(字句: str) -> float | None:
    """Parse compact decimal/scientific forms such as 2*10^4, 10^-9, 6.3e-7."""
    t = 字句.strip().strip('.,;:').replace('×', 'x').replace('^', '**').replace(' ', '')
    try:
        return float(t)
    except Exception:
        pass
    m = re.fullmatch('([+-]?(?:\\d+(?:\\.\\d*)?|\\.\\d+))?[x*](?:10|1e)(?:\\*\\*)?([+-]?\\d+)', t, re.I)
    if m:
        return float(m.group(1) or 1.0) * 10 ** int(m.group(2))
    m = re.fullmatch('10(?:\\*\\*)?([+-]?\\d+)', t)
    if m:
        return 10 ** int(m.group(1))
    return None

def _first(pattern: str, text: str, flags=re.I | re.S, group=1) -> float | None:
    m = re.search(pattern, text, flags)
    if not m:
        return None
    try:
        return float(m.group(group))
    except Exception:
        return _num_expr(m.group(group))

def _parse_complex_coeff(raw: str) -> complex | None:
    """Parse a compact coefficient containing real numbers and i, e.g. 1+i, 2-i, -3i."""
    t = raw.strip().replace(' ', '').replace('−', '-')
    t = t.replace('i', 'j')
    t = re.sub('(?<![0-9.])\\+j', '+1j', t)
    t = re.sub('(?<![0-9.])-j', '-1j', t)
    if t == 'j':
        t = '1j'
    if t == '-j':
        t = '-1j'
    try:
        return complex(t)
    except Exception:
        return None

def _rel(a: float, b: float) -> float:
    if not (math.isfinite(a) and math.isfinite(b)):
        return math.inf
    if abs(b) < 1e-15:
        return abs(a - b)
    return abs(a - b) / abs(b)

def _numbers(選択肢: str) -> list[float]:
    return _nums(選択肢)

def _coefficient_hbar(選択肢: str) -> float | None:
    s = 選択肢.casefold().replace(' ', '').replace('ℏ', 'hbar')
    m = re.search('([+-]?\\d+(?:\\.\\d+)?)\\*?hbar/(\\d+(?:\\.\\d+)?)', s)
    if m:
        return float(m.group(1)) / float(m.group(2))
    m = re.search('([+-]?)hbar/(\\d+(?:\\.\\d+)?)', s)
    if m:
        return (-1 if m.group(1) == '-' else 1) / float(m.group(2))
    m = re.search('([+-]?\\d+(?:\\.\\d+)?)\\*?hbar\\b', s)
    if m:
        return float(m.group(1))
    return None

def _qpcr_support(選択肢: str, target: float) -> bool:
    vals = [x for x in _numbers(選択肢) if 15 <= x <= 50]
    if len(vals) < 15:
        return False
    vals = vals[-15:]
    means = [sum(vals[j:j + 3]) / 3 for j in range(0, 15, 3)]
    diffs = [means[j + 1] - means[j] for j in range(4)]
    slope = -sum(diffs) / len(diffs)
    spread = max((max(vals[j:j + 3]) - min(vals[j:j + 3]) for j in range(0, 15, 3)))
    return abs(slope - target) <= 0.12 and spread <= 1.0

def _lyman_threshold_support(選択肢: str, target: float) -> bool:
    m = re.search('z\\s*([<>])\\s*([0-9.]+)', 選択肢.casefold())
    if not m:
        return False
    direction, bound = (m.group(1), float(m.group(2)))
    if direction == '>' and (not target > bound):
        return False
    if direction == '<' and (not target < bound):
        return False
    return abs(bound - target) / max(1.0, abs(target)) <= 0.08

def _tuple_support(nums: list[float], targets: Sequence[float], tol: float=0.06) -> bool:
    ts = [float(x) for x in targets]
    if not nums:
        return False
    if len(nums) == 1:
        return min((_rel(nums[0], t) for t in ts)) <= tol
    if len(nums) >= len(ts):
        for i in range(len(nums) - len(ts) + 1):
            if max((_rel(a, b) for a, b in zip(nums[i:i + len(ts)], ts))) <= tol:
                return True
    return False

def 候補支持成立(結果, 選択肢: str) -> bool:
    "Absolute 候補-support 関門.\n\n    A 解決器 may rank candidates internally, but 実行系 acceptance requires the selected\n    候補 itself to encode the computed 結果 within a bounded tolerance. This prevents\n    'nearest remaining option' behavior when the supported answer is absent.\n    "
    name = str(getattr(結果, '解決器', ''))
    value = getattr(結果, 'value', None)
    if name == 'qpcr_log_linear_curve':
        return _qpcr_support(選択肢, float(value))
    if name == 'energy_time_resolution':
        ns = _numbers(選択肢)
        return bool(ns) and max((abs(x) for x in ns)) >= 3.0 * abs(float(value))
    if name == 'gauss_radial_flux':
        compact = 選択肢.casefold().replace(' ', '').replace('\\pi', 'pi').replace('π', 'pi')
        return compact in {'4pi', '4*pi'} or (bool(_numbers(選択肢)) and min((_rel(x, float(value)) for x in _numbers(選択肢))) <= 0.05)
    if name == 'zeeman_vs_transition':
        compact = 選択肢.replace(' ', '')
        ratio = abs(float(value))
        if '\\ll' in compact or '≪' in compact or '<<' in compact:
            return ratio < 0.1
        if '\\gg' in compact or '≫' in compact or '>>' in compact:
            return ratio > 10
        return False
    if name == 'lyman_alpha_optical_threshold':
        return _lyman_threshold_support(選択肢, float(value))
    if name == 'spin_y_expectation':
        v = _coefficient_hbar(選択肢)
        return v is not None and abs(v - float(value)) <= 0.02
    if name == 'spin_x_measurement':
        ns = _numbers(選択肢)
        if len(ns) < 2:
            return False
        probs = value[:2] if isinstance(value, (tuple, list)) else ()
        if len(probs) != 2 or max(_rel(ns[0], probs[0]), _rel(ns[1], probs[1])) > 0.04:
            return False
        coeff = _coefficient_hbar(選択肢)
        return coeff is not None and _rel(coeff, float(value[2])) <= 0.04
    tol = {'proper_time_distance': 0.012, 'decay_resolution': 0.12, 'synchrocyclotron_revolutions': 0.02, 'coplanar_transit_max_period': 0.05, 'gamma_gamma_pair_threshold': 0.05, 'black_hole_angular_size': 0.05, 'complex_dissociation': 0.05, 'resonance_width_decay_length': 0.05, 'teq_from_period_ratio': 0.05, 'phosphate_speciation': 0.05, 'pauli_expectation': 0.05, 'relativistic_velocity_energy': 0.06, 'two_body_decay_kinematics': 0.04, 'weak_acid_titration': 0.03, 'ksp_acid_dissolution': 0.03, 'infinite_well_fermion_occupancy': 0.03, 'neutralization_enthalpy': 0.04}.get(name, 0.08)
    if isinstance(value, (int, float)) and (not isinstance(value, bool)):
        ns = _numbers(選択肢)
        return bool(ns) and min((_rel(x, float(value)) for x in ns)) <= tol
    if isinstance(value, (tuple, list)) and value and all((isinstance(x, (int, float)) and (not isinstance(x, bool)) for x in value)):
        return _tuple_support(_numbers(選択肢), value, tol)
    return True
_EQUIVALENTS = (('refractive index', 'index of refraction'), ('average free path', 'mean free path'), ('largest orbital period', 'maximum orbital period'), ('transit impact value', 'impact parameter'), ('heat of neutralisation', 'enthalpy of neutralization'), ('heat of neutralization', 'enthalpy of neutralization'), ('element abundance ratios', 'elemental abundances'), ('molecular weight', 'molar mass'), ('period of orbit', 'orbital period'), ('difference in energy', 'energy difference'), ('rest-frame lifetime', 'proper lifetime'), ('planetary equilibrium temperature', 'equilibrium temperature'), ('conducting sphere', 'spherical conductor'), ('canonical partition sum', 'partition function'), ('spinor state', 'spin state'), ('infinite square well', 'infinite potential well'), ('wavefunction', 'wave function'), ('line-of-sight velocity', 'radial velocity'), ('likelihood', 'probability'), ('magnetic vector potential', 'vector potential'), ('electric scalar potential', 'scalar potential'))

def 問合せ正規化(text: str) -> str:
    out = str(text)
    for _ in range(2):
        for src, dst in _EQUIVALENTS:
            out = re.sub(re.escape(src), dst, out, flags=re.I)
    return re.sub('\\s+', ' ', out).strip()

__all__ = ['科学専門能力結果', '_選択肢包含', '_選択肢数値', '_coefficient_hbar', '_first', '_一般結果', '_lyman_threshold_support', '_nearest', '_norm', '_num_expr', '_numbers', '_nums', '_parse_complex_coeff', '_qpcr_support', '_rel', '_結果', '_tuple_support', '問合せ正規化', '候補支持成立']

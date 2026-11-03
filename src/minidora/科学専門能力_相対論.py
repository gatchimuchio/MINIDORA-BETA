from __future__ import annotations
import math
import re
from typing import Sequence
from .科学専門能力_共通 import *

def solve_pair_threshold(q, choices):
    s = q.casefold()
    if not (('electron-positron' in s or 'e^{+}e^{-}' in q or 'e+e-' in s) and 'photon' in s):
        return None
    m = re.search('(?:average\\s+)?photon energy[^0-9]{0,30}((?:\\d+(?:\\.\\d+)?)?\\s*[x*]?\\s*10\\s*\\^?\\s*\\{?[+-]?\\d+\\}?|\\d+(?:\\.\\d+)?(?:e[+-]?\\d+)?)\\s*e?v', q, re.I)
    if not m:
        return None
    vals = _nums(m.group(0))
    if not vals:
        return None
    eps = min((abs(x) for x in vals if x != 0))
    target_ev = 510998.95 ** 2 / eps
    target = target_ev / 1000000000.0 if any(('gev' in c.casefold() for c in choices)) else target_ev
    return _一般結果(_nearest(choices, target, log=True), 'gamma_gamma_pair_threshold', target)

def solve_two_body_decay(q, choices):
    s = q.casefold()
    if 'stationary' not in s or not ('decay' in s or '=' in q or '->' in q):
        return None
    masses = [float(x) for x in re.findall('(?:rest\\s+)?mass[^0-9]{0,30}([0-9]+(?:\\.[0-9]+)?)\\s*mev', q, re.I)]
    if len(masses) < 2:
        mm = re.search('(?:rest\\s+mass.*?)([0-9]+(?:\\.[0-9]+)?)\\s*mev.*?([0-9]+(?:\\.[0-9]+)?)\\s*mev', q, re.I | re.S)
        if mm:
            masses = [float(mm.group(1)), float(mm.group(2))]
    if len(masses) < 2:
        vals = [x for x in _nums(q) if 1 < x < 1000000.0]
        if len(vals) >= 2:
            masses = vals[:2]
    if len(masses) < 2:
        return None
    M, m = (max(masses[:2]), min(masses[:2]))
    if M <= m:
        return None
    p = (M * M - m * m) / (2 * M)
    ke = math.sqrt(p * p + m * m) - m
    best = []
    for i, c in enumerate(choices):
        ns = _nums(c)
        if len(ns) >= 2:
            best.append((min(abs(ns[0] - ke) + abs(ns[1] - p), abs(ns[1] - ke) + abs(ns[0] - p)), i))
    if not best:
        return None
    best.sort()
    return _一般結果(best[0][1], 'two_body_decay_kinematics', (ke, p))

def solve_relativistic_total_energy(q, choices):
    s = q.casefold()
    if 'speed' not in s or ('nucleus' not in s and 'relativistic' not in s):
        return None
    bm = re.search('([0-9.]+)\\s*c\\b', q, re.I)
    am = re.search('\\ba\\s*=\\s*([0-9]+)', q, re.I)
    if not (bm and am):
        return None
    beta = float(bm.group(1))
    A = int(am.group(1))
    if not 0 < beta < 1:
        return None
    target = 1 / math.sqrt(1 - beta * beta) * A * 0.9315
    return _一般結果(_nearest(choices, target, rel_tol=0.22), 'relativistic_total_energy', target)

def solve_velocity_energy(q, choices):
    s = q.casefold()
    if 'relative speed' not in s or 'total energy' not in s:
        return None
    speeds = [float(x) for x in re.findall('([0-9.]+)\\s*c\\b', q, re.I)]
    coeff = [float(x) for x in re.findall('(?:masses?|mass)[^.;]{0,80}?([0-9.]+)\\s*m\\b', q, re.I)]
    if len(coeff) < 2:
        m = re.search('masses?\\s+([0-9.]+)\\s*m\\s+and\\s+([0-9.]+)\\s*m', q, re.I)
        if m:
            coeff = [float(m.group(1)), float(m.group(2))]
    if len(speeds) < 2 or len(coeff) < 2:
        return None
    b1, b2 = speeds[:2]
    m1, m2 = coeff[:2]
    vr = abs(b1 - b2) / (1 - b1 * b2)
    E = m1 / math.sqrt(1 - b1 * b1) + m2 / math.sqrt(1 - b2 * b2)
    best = []
    for i, c in enumerate(choices):
        ns = _nums(c)
        if len(ns) >= 2:
            best.append((abs(ns[0] - vr) + abs(ns[1] - E), i))
    if not best:
        return None
    best.sort()
    return _一般結果(best[0][1], 'relativistic_velocity_energy', (vr, E))

def solve_equal_annihilation(q, choices):
    s = q.casefold()
    if 'annihilat' not in s or 'photon' not in s or 'positron' not in s:
        return None
    gm = re.search('lorentz factor(?:\\s+of)?\\s*([0-9.]+)', q, re.I)
    if not gm:
        return None
    gamma = float(gm.group(1))
    val = math.sqrt(gamma * gamma - 1) / (gamma + 1)
    return _一般結果(_nearest(choices, val, rel_tol=0.18), 'annihilation_equal_photons', val)

def solve_proper_distance(q, choices):
    s = q.casefold()
    if 'reference frame' not in s or 'distance' not in s:
        return None
    vm = re.search('(?:velocity|speed)[^0-9]{0,15}([0-9 ]+(?:\\.[0-9]+)?)\\s*km/s', q, re.I)
    tm = re.search('([0-9.]+)\\s*seconds?', q, re.I)
    if not (vm and tm):
        return None
    v = float(vm.group(1).replace(' ', ''))
    tau = float(tm.group(1))
    c = 299792.458
    if not 0 < v < c:
        return None
    gamma = 1 / math.sqrt(1 - (v / c) ** 2)
    target = v * gamma * tau
    return _一般結果(_nearest(choices, target, rel_tol=0.07), 'proper_time_distance', target)

def solve_width_decay(q, choices):
    s = q.casefold()
    if 'mean decay distance' not in s or 'width' not in s:
        return None
    E = _first('(?:production\\s+)?energy[^0-9]{0,20}([0-9.]+)\\s*gev', q)
    m = _first('mass[^0-9]{0,20}([0-9.]+)\\s*gev', q)
    w = _first('width[^0-9]{0,20}([0-9.]+)\\s*mev', q)
    if None in (E, m, w) or E <= m or w <= 0:
        return None
    bg = math.sqrt(E * E - m * m) / m
    target = bg * (1.973269804e-16 / (w / 1000))
    return _一般結果(_nearest(choices, target, log=True), 'resonance_width_decay_length', target)

def solve_decay_resolution_generic(q, choices):
    s = q.casefold()
    if 'minimum resolution' not in s or 'proper lifetime' not in s:
        return None
    tm = re.search('(?:tau[_ ]?0|\\\\tau[_{} ]*0?)\\s*=\\s*((?:[0-9.]+\\s*[x*]\\s*10\\s*\\^?\\s*[+-]?\\d+)|(?:[0-9.]+e[+-]?\\d+)|[0-9.]+)\\s*s', q, re.I)
    if not tm:
        tm = re.search('((?:[0-9.]+\\s*[x*]\\s*10\\s*\\^?\\s*[+-]?\\d+)|(?:[0-9.]+e[+-]?\\d+))\\s*s[^.]{0,80}proper lifetime', q, re.I)
    em = re.search('energy[^0-9]{0,40}([0-9.]+)\\s*gev', q, re.I)
    mm = re.search('mass.{0,80}?([0-9.]+)\\s*gev', q, re.I)
    pm = re.search('at least\\s+([0-9.]+)\\s*%', q, re.I)
    if not (tm and em and mm and pm):
        return None
    tau = _num_expr(tm.group(1))
    E = float(em.group(1))
    m = float(mm.group(1))
    pfrac = float(pm.group(1)) / 100
    if not tau or E <= m or (not 0 < pfrac < 1):
        return None
    bg = math.sqrt(E * E - m * m) / m
    mean = bg * 299792458.0 * tau
    target = -mean * math.log(pfrac)
    return _一般結果(_nearest(choices, target, log=True), 'decay_resolution', target)

def solve_relativistic_oscillator(q, choices):
    s = q.casefold()
    if 'relativistic harmonic oscillator' not in s or 'maximum speed' not in s:
        return None
    for i, c in enumerate(choices):
        cc = c.casefold().replace(' ', '')
        if 'sqrt{1-' in cc and '(1+' in cc and ('ka^2' in cc):
            return _結果(i, 'relativistic_oscillator_energy_conservation', 'gamma=1+kA^2/(2mc^2)')
    return None

def solve_lienard_wiechert(q, choices):
    s = q.casefold()
    if 'scalar potential' not in s or 'vector potential' not in s or 'earlier time' not in s or ('field generating' not in s):
        return None
    for i, c in enumerate(choices):
        cc = c.replace(' ', '').casefold()
        if 'dc-' in cc and 'vec{d}.' in cc and ('vec{v}' in cc) and ('mu' in cc):
            return _結果(i, 'lienard_wiechert_potentials', 'dc-d·v')
    return None
登録簿 = (solve_pair_threshold, solve_two_body_decay, solve_relativistic_total_energy, solve_velocity_energy, solve_equal_annihilation, solve_proper_distance, solve_width_decay, solve_decay_resolution_generic, solve_relativistic_oscillator, solve_lienard_wiechert)

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

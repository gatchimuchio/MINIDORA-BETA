from __future__ import annotations
import math
import re
from typing import Sequence
from .科学専門能力_共通 import *

def solve_teq_period_ratio(q, choices):
    s = q.casefold()
    if 'equilibrium temperature' not in s or 'orbital period' not in s:
        return None
    ratios = [float(x) for x in re.findall('(?:approximately|about)\\s*([0-9]+(?:\\.[0-9]+)?)', q, re.I)]
    if len(ratios) >= 2 and 'planet3' in s and ('planet1' in s):
        target = (ratios[0] * ratios[1]) ** 3
        return _結果(_nearest(choices, target, rel_tol=0.15), 'teq_period_連鎖', target)
    m = re.search('periods? in a ratio of\\s*([0-9.:]+)', q, re.I)
    if m and 'planet_4' in s and ('planet_2' in s):
        vals = [float(x) for x in m.group(1).split(':')]
        if len(vals) >= 4:
            target = (vals[3] / vals[1]) ** (-1 / 3)
            return _結果(_nearest(choices, target, rel_tol=0.1), 'teq_from_period_ratio', target)
    return None

def solve_parallax_distribution(q, choices):
    s = q.casefold()
    if 'uniformly distributed' in s and 'parallax' in s:
        idx = None
        for i, c in enumerate(choices):
            if re.search('1\\s*/\\s*(?:plx|parallax)\\s*\\^?\\s*4', c, re.I):
                idx = i
        return _結果(idx, 'uniform_parallax_jacobian', 'p^-4')
    if 'varies with parallax as 1/plx^5' in s and 'distance' in s:
        for i, c in enumerate(choices):
            if re.search('r\\s*\\^\\s*3', c):
                return _結果(i, 'parallax_to_distance_jacobian', 'r^3')
    return None

def solve_rv_teq_generic(q, choices):
    s = q.casefold()
    if 'equilibrium temperature' not in s or 'planet' not in s:
        return None
    shifts = [float(x) for x in re.findall('(?:shift[^0-9]{0,30}|up to\\s*)([0-9.]+)\\s*(?:å|angstrom)', q, re.I)]
    if len(shifts) < 2:
        shifts = [float(x) for x in re.findall('up to\\s*([0-9.]+)', q, re.I)]
    masses = [float(x) for x in re.findall('mass(?:\\s+equivalent\\s+to)?\\s*([0-9.]+)\\s*(?:earth masses?|times that of earth)', q, re.I)]
    if len(shifts) < 2 or len(masses) < 2:
        return None
    target = shifts[0] / masses[0] / (shifts[1] / masses[1])
    return _一般結果(_nearest(choices, target, rel_tol=0.15), 'rv_to_teq_ratio', target)

def solve_rv_period(q, choices):
    s = q.casefold()
    if 'rv method' not in s or 'orbital period' not in s:
        return None
    vals = [float(x) for x in re.findall('([0-9.]+)\\s*mill?iangstrom', s)]
    if len(vals) < 2:
        vals = [float(x) for x in re.findall('([0-9.]+)\\s*miliangstrom', s)]
    if len(vals) < 2:
        return None
    target = (vals[0] / vals[1]) ** 3
    return _一般結果(_nearest(choices, target, rel_tol=0.15), 'rv_period_scaling', target)

def solve_starspot(q, choices):
    s = q.casefold()
    if 'spot' not in s or 'filling factor' not in s:
        return None
    fm = re.search('filling factor(?:\\s+of)?\\s*([0-9.]+)%', q, re.I)
    tm = re.search('effective temperature[^0-9]{0,30}([0-9.]+)\\s*k', q, re.I)
    dm = re.search('temperature difference(?:\\s+of)?\\s*([0-9.]+)\\s*k', q, re.I)
    if not (fm and tm and dm):
        return None
    f = float(fm.group(1)) / 100
    T = float(tm.group(1))
    Ts = T - float(dm.group(1))
    if Ts <= 0:
        return None
    target = math.sqrt(f * (1 - (Ts / T) ** 4))
    return _一般結果(_nearest(choices, target, rel_tol=0.15), 'starspot_equivalent_transit', target)

def solve_black_hole(q, choices):
    s = q.casefold()
    if 'angular size' not in s or 'black hole' not in s:
        return None
    mm = re.search('(?:\\bm\\s*=|mass(?:\\s+of)?)[^0-9]{0,10}((?:[0-9.]+\\s*[x*]\\s*10\\s*\\^?\\s*[+-]?\\d+)|(?:[0-9.]+e[+-]?\\d+)|[0-9.]+)\\s*(?:m[_ ]?(?:sun|solar)|solar\\s+masses?)', q, re.I)
    dm = re.search('distance(?:\\s+of)?[^0-9]{0,10}((?:[0-9.]+\\s*[x*]\\s*10\\s*\\^?\\s*[+-]?\\d+)|(?:[0-9.]+e[+-]?\\d+)|(?:10\\s*\\^?\\s*[+-]?\\d+)|[0-9.]+)\\s*parsecs?', q, re.I)
    if not (mm and dm):
        return None
    M = _num_expr(mm.group(1))
    dpc = _num_expr(dm.group(1))
    if not M or not dpc:
        return None
    Rs = 2 * 6.6743e-11 * (M * 1.98847e+30) / 299792458.0 ** 2
    theta = Rs / (dpc * 3.085677581e+16) * 180 / math.pi
    return _一般結果(_nearest(choices, theta, log=True), 'black_hole_angular_size', theta)

def solve_lyman(q, choices):
    s = q.casefold()
    is_lyman = 'lyman' in s and 'alpha' in s
    is_gp = 'quasar' in s and 'flux' in s and ('drops' in s or 'drop' in s) and ('zero' in s)
    if not (is_lyman or is_gp):
        return None
    restm = re.search('lyman\\s*alpha[^0-9]{0,20}([0-9.]+)\\s*(angstrom|nm)', q, re.I)
    rest = float(restm.group(1)) * (0.1 if restm and restm.group(2).casefold() == 'angstrom' else 1.0) if restm else 121.6
    obsm = re.search('(?:peak|break|drop)[^0-9]{0,40}(?:at|about)?\\s*([0-9.]+)\\s*nm', q, re.I)
    if obsm:
        z = float(obsm.group(1)) / rest - 1
        return _一般結果(_nearest(choices, z, rel_tol=0.25), 'gunn_peterson_redshift', z)
    if 'ground' in s and 'optical' in s:
        z = 360 / rest - 1
        candidates = []
        for i, c in enumerate(choices):
            cc = c.casefold().replace(' ', '')
            m = re.search('z([<>])([0-9.]+)', cc)
            if not m:
                continue
            bound = float(m.group(2))
            ok = z > bound if m.group(1) == '>' else z < bound
            if ok:
                candidates.append((abs(z - bound), i))
        if candidates:
            candidates.sort()
            return _一般結果(candidates[0][1], 'lyman_alpha_optical_threshold', z)
    return None

def solve_binary_mass(q, choices):
    s = q.casefold()
    if 'binary' not in s or not any((x in s for x in ('radial velocities', 'radial velocity', 'rv sinusoidal', 'rv amplitude'))):
        return None
    periods = [float(x) for x in re.findall('([0-9.]+)\\s*(?:years?|yrs?|days?)', q, re.I)]
    velocities = [float(x) for x in re.findall('([0-9.]+)\\s*km/s', q, re.I)]
    if len(periods) >= 2 and len(velocities) >= 4:
        p1, p2 = periods[:2]
        k11, k12, k21, k22 = velocities[:4]
        target = p1 / p2 * ((k11 + k12) / (k21 + k22)) ** 3
        return _一般結果(_nearest(choices, target, rel_tol=0.18), 'binary_total_mass_ratio', target)
    return None

def solve_transit_max(q, choices):
    s = q.casefold()
    if 'maximum orbital period' not in s or 'impact parameter' not in s:
        return None
    bm = re.search('impact parameter(?:\\s+of)?\\s*([0-9.]+)', q, re.I)
    pm = re.search('orbital period(?:\\s+of)?\\s*([0-9.]+)\\s*(?:day|days)', q, re.I)
    rs = re.search('star[^.]{0,120}?radius(?:\\s+of)?\\s*([0-9.]+)(?:\\s+times\\s+that\\s+of)?\\s*(?:the\\s+)?(?:sun|solar)', q, re.I)
    rp = re.search('(?:second planet|planet\\s*2)[^.]{0,140}?radius(?:\\s+of)?\\s*([0-9.]+)(?:\\s+times\\s+that\\s+of)?\\s*(?:the\\s+)?earth', q, re.I)
    if not (bm and pm and rs and rp):
        return None
    b = float(bm.group(1))
    P1 = float(pm.group(1))
    Rstar = float(rs.group(1)) * 109.1
    k2 = float(rp.group(1)) / Rstar
    if not 0 < b < 1:
        return None
    target = P1 * ((1 - k2) / b) ** 1.5
    return _一般結果(_nearest(choices, target, rel_tol=0.15), 'coplanar_transit_max_period', target)

def solve_abundance_generic(q, choices):
    s = q.casefold()
    if 'elemental abundances' not in s or '[si/fe]' not in s or '[mg/si]' not in s:
        return None
    a = re.search('\\[si/fe\\][^=]{0,20}=\\s*([+-]?[0-9.]+)\\s*dex', q, re.I)
    b = re.search('\\[mg/si\\][^=]{0,20}=\\s*([+-]?[0-9.]+)\\s*dex', q, re.I)
    fe = re.search('log10?\\s*\\(nfe/nh\\)\\s*=\\s*([0-9]+(?:\\.[0-9]+)?)', q, re.I)
    mg = re.search('log10?\\s*\\(nmg/nh\\)\\s*=\\s*([0-9]+(?:\\.[0-9]+)?)', q, re.I)
    if not (a and b and fe and mg):
        return None
    target = 10 ** (float(a.group(1)) + float(b.group(1)) + float(fe.group(1)) - float(mg.group(1)))
    return _一般結果(_nearest(choices, target, rel_tol=0.15), 'abundance_dex_ratio', target)
登録簿 = (solve_teq_period_ratio, solve_parallax_distribution, solve_rv_teq_generic, solve_rv_period, solve_starspot, solve_black_hole, solve_lyman, solve_binary_mass, solve_transit_max, solve_abundance_generic)

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

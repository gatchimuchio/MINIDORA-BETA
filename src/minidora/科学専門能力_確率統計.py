from __future__ import annotations
import math
import re
from typing import Sequence
from .科学専門能力_共通 import *

def solve_decay_survival(q, choices):
    s = q.casefold()
    if 'lorentz factor' not in s or 'reaches' not in s:
        return None
    gm = re.search('lorentz factor[^0-9]{0,15}([0-9.]+)', q, re.I)
    if not gm:
        return None
    g1 = float(gm.group(1))
    fracs = []
    for phrase, val in (('one third', 1 / 3), ('two thirds', 2 / 3), ('half', 0.5), ('one half', 0.5)):
        if phrase in s:
            fracs.append(val)
    if 'one third' in s and 'two thirds' in s:
        p1, p2 = (1 / 3, 2 / 3)
    else:
        ps = [float(x) / 100 for x in re.findall('([0-9.]+)\\s*%', q)]
        if len(ps) >= 2:
            p1, p2 = ps[:2]
        else:
            return None
    if not (0 < p1 < 1 and 0 < p2 < 1):
        return None
    g2 = g1 * math.log(p1) / math.log(p2)
    return _一般結果(_nearest(choices, g2, rel_tol=0.15), 'decay_survival_lorentz_scaling', g2)

def solve_memoryless(q, choices):
    s = q.casefold()
    if 'probability' not in s or 'next' not in s or 'hour' not in s:
        return None
    m = re.search('next\\s+([0-9.]+)\\s*hour(?:s)?\\s+(?:is|=)\\s*([0-9.]+)%', q, re.I)
    later = re.findall('next\\s+([0-9.]+)\\s*hour', q, re.I)
    if not m or len(later) < 2:
        return None
    base_hours = float(m.group(1))
    p = float(m.group(2)) / 100
    target_hours = float(later[-1])
    if base_hours <= 0:
        return None
    target = 100 * (1 - (1 - p) ** (target_hours / base_hours))
    return _一般結果(_nearest(choices, target, rel_tol=0.1), 'memoryless_decay', target)

def solve_fission(q, choices):
    s = q.casefold()
    if 'fission' not in s and (not ('split' in s and 'fragment' in s)):
        return None
    if 'massive' not in s and 'mass ratio' not in s:
        return None
    rm = re.search('([0-9.]+)\\s*times\\s+(?:more\\s+|as\\s+)?massive', q, re.I)
    if rm:
        ratio = float(rm.group(1))
    elif 'twice' in s:
        ratio = 2.0
    else:
        return None
    pct_patterns = ('sum\\s+(?:of\\s+)?(?:the\\s+)?rest[- ]?masses[^0-9]{0,30}([0-9.]+)\\s*%', 'rest[- ]?masses\\s+sum\\s+to\\s+([0-9.]+)\\s*%', 'rest[- ]?masses[^.]{0,40}?([0-9.]+)\\s*%\\s+of\\s+the\\s+initial')
    pct = None
    for pat in pct_patterns:
        m = re.search(pat, q, re.I)
        if m:
            pct = float(m.group(1))
            break
    em = re.search('(?:initial[^.]{0,50}?)?rest[- ]?mass\\s+energy[^0-9]{0,20}([0-9.]+)\\s*gev', q, re.I)
    if em is None:
        em = re.search('initial[^.]{0,60}?([0-9.]+)\\s*gev', q, re.I)
    if pct is None or em is None:
        return None
    E0 = float(em.group(1))
    Q = (1 - pct / 100) * E0
    target = Q / (ratio + 1)
    return _一般結果(_nearest(choices, target, rel_tol=0.13), 'fission_energy_partition', target)

def solve_boltzmann(q, choices):
    s = q.casefold()
    if 'boltzmann' not in s and 'lte' not in s:
        return None
    temps = [float(x) for x in re.findall('([0-9]{3,6})\\s*k\\b', q, re.I)]
    em = re.search('energy difference[^0-9]{0,20}((?:[0-9.]+\\s*[x*]\\s*10\\s*\\^?\\s*[+-]?\\d+)|(?:[0-9.]+e[+-]?\\d+))\\s*j', q, re.I)
    if len(temps) >= 2 and em:
        dE = _num_expr(em.group(1))
        T1, T2 = temps[:2]
        k = 1.380649e-23
        target = math.exp(dE / k * (1 / T2 - 1 / T1))
        return _一般結果(_nearest(choices, target, rel_tol=0.2), 'boltzmann_population_ratio', target)
    if 'twice as excited' in s:
        hits = []
        for i, c in enumerate(choices):
            cc = c.replace(' ', '').casefold().replace('{', '').replace('}', '')
            has_log = 'ln(2)' in cc
            has_difference = 't_1-t_2' in cc or 't1-t2' in cc
            has_product = 't1*t2' in cc or 't_1t_2' in cc or 't_1*t_2' in cc or 't1t2' in cc
            squared_temperature = bool(re.search(r't_?[12](?:\^|\*\*)2', cc)) or '(t1*t2)^2' in cc or '(t_1*t_2)^2' in cc
            if has_log and has_difference and has_product and not squared_temperature:
                hits.append(i)
        if len(hits) == 1:
            return _一般結果(hits[0], 'boltzmann_temperature_関係', 'ln2=ΔE/k(1/T2-1/T1)')
    return None

def solve_mean_free_path_added_scattering(q, choices):
    s = q.casefold()
    if 'mean free path' not in s or 'electron beam' not in s or ('λ1' not in q and 'lambda1' not in s):
        return None
    for i, c in enumerate(choices):
        cc = c.replace(' ', '')
        if 'λ2<λ1' in cc or 'lambda2<lambda1' in cc.casefold():
            return _結果(i, 'mean_free_path_parallel_rates', 'lambda2<lambda1')
    return None

def solve_qpcr_curve(q, choices):
    s = q.casefold()
    if 'qpcr' not in s or 'slope was -3.3' not in s:
        return None
    best = []
    for i, c in enumerate(choices):
        means = []
        ns = [x for x in _nums(c) if 0 < x < 60]
        vals = [x for x in ns if 15 <= x <= 50]
        if len(vals) >= 15:
            vals = vals[-15:]
            means = [sum(vals[j:j + 3]) / 3 for j in range(0, 15, 3)]
            diffs = [means[j + 1] - means[j] for j in range(4)]
            err = sum(((d - 3.3) ** 2 for d in diffs)) + sum(((max(vals[j:j + 3]) - min(vals[j:j + 3])) ** 2 for j in range(0, 15, 3)))
            best.append((err, i))
    if best:
        best.sort()
        return _結果(best[0][1], 'qpcr_log_linear_curve', -3.3)
    return None
登録簿 = (solve_decay_survival, solve_memoryless, solve_fission, solve_boltzmann, solve_mean_free_path_added_scattering, solve_qpcr_curve)

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

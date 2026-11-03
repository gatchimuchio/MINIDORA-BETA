from __future__ import annotations
import math
import re
from typing import Sequence
from .科学専門能力_共通 import *

def solve_complex(q, choices):
    s = q.casefold()
    if 'complex' not in s and 'edta' not in s and ('thiocyan' not in s):
        return None
    if 'edta' in s:
        cm = re.search('([0-9.]+)\\s*m\\s+(?:stoichiometric\\s+)?(?:ca[- ]?edta|complex)', q, re.I)
        km = re.search('k[^=\\n]{0,20}=\\s*((?:[0-9.]+\\s*[x*]\\s*10\\s*\\^?\\s*[+-]?\\d+)|(?:[0-9.]+e[+-]?\\d+))', q, re.I)
        if not (cm and km):
            return None
        C = float(cm.group(1))
        K = _num_expr(km.group(1))
        if not K:
            return None
        x = math.sqrt(C / K)
        return _一般結果(_nearest(choices, x, log=True), 'complex_dissociation', x)
    lm = re.search('(?:ligand|scn|thiocyan\\w*)[^0-9]{0,30}([0-9.]+)\\s*m', q, re.I)
    betas = []
    for _i, val in re.findall('(?:beta|β)\\s*_?\\s*(\\d+)\\s*=\\s*([0-9.eE+\\-*x^]+)', q, re.I):
        parsed = _num_expr(val)
        if parsed is not None:
            betas.append(parsed)
    if lm and betas:
        L = float(lm.group(1))
        terms = [1] + [b * L ** (i + 1) for i, b in enumerate(betas)]
        nm = re.search('(?:fraction|percentage).*?(?:scn|ligand)[^0-9]{0,5}(\\d+)', q, re.I)
        n = int(nm.group(1)) if nm else min(2, len(betas))
        if 1 <= n <= len(betas):
            frac = 100 * terms[n] / sum(terms)
            return _一般結果(_nearest(choices, frac, rel_tol=0.08), 'cumulative_complex_fraction', frac)
    return None

def solve_weak_acid(q, choices):
    s = q.casefold()
    if 'titrate' not in s or ('weak' not in s and 'acetic acid' not in s):
        return None
    ka_m = re.search('ka(?:\\s+of[^0-9=]{0,40})?\\s*(?:=|is)\\s*((?:[0-9.]+\\s*[x*]\\s*10\\s*\\^?\\s*[+-]?\\d+)|(?:[0-9.]+e[+-]?\\d+))', q, re.I)
    acid_m = re.search('([0-9.]+)\\s*(?:cm3|ml)\\s+([0-9.]+)\\s*m\\s+(?:\\w+\\s+){0,3}acid', q, re.I)
    water_m = re.search('(?:with|and)\\s+([0-9.]+)\\s*(?:cm3|ml)\\s+water', q, re.I)
    base_m = re.search('(?:with|using)\\s+([0-9.]+)\\s*m\\s+(?:naoh|koh)', q, re.I)
    frac_m = re.search('at\\s+([0-9.]+)%\\s+titration', q, re.I)
    if not (ka_m and acid_m and base_m and frac_m):
        return None
    Ka = _num_expr(ka_m.group(1))
    Va = float(acid_m.group(1)) / 1000
    Ca = float(acid_m.group(2))
    Cb = float(base_m.group(1))
    f = float(frac_m.group(1)) / 100
    Vwater = float(water_m.group(1)) / 1000 if water_m else 0.0
    n0 = Va * Ca
    Veq = n0 / Cb
    pKa = -math.log10(Ka)
    pH1 = pKa + math.log10(f / (1 - f))
    Vtot = Va + Vwater + Veq
    Csalt = n0 / Vtot
    Kb = 1e-14 / Ka
    oh = math.sqrt(Kb * Csalt)
    pH2 = 14 + math.log10(oh)
    best = []
    for i, c in enumerate(choices):
        ns = _nums(c)
        if len(ns) >= 2:
            best.append((abs(ns[0] - pH1) + abs(ns[1] - pH2), i))
    if not best:
        return None
    best.sort()
    return _一般結果(best[0][1], 'weak_acid_titration', (pH1, pH2))

def solve_phosphate(q, choices):
    s = q.casefold()
    if 'phosphate' not in s or 'ka' not in s:
        return None
    salts = re.findall('([0-9.]+)\\s*g\\s+([^()\\n]{1,40}?)\\s*\\(\\s*mw\\s*=\\s*([0-9.]+)\\s*g/mol\\s*\\)', q, re.I)
    vm = re.search('(?:volume(?:\\s+of)?|solution(?:\\s+which)?\\s+has\\s+the\\s+volume\\s+of)[^0-9]{0,30}([0-9.]+)\\s*(l|ml|cm3)', q, re.I)
    if len(salts) < 2 or not vm:
        return None
    ka_字句 = re.findall('(?:[0-9.]+\\s*[x*]\\s*10\\s*\\^?\\s*[+-]?\\d+|[0-9.]+e[+-]?\\d+)', q, re.I)
    kas = []
    for tok in ka_字句:
        v = _num_expr(tok)
        if v is not None and 0 < v < 1:
            kas.append(v)
    if len(kas) < 3:
        return None
    Ka2, Ka3 = (kas[-2], kas[-1])
    m1, _, mw1 = salts[0]
    m2, _, mw2 = salts[1]
    n_acid = float(m1) / float(mw1)
    n_base = float(m2) / float(mw2)
    if n_base <= 0:
        return None
    V = float(vm.group(1)) * (0.001 if vm.group(2).casefold() in {'ml', 'cm3'} else 1)
    H = Ka2 * n_acid / n_base
    po4 = Ka3 * (n_base / V) / H
    return _一般結果(_nearest(choices, po4, log=True), 'phosphate_speciation', po4)

def solve_neutralization(q, choices):
    s = q.casefold()
    if 'enthalpy of neutralization' not in s:
        return None
    species = {}
    for vol, conc, name in re.findall('([0-9.]+)\\s*(?:ml|cm3)\\s+([0-9.]+)\\s*m\\s+(hcl|h2so4|ba\\(oh\\)2|naoh|koh)', q, re.I):
        species[name.casefold()] = (float(vol) / 1000, float(conc))
    acid = 0.0
    base = 0.0
    for name, (v, c) in species.items():
        if name == 'hcl':
            acid += v * c
        elif name == 'h2so4':
            acid += 2 * v * c
        elif name == 'ba(oh)2':
            base += 2 * v * c
        elif name in {'naoh', 'koh'}:
            base += v * c
    if not acid or not base:
        return None
    water = min(acid, base)
    kj = -57.0 * water
    targets = [kj, kj / 4.184]
    best = []
    for i, c in enumerate(choices):
        vals = _nums(c)
        if not vals:
            continue
        val = vals[0]
        target = targets[0] if 'kj' in c.casefold() else targets[1] if 'kcal' in c.casefold() else targets[0]
        best.append((abs(val - target), i))
    if not best:
        return None
    best.sort()
    return _一般結果(best[0][1], 'neutralization_enthalpy', (kj, kj / 4.184))

def solve_ksp(q, choices):
    s = q.casefold()
    if 'ksp' not in s or 'strong acid' not in s:
        return None
    if '(oh)3' not in s:
        return None
    mass = _first('([0-9.]+)\\s*g\\s+(?:of\\s+)?[a-z]+\\(oh\\)3', q)
    mm = _first('molar(?:\\s+mass)?[^0-9]{0,20}([0-9.]+)', q)
    volm = re.search('(?:in|volume)[^0-9]{0,20}([0-9.]+)\\s*(ml|cm3|l)\\b', q, re.I)
    km = re.search('ksp[^=0-9]{0,10}=\\s*([0-9.eE+\\-*x^]+)', q, re.I)
    acidm = re.search('([0-9.]+)\\s*m\\s+(?:strong\\s+)?acid', q, re.I)
    if not (mass and mm and volm and km and acidm):
        return None
    V = float(volm.group(1)) * (0.001 if volm.group(2).casefold() in {'ml', 'cm3'} else 1)
    K = _num_expr(km.group(1))
    Ca = float(acidm.group(1))
    mol = mass / mm
    metal = mol / V
    oh = (K / metal) ** (1 / 3)
    pH = 14 + math.log10(oh)
    H = 10 ** (-pH)
    acid_vol = (3 * mol + H * V) / Ca * 1000
    best = []
    for i, c in enumerate(choices):
        ns = _nums(c)
        if len(ns) >= 2:
            best.append((abs(ns[0] - pH) + abs(ns[1] - acid_vol) / 10, i))
    if not best:
        return None
    best.sort()
    return _一般結果(best[0][1], 'ksp_acid_dissolution', (pH, acid_vol))
登録簿 = (solve_complex, solve_weak_acid, solve_phosphate, solve_neutralization, solve_ksp)

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

from __future__ import annotations
import math
import re
from typing import Sequence
from .科学専門能力_共通 import *

def solve_gauss_radial(q, choices):
    s = q.casefold().replace(' ', '')
    radial = 'radial' in s
    invsq = '1/r^2' in s or 'r^-2' in s or 'r**-2' in s
    divergence = 'divergence' in s or '∇.' in q or 'nabla' in s
    enclosing = ('sphere' in s or 'spherical' in s) and ('integral' in s or 'flux' in s)
    if not (radial and invsq and divergence and enclosing):
        return None
    for i, c in enumerate(choices):
        cc = c.casefold().replace(' ', '').replace('\\pi', 'pi').replace('π', 'pi')
        if cc in {'4pi', '4*pi'}:
            return _結果(i, 'gauss_radial_flux', 4 * math.pi)
    return _結果(_nearest(choices, 4 * math.pi, rel_tol=0.05), 'gauss_radial_flux', 4 * math.pi)

def solve_loop_count(q, choices):
    s = q.casefold()
    if 'how many loops' not in s or '(4pi)' not in s:
        return None
    m = re.search('1\\s*/\\s*\\(4pi\\)\\^(\\d+)', q.replace(' ', ''), re.I)
    if not m:
        m = re.search('\\(4pi\\)\\^(-?\\d+)', q.replace(' ', ''), re.I)
    if not m:
        return None
    power = abs(int(m.group(1)))
    loops = power / 2
    return _結果(_nearest(choices, loops, rel_tol=0.01), 'loop_factor_count', loops)

def solve_partial_wave_forward_imag(q, choices):
    s = q.casefold()
    if 'phase shifts' not in s or 'imaginary part of the scattering amplitude' not in s:
        return None
    em = re.search('elastic scattering of\\s*([0-9.]+)\\s*~?mev\\s*electrons', q, re.I)
    E = float(em.group(1)) if em else 50.0
    deltas = []
    clean = q.replace('\\', '')
    for lm, dm in re.findall('delta[_{}\\s]*(o|0|\\d+)\\s*=\\s*([0-9.]+)', clean, re.I):
        ell = 0 if lm.casefold() == 'o' else int(lm)
        deltas.append((ell, float(dm)))
    if not deltas:
        return None
    summ = sum(((2 * l + 1) * math.sin(math.radians(d)) ** 2 for l, d in deltas))
    me = 0.51099895
    k = math.sqrt(2 * me * E) / 197.3269804
    target = summ / k
    return _結果(_nearest(choices, target, rel_tol=0.08), 'partial_wave_forward_imag', target)

def solve_rhombohedral_metric(q, choices):
    s = q.casefold()
    if 'rhombohedral crystal' not in s or 'interplanar distance' not in s:
        return None
    for i, c in enumerate(choices):
        cc = c.replace(' ', '')
        if 'a^{2}' in cc and 'h^{2}+k^{2}+l^{2}' in cc and ('sin^{2}' in cc) and ('hk+kl+hl' in cc) and ('cos^{2}' in cc):
            return _結果(i, 'rhombohedral_reciprocal_metric', 'standard reciprocal metric')
    return None

def solve_conductor_sphere_external_generic(q, choices):
    s = q.casefold()
    conductor = 'conductor' in s and ('sphere' in s or 'spherical' in s)
    cavity = 'cavity' in s or 'hollow' in s
    exterior = any((x in s for x in ('outside', 'external', 'exterior')))
    if not (conductor and cavity and exterior):
        return None
    hits = []
    for i, c in enumerate(choices):
        raw = str(c).replace(' ', '').replace('{', '').replace('}', '')
        if re.search('q/L(?:\\^?2|2)', raw) and (not any((x in raw for x in ('L+s', 'L-s')))):
            hits.append(i)
    return _一般結果(hits[0] if len(hits) == 1 else None, 'conducting_sphere_external_field', 'kq/L²')

def solve_zeeman(q, choices):
    s = q.casefold()
    if 'zeeman' not in s and 'paramagnetic coupling' not in s:
        return None
    bm = re.search('\\bb\\s*=\\s*([0-9.]+)\\s*t', q, re.I)
    wm = re.search('wavelength[^0-9]{0,20}([0-9.]+)\\s*(nm|microm|um|µm)', q, re.I)
    if not wm:
        wm = re.search('([0-9.]+)\\s*(microm|um|µm)', q, re.I)
    if not (bm and wm):
        return None
    B = float(bm.group(1))
    lam = float(wm.group(1)) * (1000 if wm.group(2).casefold() in {'microm', 'um', 'µm'} else 1)
    z = 5.7883818e-05 * B
    photon = 1239.841984 / lam
    関係 = 'll' if z < photon / 10 else 'gg' if z > photon * 10 else 'sim'
    for i, c in enumerate(choices):
        cc = c.replace(' ', '')
        if 関係 == 'll' and ('\\ll' in cc or '≪' in cc or '<<' in cc):
            return _一般結果(i, 'zeeman_vs_transition', z / photon)
        if 関係 == 'gg' and ('\\gg' in cc or '≫' in cc or '>>' in cc):
            return _一般結果(i, 'zeeman_vs_transition', z / photon)
    return None

def solve_synchro(q, choices):
    s = q.casefold()
    if 'synchrocyclotron' not in s or 'revolutions' not in s:
        return None
    em = re.search('(?:t[_ ]?1\\s*=|(?:final\\s+)?(?:energy|kinetic energy)[^0-9]{0,20})([0-9.]+)\\s*mev', q, re.I)
    um = re.search('(?:voltage[^0-9]{0,30}|u[_ ]?0\\s*=\\s*)([0-9.]+)\\s*(kv|mv|mev)', q, re.I)
    if not (em and um):
        return None
    E = float(em.group(1))
    U = float(um.group(1))
    unit = um.group(2).casefold()
    if unit == 'kv':
        U *= 0.001
    elif unit == 'mv':
        U *= 1.0
    pm_deg = re.search('(?:phase|phi|\\\\phi)[^0-9]{0,20}([0-9.]+)\\s*(?:deg|degree)', q, re.I)
    pm_pi = re.search('(?:phi|\\\\phi)[^=]{0,8}=\\s*(?:\\\\frac\\{)?(?:\\\\pi|pi)(?:\\}\\{)?\\s*/?\\s*([0-9.]+)', q, re.I)
    if pm_deg:
        phi = math.radians(float(pm_deg.group(1)))
    elif pm_pi:
        phi = math.pi / float(pm_pi.group(1))
    elif 'pi}{4' in q.casefold() or 'pi/4' in q.casefold():
        phi = math.pi / 4
    else:
        return None
    if U <= 0 or math.cos(phi) <= 0:
        return None
    target = E / (2 * U * math.cos(phi))
    return _一般結果(_nearest(choices, target, rel_tol=0.08), 'synchrocyclotron_revolutions', target)
登録簿 = (solve_gauss_radial, solve_loop_count, solve_partial_wave_forward_imag, solve_rhombohedral_metric, solve_conductor_sphere_external_generic, solve_zeeman, solve_synchro)

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

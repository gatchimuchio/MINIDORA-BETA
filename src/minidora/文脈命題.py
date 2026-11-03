"""資料の読みと照応を保持する。異なる読みの証拠を一つの世界へ混ぜない。"""
from __future__ import annotations

from dataclasses import asdict, replace
import re
from .命題句 import 構成句を分ける, 引用を切り出す, 最上位位置
from .命題構造 import 命題記載, 命題を復元, 原子群
from .命題解釈 import 命題を読む, 命題を表現, _外側
from .命題語彙 import 接続語
from .命題推論 import 命題推論器, 推論上限

文脈命題版 = 'MINIDORA-文脈命題-v0.1'
_指示語 = ('彼', '彼女', '同者', 'それ')


def _主題群(式, 射影文):
    """明示された主題だけを渡す。任意述語の第1引数や引用内の名を主題と推定しない。"""
    names = tuple(dict.fromkeys(a.項[0].名前 for a in 原子群(式)
        if a.項 and (len(a.項) == 1 or a.種別 == '帰属') and a.項[0].種別 == '定数'))
    if not names: return ()
    phrases = {name + 'は': name for name in names}
    phrases.update({name + 'によると': name for name in names})
    phrases.update({name + 'が': name for name in names})
    return tuple(dict.fromkeys(phrases[word] for _, word in 最上位位置(射影文, tuple(phrases))))


def _引用内一人称(本文, 深さ=0):
    if 深さ > 8: raise ValueError('帰属照応の深さ上限')
    trimmed = 本文.strip()
    leading = len(本文) - len(本文.lstrip())
    if trimmed.startswith(('(', '（')) and _外側(trimmed) is not None:
        changed, 追跡 = _引用内一人称(trimmed[1:-1], 深さ + 1)
        return 本文[:leading + 1] + changed + 本文[leading + len(trimmed) - 1:], 追跡
    wrapper = re.fullmatch(r'(否定|[0-9]{4}年では|時点「[^「」]+」では)([（(].*[）)])', trimmed, re.S)
    if wrapper and _外側(wrapper[2]) is not None:
        changed, 追跡 = _引用内一人称(wrapper[2], 深さ + 1)
        return 本文[:leading] + wrapper[1] + changed + 本文[leading + len(trimmed):], 追跡
    clauses = list(構成句を分ける(本文, (*接続語, '。', '\n')))
    if len(clauses) > 1:
        結果 = []; cursor = 0; traces = []
        for a, b in clauses:
            changed, local = _引用内一人称(本文[a:b], 深さ + 1)
            結果.extend((本文[cursor:a], changed)); cursor = b; traces.extend(local)
        結果.append(本文[cursor:])
        return ''.join(結果), traces
    for i, marker in 最上位位置(本文, ('は', 'が')):
        start = i + len(marker)
        if start >= len(本文) or 本文[start] not in ('「', '『'): continue
        content, end = 引用を切り出す(本文, start)
        speaker = 本文[:i].strip()
        if not speaker or speaker in (*_指示語, '私', 'わたし'): continue
        # 一人称を解消する範囲は直接の引用内のみ。入れ子引用では話者を切り替える。
        pieces = []; cursor = 0; changes = []
        for a, b in 構成句を分ける(content, (*接続語, '。', '\n')):
            fragment = content[a:b]
            inner, 追跡 = _引用内一人称(fragment, 深さ + 1)
            if not 追跡:
                m = re.match(r'\s*(私|わたし)は', fragment)
                if m:
                    left, right = m.span(1)
                    inner = fragment[:left] + speaker + fragment[right:]
                    追跡 = [{'種別': '引用内一人称', '原文': m[1], '束縛先': speaker,
                              '理由': '直接引用の明示話者'}]
            pieces.extend((content[cursor:a], inner)); cursor = b; changes.extend(追跡)
        pieces.append(content[cursor:])
        if changes:
            return 本文[:start + 1] + ''.join(pieces) + 本文[end - 1:], changes
    return 本文, []


def 文脈資料を読む(本文: str, 資料名: str, *, 照応距離: int = 1):
    if type(本文) is not str or not 本文.strip() or len(本文) > 32000:
        raise ValueError('文脈資料の型・サイズ')
    if type(資料名) is not str or not 0 < len(資料名) <= 128 or any(ord(c) < 32 for c in 資料名):
        raise ValueError('文脈資料名不正')
    if type(照応距離) is not int or not 1 <= 照応距離 <= 16:
        raise ValueError('照応距離は1〜16の整数')
    rows = []; previous = (); extended = False
    for start, end in 構成句を分ける(本文):
        original = 本文[start:end]; forms = [(original, [])]
        hit = re.match(r'\s*(彼女|彼|同者|それ)は', original)
        if hit:
            prior_index = len(rows) - 1
            # 無主題の記載だけをまたぐ明示的な有界規約。一般の指示意図は保証しない。
            if not previous and 照応距離 > 1:
                for j in range(len(rows) - 2, max(-1, len(rows) - 照応距離 - 1), -1):
                    topics = tuple(dict.fromkeys(name for prior in rows[j]['候補']
                        for name in _主題群(命題を復元(prior['式']), prior['射影文'])))
                    if topics:
                        previous, prior_index = topics, j
                        break
            if not previous: raise ValueError('指示先がない資料内照応:' + hit[1])
            forms = [(original[:hit.start(1)] + name + original[hit.end(1):],
                      [{'種別': '資料内照応', '原文': hit[1], '束縛先': name,
                        '理由': ('直前記載の明示主題（局所照応規約）' if prior_index == len(rows) - 1
                                 else '無主題記載をまたぐ有界照応規約。指示意図は未保証'), '参照記載': prior_index,
                        '参照候補': [i + 1 for i, prior in enumerate(rows[prior_index]['候補'])
                            if name in _主題群(命題を復元(prior['式']), prior['射影文'])]}]) for name in previous]
        options = {}
        for form, resolutions in forms:
            projected, quoted = _引用内一人称(form)
            for c in 命題を読む(projected):
                record = {'式': c.式.辞書(), '読み': c.読み, '解消': resolutions + quoted,
                          '射影文': projected}
                options.setdefault(c.式.鍵(), record)
                if len(options) > 8: raise ValueError('文脈資料の意味候補上限')
        candidates = list(options.values())
        extended |= bool(hit) or len(candidates) > 1 or any(c['解消'] for c in candidates)
        previous = tuple(dict.fromkeys(x for c in candidates for x in _主題群(命題を復元(c['式']), c['射影文'])))
        if len(previous) > 8: raise ValueError('次記載へ渡す主題候補上限')
        rows.append({'識別子': 資料名 + ':' + str(len(rows)), '資料': 資料名,
                     '原文': original, '範囲': (start, end), '候補': candidates})
        if len(rows) > 128: raise ValueError('文脈資料の記載数上限')
    if not rows: raise ValueError('文脈資料に記載がない')
    return {'資料': 資料名, '記載候補': rows, '文脈拡張': extended}


def 解釈場合を構成(資料群: tuple[dict, ...], *, 最大場合=16):
    if type(最大場合) is not int or not 1 <= 最大場合 <= 16:
        raise ValueError('解釈場合の上限不正')
    if type(資料群) is not tuple or not 1 <= len(資料群) <= 8:
        raise ValueError('文脈資料の数・型')
    rows = [r for doc in 資料群 for r in doc['記載候補']]
    if len(rows) > 256 or len({r['識別子'] for r in rows}) != len(rows):
        raise ValueError('文脈記載の数・重複')
    # 照応の前提となる読みの選択を保持し、互いに成立しない組合せを作らない。
    assignments = [()]
    for pos, r in enumerate(rows):
        following = []
        for prefix in assignments:
            for i, option in enumerate(r['候補']):
                compatible = True
                for resolution in option['解消']:
                    if resolution['種別'] != '資料内照応': continue
                    prior_id = r['資料'] + ':' + str(resolution['参照記載'])
                    prior_pos = next(j for j in range(pos) if rows[j]['識別子'] == prior_id)
                    if prefix[prior_pos] + 1 not in resolution['参照候補']: compatible = False
                if compatible: following.append((*prefix, i))
                if len(following) > 最大場合:
                    raise ValueError('資料の解釈組合せ上限。部分候補だけで確定しない')
        assignments = following
    if not assignments: raise ValueError('照応と資料解釈の両方が成立する場合がない')
    cases = []
    for combo in assignments:
        selected = []; choices = []; resolutions = []
        for r, i in zip(rows, combo):
            c = r['候補'][i]
            selected.append(命題記載(r['識別子'], 命題を復元(c['式']), r['資料'], r['原文'], tuple(r['範囲'])))
            if len(r['候補']) > 1:
                choices.append({'記載': r['識別子'], '番号': i + 1, '読み': c['読み']})
            resolutions.extend({'記載': r['識別子'], **t} for t in c['解消'])
        cases.append({'記載': tuple(selected), '選択': choices, '照応解消': resolutions})
    return tuple(cases)


def 文脈判定(資料群, 問い, 問い候補=1, 資料候補=0, *, 述語別名=()):
    from .明示語彙 import 述語別名を検査, 命題の述語を対応付ける
    mapping = 述語別名を検査(述語別名)
    candidates = 命題を読む(問い)
    if type(問い候補) is not int or not 1 <= 問い候補 <= len(candidates):
        raise ValueError('問いの候補番号不正')
    cases = 解釈場合を構成(資料群)
    if type(資料候補) is not int or not 0 <= 資料候補 <= len(cases):
        raise ValueError('資料解釈の候補番号不正')
    reports = []; remaining = 100000
    for number, case in enumerate(cases, 1):
        if 資料候補 and 資料候補 != number: continue
        if remaining <= 0: raise ValueError('資料候補全体の推論予算超過')
        records, traces = [], []
        for record in case['記載']:
            expression, local = 命題の述語を対応付ける(record.式, mapping)
            records.append(replace(record, 式=expression))
            traces.extend({'記載': record.識別子, **row} for row in local)
        question, local = 命題の述語を対応付ける(candidates[問い候補 - 1].式, mapping)
        traces.extend({'記載': '問い', **row} for row in local)
        engine = 命題推論器(tuple(records), 上限=推論上限(操作数=min(50000, remaining)))
        結果 = engine.判定(question)
        remaining -= 結果['操作数']
        reports.append({'場合': number, '選択': case['選択'], '照応解消': case['照応解消'],
                        '記載': [asdict(r) for r in records], '判定結果': 結果,
                        **({'語彙対応': traces} if 述語別名 else {})})
    statuses = {r['判定結果']['判定'] for r in reports}
    return {'問い': 問い, '問い候補': 問い候補, '資料候補': 資料候補, '場合総数': len(cases),
            '解釈状態': '利用者選択' if 資料候補 else '一意' if len(cases) == 1 else '読み未確定',
            '判定': next(iter(statuses)) if len(statuses) == 1 else '解釈依存',
            '場合別': reports, '操作数': 100000 - remaining,
            **({'別名定義': [dict(row) for row in 述語別名]} if 述語別名 else {})}

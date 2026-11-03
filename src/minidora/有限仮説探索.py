"""明示した規則と候補集合から、観測を説明する極小仮説集合を求める。

仮説は世界事実ではない。因果の方向・常識・尤度を自動補完しない。
有限の基底命題と明示否定、連言前件を持つ前向き規則を対象とする。
"""
from __future__ import annotations

from copy import deepcopy
from itertools import combinations
from math import comb
from .能力合成 import _符号化
from .会話意味 import 意味指紋
from .命題解釈 import 命題を読む, 命題を表現
from .命題構造 import 命題式, 反対

仮説探索版 = 'MINIDORA-有限仮説探索-v0.2'


def _文字(value: object, 名前: str, 最大: int = 256) -> str:
    if (type(value) is not str or not value.strip() or len(value) > 最大
            or any(ord(c) < 32 for c in value)):
        raise ValueError(名前 + 'の型・長さ不正')
    return value


def _欄(value: object, 必須: set[str], 任意: set[str] = frozenset()) -> dict:
    if type(value) is not dict or not 必須 <= set(value) <= 必須 | 任意:
        raise ValueError('入力欄の不足又は未知欄')
    return value


def _列(value: object, 名前: str, 最大: int, 最低: int = 0) -> list:
    if type(value) is not list or not 最低 <= len(value) <= 最大:
        raise ValueError(名前 + 'の型・件数不正')
    return value


def _整数(value: object, 名前: str, 最低: int, 最大: int) -> int:
    if type(value) is not int or not 最低 <= value <= 最大:
        raise ValueError(名前 + 'の範囲不正')
    return value


def _リテラル(text: object) -> 命題式:
    _文字(text, '基底命題', 4096)
    candidates = 命題を読む(text)
    if len(candidates) != 1:
        raise ValueError('仮説探索の命題解釈は一意に指定する')
    expression = candidates[0].式
    atom = expression.子[0] if expression.種別 == '否定' else expression
    if (atom.種別 != '原子' or atom.様相 != '記載'
            or any(term.種別 != '定数' for term in atom.項)):
        raise ValueError('仮説探索は基底原子とその明示否定に限定する')
    return expression


def 仮説を検討(要求: dict) -> dict:
    """指定された探索範囲を閉じてから報告する。予算超過では部分採用しない。"""
    _欄(要求, {'事実', '規則', '観測', '仮説候補'},
        {'最大仮説数', '最大試行数', '最大操作数'})
    if len(_符号化(要求)) > 100000:
        raise ValueError('仮説要求のバイト上限')
    要求 = deepcopy(要求)
    最大仮説数 = _整数(要求.get('最大仮説数', 3), '最大仮説数', 0, 6)
    最大試行数 = _整数(要求.get('最大試行数', 2048), '最大試行数', 1, 4096)
    最大操作数 = _整数(要求.get('最大操作数', 100000), '最大操作数', 1, 1000000)
    表現: dict[str, 命題式] = {}
    反転: dict[str, str] = {}

    def 登録(text: str) -> str:
        e = _リテラル(text)
        key, other = e.鍵(), 反対(e).鍵()
        表現[key] = e
        反転[key] = other
        反転[other] = key
        return key

    識別子 = set()
    事実 = []
    規則 = []
    for row in _列(要求['事実'], '事実', 128):
        _欄(row, {'識別子', '命題', '出典'})
        name = _文字(row['識別子'], '事実識別子')
        if name in 識別子:
            raise ValueError('事実・規則識別子の重複')
        識別子.add(name)
        事実.append((name, 登録(row['命題']), _文字(row['出典'], '事実出典')))
    for row in _列(要求['規則'], '規則', 128):
        _欄(row, {'識別子', '前件', '後件', '出典'})
        name = _文字(row['識別子'], '規則識別子')
        if name in 識別子:
            raise ValueError('事実・規則識別子の重複')
        識別子.add(name)
        left = tuple(登録(text) for text in _列(row['前件'], '規則前件', 8, 1))
        if len(set(left)) != len(left):
            raise ValueError('規則前件の重複')
        規則.append((name, left, 登録(row['後件']), _文字(row['出典'], '規則出典')))
    観測 = tuple(登録(text) for text in _列(要求['観測'], '観測', 32, 1))
    仮説 = tuple(登録(text) for text in _列(要求['仮説候補'], '仮説候補', 16))
    for values in (観測, 仮説):
        if len(set(values)) != len(values):
            raise ValueError('観測又は仮説候補の重複')
    if any(反転[k] in 観測 for k in 観測):
        raise ValueError('観測集合が自己矛盾')
    if set(観測) & set(仮説):
        raise ValueError('観測をそのまま仮説として自己説明しない')
    上限数 = min(最大仮説数, len(仮説))
    全組合せ数 = sum(comb(len(仮説), n) for n in range(上限数 + 1))
    事実.sort()
    規則.sort()
    仮説 = tuple(sorted(仮説))
    操作数 = 0

    def 刻む() -> None:
        nonlocal 操作数
        操作数 += 1
        if 操作数 > 最大操作数:
            raise ValueError('仮説探索の操作予算超過。途中結果を採用しない')

    def 閉包(候補: tuple[str, ...]):
        nodes, facts = {}, {}

        def 加える(key, 種別, 親=(), 出典='', 入力ID=''):
            刻む()
            if key in facts:
                return False
            node = {'命題': 命題を表現(表現[key]), '作用': 種別,
                    '親': list(親), '出典': 出典, '入力ID': 入力ID}
            proof_id = 意味指紋(node)
            nodes[proof_id] = node
            facts[key] = proof_id
            return True

        for name, key, 情報源 in 事実:
            加える(key, '提供事実', 出典=情報源, 入力ID=name)
        for key in 候補:
            加える(key, '仮説導入', 入力ID=key)
        changed = True
        while changed:
            changed = False
            for name, left, right, 情報源 in 規則:
                刻む()
                if all(k in facts for k in left):
                    changed |= 加える(right, '規則適用', tuple(facts[k] for k in left), 情報源, name)
        conflicts = sorted(k for k in facts if 反転[k] in facts)
        return facts, nodes, conflicts

    背景, _, 衝突 = 閉包(())
    # 前向き・単調な規則だけなので、観測の祖先でない候補は極小説明に不要。
    # 規則・背景事実は削らない。無関係な出力に生じる矛盾も閉包で検出する。
    関連 = set(観測)
    changed = True
    while changed:
        changed = False
        for _, left, right, _ in 規則:
            刻む()
            if right in 関連:
                old = len(関連)
                関連.update(left)
                changed |= len(関連) != old
    関連仮説 = tuple(k for k in 仮説 if k in 関連)
    関連上限 = min(上限数, len(関連仮説))
    関連組合せ数 = sum(comb(len(関連仮説), n) for n in range(関連上限 + 1))
    極小集合: list[frozenset[str]] = []
    解 = []
    件数 = {'評価': 0, '極小性による省略': 0, '不整合': 0, '説明不足': 0,
            '関連外による省略': 全組合せ数 - 関連組合せ数}
    背景不整合 = bool(衝突 or any(反転[k] in 背景 for k in 観測))
    if not 背景不整合:
        for n in range(関連上限 + 1):
            for combo in combinations(関連仮説, n):
                刻む()
                current = frozenset(combo)
                if any(prior <= current for prior in 極小集合):
                    件数['極小性による省略'] += 1
                    continue
                if 件数['評価'] >= 最大試行数:
                    raise ValueError('指定仮説範囲の探索予算不足。部分候補を採用しない')
                件数['評価'] += 1
                closure, 関係図, conflicts = 閉包(combo)
                if conflicts or any(反転[k] in closure for k in 観測):
                    件数['不整合'] += 1
                    continue
                if not all(k in closure for k in 観測):
                    件数['説明不足'] += 1
                    continue
                極小集合.append(current)
                keep = set()
                pending = [closure[k] for k in 観測]
                while pending:
                    pid = pending.pop()
                    if pid in keep:
                        continue
                    keep.add(pid)
                    pending.extend(関係図[pid]['親'])
                解.append({'仮説': [命題を表現(表現[k]) for k in combo],
                           '観測の根拠': {命題を表現(表現[k]): closure[k] for k in 観測},
                           '導出': {k: 関係図[k] for k in sorted(keep)}})
    status = ('背景不整合' if 背景不整合 else '説明候補あり' if 解 else '指定範囲に説明なし')
    report = {'版': 仮説探索版, '状態': status, '要求': 要求, '候補': 解,
              '探索範囲': {'候補命題数': len(仮説), '最大仮説数': 上限数,
                           '対象組合せ数': 全組合せ数, '関連候補命題数': len(関連仮説)},
              '探索完了': not 背景不整合, '件数': 件数, '操作数': 操作数,
              '事実認定': False,
              '限界': '提供規則と指定候補・最大仮説数の下での包含極小説明。因果の真実性・候補の網羅性・尤度は未認定。'}
    report['記録SHA256'] = 意味指紋(report)
    return report


def 仮説報告を検査(報告: dict) -> bool:
    try:
        return _符号化(報告) == _符号化(仮説を検討(報告['要求']))
    except (ValueError, TypeError, KeyError, AttributeError, RecursionError):
        return False

"""明示された非巡回ブール構造モデルで、同一外生状態の介入差を計算する。

データから因果を発見する器ではない。モデルの正しさを現実へ昇格しない。
外生状態が不明な場合の自動補完や、観測条件付けを介入とみなす処理はしない。
"""
from __future__ import annotations

from copy import deepcopy
from graphlib import TopologicalSorter, CycleError
from .能力合成 import _符号化
from .会話意味 import 意味指紋
from .有限仮説探索 import _欄, _文字, _列

因果モデル版 = 'MINIDORA-有限因果モデル-v0.1'


def 介入を比較(要求: dict) -> dict:
    _欄(要求, {'外生', '方程式', '介入'}, {'観測'})
    if len(_符号化(要求)) > 100000:
        raise ValueError('因果モデルのバイト上限')
    要求 = deepcopy(要求)
    外生, 介入, 観測 = 要求['外生'], 要求['介入'], 要求.get('観測', {})
    for name, table in (('外生', 外生), ('介入', 介入), ('観測', 観測)):
        if type(table) is not dict or len(table) > 64:
            raise ValueError(name + 'の型・上限')
        for key, value in table.items():
            _文字(key, name + '変数', 80)
            if type(value) is not bool:
                raise ValueError(name + 'の値はbool')
    式 = {}
    出典 = {}
    for row in _列(要求['方程式'], '方程式', 64, 1):
        _欄(row, {'変数', '式', '出典'})
        name = _文字(row['変数'], '内生変数', 80)
        if name in 式 or name in 外生:
            raise ValueError('内生変数の重複又は外生との衝突')
        式[name] = row['式']
        出典[name] = _文字(row['出典'], '方程式出典')
    全変数 = set(式) | set(外生)
    if len(全変数) > 64:
        raise ValueError('モデル変数数の上限')
    if not set(介入) <= set(式) or not set(観測) <= 全変数:
        raise ValueError('介入又は観測が未定義。介入対象は内生変数')
    節点数 = 0

    def 参照群(e, depth=0):
        nonlocal 節点数
        節点数 += 1
        if depth > 16 or 節点数 > 512:
            raise ValueError('構造式の深さ・節点数上限')
        if type(e) is bool:
            return set()
        if type(e) is not dict or len(e) != 1:
            raise ValueError('構造式の形式不正')
        op, arg = next(iter(e.items()))
        if op == '参照':
            _文字(arg, '構造式参照', 80)
            if arg not in 全変数:
                raise ValueError('構造式の参照先が未定義')
            return {arg}
        if op == '否定':
            return 参照群(arg, depth + 1)
        if op in ('すべて', 'いずれか'):
            children = _列(arg, '構造式の子', 16, 1)
            return set().union(*(参照群(c, depth + 1) for c in children))
        raise ValueError('未知の構造式演算')

    依存 = {name: 参照群(e) & set(式) for name, e in sorted(式.items())}
    try:
        順序 = tuple(TopologicalSorter({k: sorted(v) for k, v in 依存.items()}).static_order())
    except CycleError as exc:
        raise ValueError('巡回する構造モデルは未対応') from exc

    def 計算(置換):
        values = dict(外生)
        追跡 = []

        def 評価(e):
            if type(e) is bool:
                return e
            op, arg = next(iter(e.items()))
            if op == '参照':
                return values[arg]
            if op == '否定':
                return not 評価(arg)
            results = [評価(x) for x in arg]
            return all(results) if op == 'すべて' else any(results)

        for name in 順序:
            values[name] = 置換[name] if name in 置換 else 評価(式[name])
            追跡.append({'変数': name, '値': values[name],
                          '作用': '介入置換' if name in 置換 else '構造式評価',
                          '式': 式[name], '出典': 出典[name]})
        return values, 追跡

    現状, 現状導出 = 計算({})
    if any(現状[k] != v for k, v in 観測.items()):
        raise ValueError('提供モデル・外生状態が観測と矛盾')
    介入後, 介入導出 = 計算(介入)
    変化 = {k: {'前': 現状[k], '後': 介入後[k]} for k in sorted(式) if 現状[k] != 介入後[k]}
    report = {'版': 因果モデル版, '要求': 要求, '状態': 'モデル内計算完了',
              '現状': 現状, '介入後': 介入後, '変化': 変化,
              '現状導出': 現状導出, '介入導出': 介入導出, '事実認定': False,
              '限界': '明示した非巡回ブール構造式と同一外生状態の下での比較。現実の因果同定や効果推定ではない。'}
    report['記録SHA256'] = 意味指紋(report)
    return report


def 介入報告を検査(報告: dict) -> bool:
    try:
        return _符号化(報告) == _符号化(介入を比較(報告['要求']))
    except (ValueError, TypeError, KeyError, AttributeError, RecursionError):
        return False

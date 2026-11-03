"""意味目的から検討・説明・最終照合を計画し、既存の統合採用境界へ渡す。"""
from __future__ import annotations
from copy import deepcopy
from .会話意味 import 意味目的, 意味指紋
from .役割計画 import 役割作用, 役割計画器
from .能力合成 import 登録能力, _結果辞書, _符号化
from .能力結果復元 import 能力結果を復元
from .製品版.型 import 能力結果
from .監査改善接続 import (改善能力群, 改善回答を検査, 改善回答版, 意味拡張版)
from .有限仮説探索 import 仮説探索版
from .有限因果モデル import 因果モデル版

from .資料読解 import 資料読解版

改善計画版 = 'MINIDORA-監査改善計画-v0.2'
能力名 = {'命題': '拡張命題検討', '仮説': '有限仮説検討', '介入': '有限介入比較', '読解': '資料読解'}
報告版 = {'命題': 意味拡張版, '仮説': 仮説探索版, '介入': 因果モデル版, '読解': 資料読解版}


class 改善回答照合モジュール:
    """報告自身の再計算に加え、依頼された原要求・表示条件・参照との一致を検査する。"""
    名前 = '監査改善回答照合'
    版 = 改善計画版
    優先度 = 0

    def 判定(self, 文脈):
        return 1.0

    def 実行(self, 文脈):
        try:
            aux = 文脈.補助
            rows, settings = aux['合成入力'], aux['合成設定']
            if type(rows) is not tuple or len(rows) != 2:
                raise ValueError('回答と原要求の二役割が必要')
            if type(settings) is not dict or set(settings) != {'種類', '詳細'}:
                raise ValueError('最終回答照合の設定欄不正')
            kind, detail = settings['種類'], settings['詳細']
            if type(kind) is not str or kind not in 能力名 or type(detail) is not bool:
                raise ValueError('最終回答照合の種類・詳細型不正')
            for row in rows:
                if type(row) is not dict or set(row) != {'参照', '結果'}:
                    raise ValueError('照合入力の欄不正')
            answer, original = (能力結果を復元(row['結果']) for row in rows)
            if not answer.成立 or not original.成立:
                raise ValueError('未成立の回答又は要求')
            d = answer.データ
            if (d.get('版') != 改善回答版 or not 改善回答を検査(d)
                    or d['報告']['版'] != 報告版[kind]
                    or _符号化(d['報告']['要求']) != _符号化(original.データ)
                    or d['詳細'] is not detail or answer.本文 != d['本文']
                    or answer.根拠 != ('原データ:' + 意味指紋(d['報告']),)):
                raise ValueError('回答・元要求・種類・表示条件の不一致')
            # 全参照を保持。自己整合した別入力の回答や出典の差替えを通さない。
            def references(value):
                return _符号化(sorted((r.辞書化() for r in value.参照), key=lambda r: r['識別子']))
            if references(answer) != references(original):
                raise ValueError('回答と原要求の参照不一致')
            _結果辞書(answer)
            return deepcopy(answer)
        except (ValueError, TypeError, KeyError, AttributeError, RecursionError) as exc:
            return 能力結果(False, '', 保留理由='監査改善:最終照合:' + str(exc))

    def 登録(self):
        return 登録能力(self)


def 改善統合能力群():
    return (*改善能力群(), 改善回答照合モジュール().登録())


def 改善作用群():
    def request_goal(p):
        return 意味目的('原資料', {'資料': p['要求資料']})
    rules = [
        役割作用('監査改善の原要求照合', '監査改善回答照合', '監査改善の検証済回答',
            lambda p: (('回答', 意味目的('監査改善の説明', p)), ('原要求', request_goal(p))),
            lambda p: {k: p[k] for k in ('種類', '詳細')}, lambda p: True,
            保持事項=('対象', '原要求', '出典', '仮定', '留保', '表示条件')),
        役割作用('監査改善の説明構成', '監査改善回答', '監査改善の説明',
            lambda p: (('検討', 意味目的('原資料', {'資料': '保存報告'}) if p['再説明']
                       else 意味目的('監査改善の検討:' + p['種類'], p)),),
            lambda p: {'詳細': p['詳細']}, lambda p: True,
            保持事項=('根拠', '仮定', '由来', '留保')),
    ]
    for kind, モジュール in 能力名.items():
        rules.append(役割作用('監査改善の検討:' + kind, モジュール, '監査改善の検討:' + kind,
            lambda p: (('原要求', request_goal(p)),), lambda p: {}, lambda p: True,
            保持事項=('原文', '原要求', '出典', '仮説と事実の区別')))
    return tuple(rules)


def 改善目的を計画(種類: str, 原要求: 能力結果, 登録一覧, *, 詳細: bool = True, 元報告: 能力結果 | None = None):
    if type(種類) is not str or 種類 not in 能力名 or type(詳細) is not bool:
        raise ValueError('改善目的の種類・表示条件不正')
    _結果辞書(原要求)
    if not 原要求.成立:
        raise ValueError('未成立の原要求')
    materials = {'原要求': 原要求}
    if 元報告 is not None:
        _結果辞書(元報告)
        if not 元報告.成立:
            raise ValueError('未成立の保存報告')
        materials['保存報告'] = 元報告
    goal = 意味目的('監査改善の検証済回答', {'種類': 種類, '要求資料': '原要求',
                  '詳細': 詳細, '再説明': 元報告 is not None})
    return 役割計画器(改善作用群(), 登録一覧).計画する(goal, materials)

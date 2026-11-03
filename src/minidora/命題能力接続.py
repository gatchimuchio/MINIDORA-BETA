"""命題資料・導出報告を既存の役割計画と能力合成へ接続する。"""
from __future__ import annotations

from dataclasses import asdict
import json
from .命題構造 import 命題版
from .命題解釈 import 命題を読む, 命題資料を読む
from .命題推論 import 命題推論器
from .能力合成 import 登録能力, _結果辞書, _参照結合, _符号化
from .応答構成 import 能力結果を復元
from .製品版.型 import 能力結果
from .会話意味 import 意味目的, 意味指紋
from .会話能力接続 import _入力
from .役割計画 import 役割作用


def 命題資料を構成(value, name):
    _結果辞書(value)
    if not value.成立 or value.保留理由: raise ValueError('不成立の命題原資料')
    if value.データ: raise ValueError('未知の構造資料を本文だけの命題資料へ読み替えない')
    rows = 命題資料を読む(value.本文, name)
    資料 = {'版': 命題版, '種別': '命題資料', '資料': name,
            '記載': [asdict(r) for r in rows], '原資料': _結果辞書(value)}
    資料 = json.loads(_符号化(資料))
    資料['記録SHA256'] = 意味指紋(資料)
    return 能力結果(True, f'資料「{name}」の命題記載{len(rows)}件',
                     根拠=tuple(r.識別子 for r in rows), 参照=value.参照, データ=資料)


def _必須一致(value, rebuilt):
    _結果辞書(value)
    available = {r.識別子: r for r in _参照結合(value.参照)}
    return (value.成立 is True and value.本文 == rebuilt.本文 and value.データ == rebuilt.データ
            and value.根拠 == rebuilt.根拠 and value.保留理由 == rebuilt.保留理由
            and all(available.get(r.識別子) == r for r in rebuilt.参照))


def 命題資料整合(value):
    try:
        d = value.データ
        return _必須一致(value, 命題資料を構成(能力結果を復元(d['原資料']), d['資料']))
    except (ValueError, TypeError, KeyError, AttributeError, RecursionError):
        return False


def 命題を検討(values: tuple[能力結果, ...], settings: dict):
    if type(values) is not tuple or not 1 <= len(values) <= 8:
        raise ValueError('命題資料の入力数・型')
    if type(settings) is not dict or set(settings) != {'問い', '候補'}:
        raise ValueError('命題検討の設定不正')
    candidates = 命題を読む(settings['問い'])
    index = settings['候補']
    if type(index) is not int or not 1 <= index <= len(candidates):
        raise ValueError('問いの意味候補が未確定')
    names = []; rows = []
    for value in values:
        if not 命題資料整合(value): raise ValueError('命題資料の再構成不一致')
        d = value.データ; names.append(d['資料'])
        rows.extend(命題資料を読む(d['原資料']['本文'], d['資料']))
    if len(set(names)) != len(names): raise ValueError('同じ資料の重複役割')
    answer = 命題推論器(tuple(rows)).判定(candidates[index - 1].式)
    資料 = {'版': 命題版, '種別': '命題判定', '設定': dict(settings),
            '候補': [asdict(c) for c in candidates], '原入力': [_結果辞書(v) for v in values],
            '記載': [asdict(r) for r in rows], '判定結果': answer}
    資料 = json.loads(_符号化(資料))
    資料['記録SHA256'] = 意味指紋(資料)
    # 報告生成の成功であり、問いの真偽の採用ではない。
    return 能力結果(True, '資料命題の判定：' + answer['判定'],
                     根拠=tuple(r.識別子 for r in rows),
                     参照=_参照結合(r for v in values for r in v.参照), データ=資料)


def 命題判定整合(value):
    try:
        d = value.データ
        rebuilt = 命題を検討(tuple(能力結果を復元(v) for v in d['原入力']), d['設定'])
        return _必須一致(value, rebuilt)
    except (ValueError, TypeError, KeyError, AttributeError, RecursionError):
        return False


class 命題能力モジュール:
    優先度 = 0
    版 = 命題版

    def __init__(self, 名前):
        if 名前 not in ('命題資料解釈', '命題関係判定'):
            raise ValueError('命題能力が未登録')
        self.名前 = 名前

    def 判定(self, 文脈): return 1.0

    def 実行(self, 文脈):
        try:
            values, settings = _入力(文脈)
            if self.名前 == '命題資料解釈':
                if len(values) != 1 or set(settings) != {'資料'}:
                    raise ValueError('命題資料解釈の役割・設定不正')
                return 命題資料を構成(values[0], settings['資料'])
            return 命題を検討(values, settings)
        except (ValueError, TypeError, KeyError, AttributeError, RecursionError) as exc:
            return 能力結果(False, '', 保留理由='会話失敗:意味未確定:' + str(exc))

    def 登録(self): return 登録能力(self)


def 命題能力群():
    return tuple(命題能力モジュール(n).登録() for n in ('命題資料解釈', '命題関係判定'))


def 命題作用群():
    def selected(p):
        if (type(p) is not dict or set(p) != {'資料', '問い', '候補'}
                or type(p['資料']) not in (list, tuple) or not 1 <= len(p['資料']) <= 8
                or any(type(n) is not str or not n for n in p['資料'])
                or len(set(p['資料'])) != len(p['資料'])):
            raise ValueError('命題目的の入力役割不正')
        return p
    return (
        役割作用('命題回答化', '会話回答構成', '命題回答',
            lambda p: (('判定', 意味目的('命題判定', selected({k: p[k] for k in ('資料', '問い', '候補')}))),),
            lambda p: {k: p[k] for k in ('詳細', '形式', '手順')}, lambda p: True),
        役割作用('資料命題判定', '命題関係判定', '命題判定',
            lambda p: tuple((f'資料:{i}', 意味目的('命題資料', {'資料': n}))
                            for i, n in enumerate(selected(p)['資料'])),
            lambda p: {k: p[k] for k in ('問い', '候補')}, lambda p: True),
        役割作用('資料の命題化', '命題資料解釈', '命題資料',
            lambda p: (('原資料', 意味目的('原資料', p)),),
            lambda p: {'資料': p['資料']}, lambda p: True),
    )

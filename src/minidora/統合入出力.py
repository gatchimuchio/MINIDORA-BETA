"""統合セッションのJSON通信境界。単一能力・複合計画・日英依頼を明示的に区別する。"""
from __future__ import annotations

import json
from dataclasses import fields

from .統合実行 import 受入条件
from .能力合成 import 合成計画, 合成工程, 素材参照
from .応答構成 import 能力結果を復元
from .製品版.型 import 能力結果


def JSON要求を読む(text):
    if type(text) is not str or len(text.encode('utf-8')) > 2000000:
        raise ValueError('要求の型・サイズ不正')
    def pairs(items):
        out = {}
        for key, value in items:
            if key in out:
                raise ValueError('JSONキー重複')
            out[key] = value
        return out
    def nonfinite(value):
        raise ValueError('非有限数値')
    return json.loads(text, object_pairs_hook=pairs, parse_constant=nonfinite)


def _素材(raw):
    if type(raw) is str:
        return 能力結果(True, raw)
    if type(raw) is dict and raw and set(raw) <= {'本文', 'データ'}:
        if type(raw.get('本文', '')) is not str or type(raw.get('データ', {})) is not dict:
            raise ValueError('素材の型不正')
        return 能力結果(True, raw.get('本文', ''), データ=raw.get('データ', {}))
    return 能力結果を復元(raw)


def _項目(raw, required, optional=()):
    if type(raw) is not dict or not set(required) <= set(raw) or set(raw)-set(required)-set(optional):
        raise ValueError('要求フィールド不一致')


def _計画(raw):
    _項目(raw, ('工程', '出力工程'))
    if type(raw['工程']) is not list or type(raw['出力工程']) is not list:
        raise ValueError('計画は配列で指定')
    rows = []
    for row in raw['工程']:
        _項目(row, tuple(f.name for f in fields(合成工程)))
        if type(row['入力']) is not list or type(row['能力候補']) is not list:
            raise ValueError('工程の配列型不正')
        inputs = []
        for 情報源 in row['入力']:
            _項目(情報源, ('領域', '識別子'))
            inputs.append(素材参照(**情報源))
        rows.append(合成工程(row['識別子'], tuple(row['能力候補']), row['指示参照'], tuple(inputs), row['設定参照']))
    return 合成計画(tuple(rows), tuple(raw['出力工程']))


def 統合要求を実行(session, request):
    """JSON内の資料や語句から外部権限を推測しない。セッション側の許可が上限。"""
    try:
        if type(request) is not dict:
            raise ValueError('要求は対象型')
        kind = request.get('種別')
        if kind == '一覧':
            _項目(request, ('種別',))
            return {'状態': '合格', '能力': list(session.能力一覧())}
        if kind == '初期化':
            _項目(request, ('種別',))
            session.初期化()
            return {'状態': '合格', '本文': 'セッションを初期化しました。'}
        if kind == '依頼':
            _項目(request, ('種別', '依頼'), ('資料', '入力言語'))
            materials = request.get('資料', {})
            if type(materials) is not dict:
                raise ValueError('資料は名前付き対象')
            結果 = session.応答(request['依頼'], {k:_素材(v) for k,v in materials.items()},
                                  入力言語=request.get('入力言語', 'ja'))
        elif kind == '能力':
            _項目(request, ('種別', '能力', '入力'), ('設定', '外部読取許可'))
            資料 = {'指示': 能力結果(True, '明示した能力処理'), '入力': _素材(request['入力'])}
            config = None
            if '設定' in request:
                if type(request['設定']) is not dict:
                    raise ValueError('設定は対象型')
                資料['設定'] = 能力結果(True, '', データ=request['設定'])
                config = '設定'
            plan = 合成計画((合成工程('結果',(request['能力'],),'指示',(素材参照('入力','入力'),),config),),('結果',))
            結果 = session.計画実行(plan, 資料, 外部読取許可=request.get('外部読取許可', False))
        elif kind == '計画':
            _項目(request, ('種別', '計画', '資料'), ('条件', '外部読取許可', '依頼文'))
            if type(request['資料']) is not dict or type(request.get('条件', [])) is not list:
                raise ValueError('資料・条件の型不正')
            conditions = []
            for condition in request.get('条件', []):
                _項目(condition, ('対象出力', '検査能力'), ('設定参照', '基準資料'))
                if type(condition.get('基準資料', [])) is not list:
                    raise ValueError('基準資料は配列')
                conditions.append(受入条件(condition['対象出力'], condition['検査能力'],
                                          condition.get('設定参照'), tuple(condition.get('基準資料', []))))
            結果 = session.計画実行(_計画(request['計画']), {k:_素材(v) for k,v in request['資料'].items()},
                条件=tuple(conditions), 外部読取許可=request.get('外部読取許可',False),
                依頼文=request.get('依頼文','明示された能力計画を実行'))
        else:
            raise ValueError('未対応の要求種別')
        return 結果.辞書化()
    except Exception as exc:
        return {'状態': '失敗', '本文': '', '理由': '統合通信契約不成立:'+type(exc).__name__}

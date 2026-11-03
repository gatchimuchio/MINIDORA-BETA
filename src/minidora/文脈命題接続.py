"""資料の候補別判定と取得本文を既存合成・採用境界へ接続する。"""
from __future__ import annotations

import json
from hashlib import sha256
from .文脈命題 import 文脈命題版, 文脈資料を読む, 文脈判定
from .能力合成 import 登録能力, _結果辞書, _参照結合, _符号化
from .命題能力接続 import _必須一致
from .応答構成 import 能力結果を復元
from .製品版.型 import 能力結果
from .会話能力接続 import _入力
from .会話意味 import 意味目的, 意味指紋
from .役割計画 import 役割作用
from .取得意味境界 import 取得本文の意味境界を検査


def 文脈資料群(values, names):
    if type(values) is not tuple or not 1 <= len(values) <= 8 or len(values) != len(names):
        raise ValueError('文脈入力役割の数・型')
    if len(set(names)) != len(names): raise ValueError('文脈資料の重複')
    docs = []
    for value, name in zip(values, names):
        _結果辞書(value)
        if not value.成立 or value.保留理由 or value.データ:
            raise ValueError('文脈原資料は成立した未加工本文と参照に限定する')
        docs.append(文脈資料を読む(value.本文, name))
    return tuple(docs)


def 文脈報告を構成(values, settings):
    if type(settings) is not dict or set(settings) != {'資料', '問い', '候補', '資料候補'}:
        raise ValueError('文脈判定の設定不正')
    names = settings['資料']
    if type(names) not in (tuple, list): raise ValueError('資料名の列型不正')
    docs = 文脈資料群(values, names)
    report = 文脈判定(docs, settings['問い'], settings['候補'], settings['資料候補'])
    資料 = {'版': 文脈命題版, '種別': '文脈命題判定', '設定': settings,
            '原入力': [_結果辞書(v) for v in values], '資料候補': docs, '判定結果': report}
    資料 = json.loads(_符号化(資料)); 資料['記録SHA256'] = 意味指紋(資料)
    return 能力結果(True, '資料の読みを保持した判定：' + report['判定'],
        根拠=tuple(r['識別子'] for d in docs for r in d['記載候補']),
        参照=_参照結合(r for v in values for r in v.参照), データ=資料)


def 文脈報告整合(value):
    try:
        d = value.データ
        return _必須一致(value, 文脈報告を構成(tuple(能力結果を復元(v) for v in d['原入力']), d['設定']))
    except (ValueError, TypeError, KeyError, AttributeError, RecursionError): return False


def 取得本文を資料化(value):
    """取得記録と全文参照の対応を照合。検索抜粋を完全な資料と見なさない。"""
    from .知識取得 import 知識取得版, 知識取得要求, _本文記録を復元, _照合語
    _結果辞書(value)
    if not value.成立 or value.保留理由: raise ValueError('未成立の外部取得')
    d = dict(value.データ)
    required = {'版', '状態', '意味的事実検証', '要求', '不足語', '資料', '抜粋',
                '試行', '本文取得数', '資料数', '記録SHA256'}
    if (not required <= set(d) or set(d) - required - {'継続束', '資源制御', '検索来歴'}
            or any(type(d[k]) is not list for k in ('資料', '抜粋', '不足語', '試行'))
            or type(d['資料数']) is not int or type(d['本文取得数']) is not int):
        raise ValueError('外部取得記録の欄・型不正')
    if '継続束' in d:
        from .知識取得継続 import 継続束を読む
        継続束を読む(value)
    seal = d.pop('記録SHA256')
    expected = sha256(json.dumps(d, ensure_ascii=False, sort_keys=True, allow_nan=False).encode()).hexdigest()
    if seal != expected or d['版'] != 知識取得版 or d['状態'] != '合格' or d['不足語']:
        raise ValueError('外部取得記録の整合不一致')
    settings = dict(d['要求'])
    for key in ('必要語', '優先ホスト'):
        if key in settings: settings[key] = tuple(settings[key])
    request = 知識取得要求(**settings); request.検証()
    if (not 1 <= d['資料数'] <= d['本文取得数'] <= request.最大取得数
            or d['意味的事実検証'] != '未実施'):
        raise ValueError('取得数又は意味的事実検証の記録不正')
    refs = {ref.識別子: ref for ref in value.参照}
    metadata = {row['参照ID']: row for row in d['資料']}
    if (len(refs) != len(value.参照) or len(metadata) != len(d['資料']) or set(refs) != set(metadata)
            or not request.最低資料数 <= len(refs) <= request.最大資料数 or len(refs) != d['資料数']):
        raise ValueError('取得全文と資料IDの対応不一致')
    parsed = []; names = []
    for key, ref in refs.items():
        meta = metadata[key]
        doc = _本文記録を復元(meta, ref.本文)
        取得本文の意味境界を検査(doc)
        identity = 'web:' + sha256((doc.最終URL + '\n' + doc.本文SHA256).encode()).hexdigest()[:24]
        if (ref.URL != doc.最終URL or key != identity or ref.出典 != '公開HTTPS本文'
                or ref.公開時刻 is not None or meta['公開時刻'] is not None):
            raise ValueError('参照ID・URL・出典又は公開時刻と取得記録不一致')
        names.append(key)
        parsed.append(能力結果(True, doc.本文, 根拠=(key,), 参照=(ref,)))
    covered = set()
    for p in d['抜粋']:
        text = refs[p['参照ID']].本文; a, b = p['開始'], p['終了']
        if (type(a) is not int or type(b) is not int or not 0 <= a < b <= len(text)
                or text[a:b] != p['本文']): raise ValueError('外部抜粋の原文位置不一致')
        hits = [w for w in request.必要語 if _照合語(w) in _照合語(text[a:b])]
        if hits != p['一致語']: raise ValueError('取得抜粋の照合語不一致')
        covered.update(hits)
    if (set(request.必要語) != covered or value.本文 != '\n\n'.join(p['本文'] for p in d['抜粋'])
            or not set(refs) <= set(value.根拠)):
        raise ValueError('取得本文・必要語・根拠不一致')
    return tuple(parsed), names


def 取得命題を構成(value, settings):
    if type(settings) is not dict or set(settings) != {'問い', '候補'}:
        raise ValueError('取得命題の設定不正')
    values, names = 取得本文を資料化(value)
    report = 文脈報告を構成(values, {'資料': names, **settings, '資料候補': 0})
    資料 = {'版': 文脈命題版, '種別': '取得命題判定', '元取得': _結果辞書(value),
            '設定': settings, '文脈報告': _結果辞書(report)}
    資料 = json.loads(_符号化(資料)); 資料['記録SHA256'] = 意味指紋(資料)
    return 能力結果(True, report.本文, 根拠=report.根拠, 参照=report.参照, データ=資料)


def 取得命題整合(value):
    try:
        d = value.データ
        return _必須一致(value, 取得命題を構成(能力結果を復元(d['元取得']), d['設定']))
    except (ValueError, TypeError, KeyError, AttributeError, RecursionError): return False


class 文脈命題モジュール:
    版 = 文脈命題版
    優先度 = 0

    def __init__(self, 名前):
        if 名前 not in ('文脈命題判定', '取得命題判定'): raise ValueError('文脈能力名不正')
        self.名前 = 名前

    def 判定(self, 文脈): return 1.0

    def 実行(self, 文脈):
        try:
            values, settings = _入力(文脈)
            if self.名前 == '文脈命題判定': return 文脈報告を構成(values, settings)
            if len(values) != 1: raise ValueError('取得命題は単一取得報告')
            return 取得命題を構成(values[0], settings)
        except (ValueError, TypeError, KeyError, AttributeError, RecursionError) as exc:
            return 能力結果(False, '', 保留理由='会話失敗:意味未確定:' + str(exc))

    def 登録(self): return 登録能力(self)


def 文脈命題能力群():
    return tuple(文脈命題モジュール(n).登録() for n in ('文脈命題判定', '取得命題判定'))


def 文脈命題作用群():
    def local_settings(p): return {k: p[k] for k in ('資料', '問い', '候補', '資料候補')}
    return (
        役割作用('文脈判定回答化', '会話回答構成', '文脈命題回答',
            lambda p: (('判定', 意味目的('文脈命題判定', local_settings(p))),),
            lambda p: {k: p[k] for k in ('詳細', '形式', '手順')}, lambda p: True),
        役割作用('文脈資料判定', '文脈命題判定', '文脈命題判定',
            lambda p: tuple((f'資料:{i}', 意味目的('原資料', {'資料': n})) for i, n in enumerate(p['資料'])),
            local_settings, lambda p: True),
        役割作用('取得命題回答化', '会話回答構成', '取得命題回答',
            lambda p: (('判定', 意味目的('取得命題判定', {k: p[k] for k in ('問い', '候補', '取得要求')})),),
            lambda p: {k: p[k] for k in ('詳細', '形式', '手順')}, lambda p: True),
        役割作用('取得全文の命題化', '取得命題判定', '取得命題判定',
            lambda p: (('取得', 意味目的('命題取得報告', p['取得要求'])),),
            lambda p: {k: p[k] for k in ('問い', '候補')}, lambda p: True),
        役割作用('命題の公開取得', '知識取得', '命題取得報告', lambda p: (),
            lambda p: p, lambda p: True, 外部読取=True),
    )

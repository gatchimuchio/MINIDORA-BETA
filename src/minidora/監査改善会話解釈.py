"""有限な日本語の会話行為を解釈する。資料本文を操作指示へ昇格させない。

対応外の語尾・未知欄・余分な条件を捨てない。全原文と各フィールドの位置を保持する。
"""
from __future__ import annotations
import json
import re
import unicodedata
from .能力合成 import _符号化
from .命題句 import 引用を切り出す, 最上位位置
from .命題解釈 import 命題を読む

改善会話解釈版 = 'MINIDORA-監査改善会話解釈-v0.3'
種類名 = ('命題', '仮説', '介入', '本文')


def JSONを厳格に読む(text: str, *, 最大バイト数=2_000_000):
    if type(text) is not str or len(text.encode('utf-8')) > 最大バイト数:
        raise ValueError('JSONの型・サイズ上限')
    def pairs(items):
        out = {}
        for key, value in items:
            if key in out:
                raise ValueError('JSONキー重複:' + key)
            out[key] = value
        return out
    def constant(value):
        raise ValueError('非有限JSON値')
    try:
        value = json.loads(text, object_pairs_hook=pairs, parse_constant=constant)
        _符号化(value)
    except RecursionError as exc:
        raise ValueError('JSONの深さ上限') from exc
    return value


def 名前を確認(name: str) -> str:
    if (type(name) is not str or not name or name != name.strip() or len(name) > 80
            or any(unicodedata.category(c) in ('Cc', 'Cf', 'Zl', 'Zp') or c in '「」『』' for c in name)):
        raise ValueError('資料名の型・範囲不正')
    name.encode('utf-8')
    return name


def 命題列(text: str):
    if type(text) is not str or not text.strip():
        raise ValueError('空の命題列')
    pos = 0
    out = []
    # 空の項目を落とさない。関数の引数や引用内の読点では分けない。
    for i, word in 最上位位置(text, ('、', ',')):
        out.append(text[pos:i].strip()); pos = i + len(word)
    out.append(text[pos:].strip())
    if any(not x for x in out) or len(out) > 32:
        raise ValueError('命題列の空要素又は上限')
    return out


def 真偽割当(text: str):
    if text.strip() == 'なし':
        return {}
    out = {}
    for fragment in 命題列(text):
        m = re.fullmatch(r'([A-Za-z_][A-Za-z0-9_]{0,63})\s*[=＝]\s*(真|偽|true|false)', fragment)
        if not m or m[1] in out:
            raise ValueError('真偽割当は重複のない変数=真/偽')
        out[m[1]] = m[2] in ('真', 'true')
    return out


def _式(text: str):
    if text.strip() in ('真', '偽', 'true', 'false'):
        return text.strip() in ('真', 'true')
    candidates = 命題を読む(text)
    if len(candidates) != 1:
        raise ValueError('構造式の読みを括弧で一意に指定する')
    def convert(e):
        if e.時点 != '未指定' or e.様相 != '記載':
            raise ValueError('構造式に時点・様相を混ぜない')
        if e.種別 == '原子' and not e.項:
            if not re.fullmatch('[A-Za-z_][A-Za-z0-9_]{0,63}', e.述語):
                raise ValueError('構造式の変数名不正')
            return {'参照': e.述語}
        if e.種別 == '否定':
            return {'否定': convert(e.子[0])}
        if e.種別 in ('連言', '選言'):
            return {('すべて' if e.種別 == '連言' else 'いずれか'): [convert(x) for x in e.子]}
        raise ValueError('構造式は変数・否定・連言・選言に限定する')
    return convert(candidates[0].式)


def 資料を構造化(kind: str, name: str, body: str):
    if kind not in 種類名:
        raise ValueError('資料の種類不正')
    名前を確認(name)
    if type(body) is not str or not body.strip() or len(body.encode('utf-8')) > 100_000:
        raise ValueError('資料本文の型・上限')
    if kind in ('命題', '本文'):
        return {'本文': body}, []
    if body.lstrip().startswith('{'):
        資料 = JSONを厳格に読む(body, 最大バイト数=100_000)
        allowed = ({'事実', '規則', '観測', '仮説候補', '最大仮説数', '最大試行数', '最大操作数'}
                   if kind == '仮説' else {'外生', '方程式', '介入', '観測'})
        required = {'事実', '規則', '仮説候補'} if kind == '仮説' else {'外生', '方程式'}
        if type(資料) is not dict or not required <= set(資料) <= allowed:
            raise ValueError('資料JSONの必須欄不足又は未知欄')
        return 資料, [{'欄': 'JSON', '開始': 0, '終了': len(body), '原文': body}]
    資料 = {'事実': [], '規則': []} if kind == '仮説' else {'外生': {}, '方程式': []}
    seen, spans, cursor = set(), [], 0
    empty_facts = False
    for number, raw in enumerate(body.splitlines(keepends=True), 1):
        line = raw.rstrip('\r\n')
        if not line.strip():
            cursor += len(raw); continue
        m = re.fullmatch(r'\s*([^:：]+)[:：]\s*(.*?)\s*', line)
        if not m or not m[2]:
            raise ValueError('資料の未解釈行:' + str(number))
        field, value = m[1].strip(), m[2]
        repeated = field in (('事実', '規則') if kind == '仮説' else ('構造',))
        if field in seen and not repeated:
            raise ValueError('資料欄の重複:' + field)
        seen.add(field)
        origin = name + ':行' + str(number)
        spans.append({'欄': field, '開始': cursor, '終了': cursor + len(line), '原文': line})
        cursor += len(raw)
        if kind == '仮説':
            if field == '事実':
                if (value == 'なし' and 資料['事実']) or (value != 'なし' and empty_facts):
                    raise ValueError('事実なしと事実記載を併存させない')
                empty_facts |= value == 'なし'
                if value != 'なし':
                    for i, text in enumerate(命題列(value)):
                        資料['事実'].append({'識別子': 'f' + str(number) + '_' + str(i), '命題': text, '出典': origin})
            elif field == '規則':
                candidates = 命題を読む(value)
                if len(candidates) != 1 or candidates[0].式.種別 != '含意':
                    raise ValueError('規則は一意な「前件ならば後件」が必要')
                from .命題解釈 import 命題を表現
                e = candidates[0].式
                left = e.子[0].子 if e.子[0].種別 == '連言' else (e.子[0],)
                資料['規則'].append({'識別子': 'r' + str(number), '前件': [命題を表現(x) for x in left],
                                     '後件': 命題を表現(e.子[1]), '出典': origin})
            elif field in ('候補', '観測'):
                資料['仮説候補' if field == '候補' else field] = [] if value == 'なし' else 命題列(value)
            elif field == '最大仮説数':
                if not re.fullmatch('[0-6]', value):
                    raise ValueError('最大仮説数は0〜6')
                資料[field] = int(value)
            else:
                raise ValueError('未解釈の仮説資料欄:' + field)
        else:
            if field in ('外生', '介入', '観測'):
                資料[field] = 真偽割当(value)
            elif field == '構造':
                m2 = re.fullmatch(r'([A-Za-z_][A-Za-z0-9_]{0,63})\s*[=＝]\s*(.+)', value)
                if not m2:
                    raise ValueError('構造は変数=式で指定する')
                資料['方程式'].append({'変数': m2[1], '式': _式(m2[2]), '出典': origin})
            else:
                raise ValueError('未解釈の介入資料欄:' + field)
    if kind == '仮説' and '仮説候補' not in 資料:
        raise ValueError('仮説資料には候補欄が必要')
    if kind == '介入' and (not 資料['方程式'] or '外生' not in seen):
        raise ValueError('介入資料には外生と構造が必要')
    return 資料, spans


def 改善発話を解釈(text: str) -> dict:
    if type(text) is not str or not text.strip() or len(text) > 8192:
        raise ValueError('会話原文の型・上限')
    original = text
    text = text.strip()
    def out(作用, **values):
        return {'行為': 作用, '原文': original, **values}
    for kind in 種類名:
        prefix = kind + '資料'
        if text.startswith(prefix):
            name, end = 引用を切り出す(text, len(prefix))
            suffix = text[end:]
            m = re.match(r'を(登録|更新)(?:して|してください)?[:：]\s*', suffix)
            if not m:
                raise ValueError('資料の登録・更新の語尾不正')
            body = suffix[m.end():]
            資料, spans = 資料を構造化(kind, name, body)
            return out(m[1], 種類=kind, 資料=name, 本文=body, データ=資料, 原文対応=spans)
    from .依頼表現 import 検討依頼を読む, 再説明依頼を読む
    expanded = 検討依頼を読む(original) or 再説明依頼を読む(original)
    if expanded is not None:
        if '資料' in expanded:
            名前を確認(expanded['資料'])
        return expanded
    if text.endswith('。'):
        text = text[:-1]
    if text.startswith('述語別名を'):
        value, end = 引用を切り出す(text, len('述語別名を'))
        if text[end:] not in ('にして', 'に訂正して', 'に訂正してください'):
            raise ValueError('述語別名訂正の未知語尾')
        definitions = []
        for fragment in (() if value == 'なし' else 命題列(value)):
            match = re.fullmatch(r'([^/＝=]+)/([0-4])[=＝]([^/＝=]+)', fragment)
            if match is None:
                raise ValueError('述語別名は「表記/引数数=正規名」で指定する')
            definitions.append({'表記': match[1], '正規名': match[3], '引数数': int(match[2]), '出典': original.strip()})
        from .明示語彙 import 述語別名を検査
        述語別名を検査(definitions)
        return out('訂正', 変更={'述語別名': definitions})
    if text.startswith(('資料「', '資料『')):
        name, end = 引用を切り出す(text, 2)
        名前を確認(name)
        suffix = text[end:]
        simple = {'で仮説を検討して': '仮説', 'で介入を比較して': '介入', 'で命題を判定して': '命題'}
        if suffix in simple:
            return out('検討', 種類=simple[suffix], 資料=name, 変更={})
        if suffix.startswith('で観測'):
            value, end2 = 引用を切り出す(suffix, len('で観測'))
            if suffix[end2:] not in ('を説明する仮説を検討して', 'を説明する仮説を検討してください'):
                raise ValueError('仮説検討の未知語尾')
            return out('検討', 種類='仮説', 資料=name, 変更={'観測': 命題列(value)})
        if suffix.startswith('から'):
            value, end2 = 引用を切り出す(suffix, len('から'))
            if suffix[end2:] not in ('を判定して', 'を判定してください'):
                raise ValueError('命題判定の未知語尾')
            return out('検討', 種類='命題', 資料=name, 変更={'問い': value})
        if suffix.startswith('で'):
            value, end2 = 引用を切り出す(suffix, 1)
            if suffix[end2:] not in ('に介入した結果を比較して', 'に介入した結果を比較してください'):
                raise ValueError('介入比較の未知語尾')
            return out('検討', 種類='介入', 資料=name, 変更={'介入': 真偽割当(value)})
        raise ValueError('資料を使う依頼の未対応構文')
    for field in ('観測', '候補', '介入', '問い'):
        prefix = field + 'を'
        if text.startswith(prefix):
            value, end = 引用を切り出す(text, len(prefix))
            if text[end:] not in ('に訂正して', 'に訂正してください', 'にして'):
                raise ValueError('目的訂正の未知語尾')
            key = '仮説候補' if field == '候補' else field
            converted = 真偽割当(value) if field == '介入' else 命題列(value) if field in ('観測', '候補') else value
            return out('訂正', 変更={key: converted})
    match = re.fullmatch('(問い候補|資料候補)([1-9][0-9]?)で続けて', text)
    if match:
        return out('選択', 欄=match[1], 番号=int(match[2]))
    match = re.fullmatch('照応距離を([1-9]|1[0-6])にして', text)
    if match:
        return out('訂正', 変更={'照応距離': int(match[1])})
    actions = {'監査改善の続き': ('継続', {}), '続けて': ('継続', {}), 'もう一度': ('再実行', {}),
               '短く説明して': ('再表現', {'詳細': False}), '詳しく説明して': ('再表現', {'詳細': True}),
               '監査改善の状態を表示して': ('状態', {}), '監査改善の確認を取り消して': ('取消', {})}
    if text in actions:
        作用, args = actions[text]
        return out(作用, **args)
    raise ValueError('未対応の会話行為。未解釈部分を捨てず保留する')

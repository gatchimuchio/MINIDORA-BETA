"""位置・型・行列を指定した選択と変換。原本を変更せず、操作履歴から再構成する。"""
from __future__ import annotations

from copy import deepcopy
import csv
from hashlib import sha256
import io
import json
import re

from .構造化文書 import (構造化文書版, 文書境界違反, _読む, _節, _文字,
                          _位置名, _JSON出力, _失敗)
from .能力合成 import _結果辞書
from .製品版.型 import 能力結果


def _符号(value) -> bytes:
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(',', ':'), allow_nan=False).encode('utf-8')


def _末端(node, path=''):
    kind, value = node['型'], node['値']
    if kind == '対象' and value:
        return [p for key, child in value.items() for p in _末端(child, path+'/'+_位置名(key))]
    if kind == '配列' and value:
        return [p for i, child in enumerate(value) for p in _末端(child, path+'/'+str(i))]
    return [path]


def _取得(root, pointer):
    _文字(pointer)
    if len(pointer) > 2048 or (pointer and not pointer.startswith('/')) or re.search(r'~(?![01])', pointer):
        raise 文書境界違反('JSON Pointerの構文不正')
    node = root
    for 字句 in pointer.split('/')[1:] if pointer else ():
        字句 = 字句.replace('~1', '/').replace('~0', '~')
        if node['型'] == '対象':
            if 字句 not in node['値']:
                raise KeyError(pointer)
            node = node['値'][字句]
        elif node['型'] == '配列':
            if not re.fullmatch(r'0|[1-9][0-9]*', 字句):
                raise 文書境界違反('配列位置は先行零のない非負整数')
            if len(字句) > 8 or int(字句) >= len(node['値']):
                raise KeyError(pointer)
            node = node['値'][int(字句)]
        else:
            raise KeyError(pointer)
    return node


def _子位置(path, parent):
    return path == parent or path.startswith(parent + '/')


def _項目(settings, keys):
    if type(settings) is not dict or set(settings) != set(keys):
        raise 文書境界違反('操作の設定項目が不一致')


def _列名(names):
    if type(names) is not list or not 1 <= len(names) <= 64:
        raise 文書境界違反('列名は1〜64個の配列')
    for name in names:
        _文字(name)
        if not name or len(name) > 32768:
            raise 文書境界違反('列名の空値・長さ不正')
    if len(set(names)) != len(names):
        raise 文書境界違反('列名重複')


def _CSV出力(doc):
    names = doc['列名']
    rows = ([names] if names is not None else []) + [[cell['値'] for cell in row['値']] for row in doc['構造']['値']]
    if not rows:
        raise 文書境界違反('見出しなしの空表はCSVで列構造を保持できない')
    for row in rows:
        for cell in row:
            # 外部表計算での式実行を避ける出力境界。引用符だけで安全とはしない。
            if cell.startswith(('\t', '\r', '\n')) or cell.lstrip().startswith(('=', '+', '-', '@')):
                raise 文書境界違反('CSV式解釈リスクのあるセル。値を改変せず出力保留')
    stream = io.StringIO(newline='')
    writer = csv.writer(stream, delimiter=doc['区切り'], lineterminator='\r\n')
    writer.writerows(rows)
    return stream.getvalue()


def _操作(doc, operation, settings):
    # docは再構築した局所値。元の能力結果を変更しない。
    if operation == 'JSON選択':
        _項目(settings, {'位置'})
        if doc['形式'] != 'JSON':
            raise 文書境界違反('JSON選択にはJSON文書が必要')
        p = settings['位置']
        try:
            node = _取得(doc['構造'], p)
        except KeyError as exc:
            raise 文書境界違反('必須位置が存在しない。空値で補完しない') from exc
        doc['構造'] = node
        for name in ('対応', 'キー対応'):
            doc[name] = {key[len(p):]: val for key, val in doc[name].items() if _子位置(key, p)}
    elif operation == 'CSV選択':
        _項目(settings, {'列', '行'})
        if doc['形式'] != 'CSV':
            raise 文書境界違反('CSV選択にはCSV文書が必要')
        cols, indices, rows = settings['列'], settings['行'], doc['構造']['値']
        if type(cols) is not list or not cols or len(cols) > 64:
            raise 文書境界違反('列の明示が必要')
        width = len(doc['列名']) if doc['列名'] is not None else len(rows[0]['値']) if rows else 0
        selected_cols = []
        for col in cols:
            if type(col) is str and doc['列名'] is not None and col in doc['列名']:
                col = doc['列名'].index(col)
            if type(col) is not int or not 0 <= col < width or col in selected_cols:
                raise 文書境界違反('存在しない列・重複列・列型不正')
            selected_cols.append(col)
        if indices is None:
            indices = list(range(len(rows)))
        if (type(indices) is not list or len(indices) > 2048
                or any(type(i) is not int or not 0 <= i < len(rows) for i in indices)
                or len(set(indices)) != len(indices)):
            raise 文書境界違反('存在しない行・重複行・行型不正')
        spans = {'': doc['対応']['']}
        table = []
        for new_i, old_i in enumerate(indices):
            table.append(_節('配列', [rows[old_i]['値'][j] for j in selected_cols]))
            spans['/'+str(new_i)] = doc['対応']['/'+str(old_i)]
            for new_j, old_j in enumerate(selected_cols):
                spans[f'/{new_i}/{new_j}'] = doc['対応'][f'/{old_i}/{old_j}']
        doc['構造'], doc['対応'] = _節('配列', table), spans
        if doc['列名'] is not None:
            doc['列名'] = [doc['列名'][j] for j in selected_cols]
            doc['見出し対応'] = [doc['見出し対応'][j] for j in selected_cols]
    elif operation == '型検査':
        _項目(settings, {'規則', '未指定許可'})
        rules = settings['規則']
        if type(rules) is not list or not 1 <= len(rules) <= 128 or type(settings['未指定許可']) is not bool:
            raise 文書境界違反('型検査規則の型・件数不正')
        covered, seen = set(), set()
        for rule in rules:
            _項目(rule, {'位置', '型', '必須'})
            if rule['型'] not in ('対象', '配列', '文字列', '数値', '真偽', '空値') or type(rule['必須']) is not bool:
                raise 文書境界違反('型検査の宣言不正')
            p = rule['位置']
            _文字(p)
            if p in seen:
                raise 文書境界違反('型検査位置の重複')
            seen.add(p)
            try:
                value = _取得(doc['構造'], p)
            except KeyError as exc:
                if rule['必須']:
                    raise 文書境界違反('必須項目欠落:' + p) from exc
                continue
            if value['型'] != rule['型']:
                raise 文書境界違反('型不一致:' + p)
            covered.update(q for q in _末端(doc['構造']) if _子位置(q, p))
        if not settings['未指定許可'] and set(_末端(doc['構造'])) - covered:
            raise 文書境界違反('型検査で未指定の項目が残る')
    elif operation == '形式変換':
        if doc['形式'] == 'CSV':
            _項目(settings, {'出力', '行表現'})
            if settings['出力'] != 'JSON' or settings['行表現'] not in ('配列', '対象'):
                raise 文書境界違反('CSVからJSONへの行表現を明示する')
            if settings['行表現'] == '対象':
                if doc['列名'] is None:
                    raise 文書境界違反('対象形式への変換には見出しが必要')
                newrows, spans, keyspans = [], {'': doc['対応']['']}, {}
                for i, row in enumerate(doc['構造']['値']):
                    newrows.append(_節('対象', dict(zip(doc['列名'], row['値']))))
                    spans['/'+str(i)] = doc['対応']['/'+str(i)]
                    for j, name in enumerate(doc['列名']):
                        p = '/'+str(i)+'/'+_位置名(name)
                        spans[p] = doc['対応'][f'/{i}/{j}']
                        keyspans[p] = doc['見出し対応'][j]
                doc['構造'], doc['対応'], doc['キー対応'] = _節('配列', newrows), spans, keyspans
            doc['形式'] = 'JSON'
            doc['列名'], doc['見出し対応'], doc['区切り'] = None, [], None
        else:
            _項目(settings, {'出力', '列名'})
            if settings['出力'] != 'CSV' or doc['構造']['型'] != '配列':
                raise 文書境界違反('JSONの対象配列からCSVへ変換する')
            names = settings['列名']; _列名(names)
            table, spans = [], {'': doc['対応']['']}
            for i, row in enumerate(doc['構造']['値']):
                if row['型'] != '対象' or set(row['値']) != set(names):
                    raise 文書境界違反('CSV変換で欠落項目を補完・余剰項目を破棄しない')
                vals = []
                spans['/'+str(i)] = doc['対応']['/'+str(i)]
                for j, name in enumerate(names):
                    cell = row['値'][name]
                    if cell['型'] != '文字列':
                        raise 文書境界違反('CSV化で数値・空値・複合型を文字列へ暗黙変換しない')
                    vals.append(cell)
                    spans[f'/{i}/{j}'] = doc['対応']['/'+str(i)+'/'+_位置名(name)]
                table.append(_節('配列', vals))
            doc['構造'], doc['対応'] = _節('配列', table), spans
            doc['形式'], doc['列名'], doc['区切り'] = 'CSV', names, ','
            doc['見出し対応'] = [doc['キー対応'].get('/0/'+_位置名(name)) for name in names]
            doc['キー対応'] = {}
    elif operation == '値取出':
        _項目(settings, {'型'})
        if settings['型'] not in ('文字列', '数値', '真偽', '空値') or doc['構造']['型'] != settings['型']:
            raise 文書境界違反('値取出には指定型の単一値が必要')
        return doc['構造']['値'] if settings['型'] == '文字列' else _JSON出力(doc['構造'])
    else:
        raise 文書境界違反('未対応の文書操作')
    return _CSV出力(doc) if doc['形式'] == 'CSV' else _JSON出力(doc['構造'])


def _結果(情報源, operations, doc, body):
    original = _読む(情報源['原文'], 情報源['形式'], 情報源['区切り'], 情報源['見出し'])
    leaves = _末端(original['構造'])
    used = {doc['対応'][p]['原位置'] for p in _末端(doc['構造']) if p in doc['対応']}
    omitted = [p for p in leaves if p not in used]
    header_links = doc['見出し対応'] + list(doc['キー対応'].values())
    omitted_headers = [name for name, span in zip(original['列名'] or [], original['見出し対応'])
                       if span not in header_links]
    資料 = {"版": 構造化文書版, "原本": deepcopy(情報源), "操作列": deepcopy(operations),
            "文書": deepcopy(doc), "未選択位置": omitted, "全原値保持": not omitted,
            "未選択見出し": omitted_headers, "値保持の範囲": "原本の末端値のみ。キー・見出し・空白の完全同一性ではない",
            "原文SHA256": sha256(情報源['原文'].encode('utf-8')).hexdigest(),
            "保証範囲": "形式・位置・型の処理。内容の真偽・意味的十分性・選択の妥当性は未判定"}
    資料['記録SHA256'] = sha256(_符号({'本文': body, 'データ': 資料})).hexdigest()
    結果 = 能力結果(True, body, データ=資料)
    if len(_符号(_結果辞書(結果))) > 2000000:
        raise 文書境界違反('文書記録サイズ上限')
    return 結果


def _再構成(情報源, operations):
    _項目(情報源, {'原文', '形式', '区切り', '見出し'})
    if type(operations) is not list or len(operations) > 8:
        raise 文書境界違反('文書操作列の型・長さ不正')
    doc = _読む(情報源['原文'], 情報源['形式'], 情報源['区切り'], 情報源['見出し'])
    body = 情報源['原文']
    for row in operations:
        _項目(row, {'操作', '設定'})
        body = _操作(doc, row['操作'], row['設定'])
    return _結果(情報源, operations, doc, body)


def 文書記録整合(結果: 能力結果) -> bool:
    try:
        _結果辞書(結果)
        if 結果.成立 is not True or 結果.保留理由 or 結果.根拠 or 結果.データ['版'] != 構造化文書版:
            return False
        expected = _再構成(結果.データ['原本'], 結果.データ['操作列'])
        return expected.本文 == 結果.本文 and _符号(expected.データ) == _符号(結果.データ)
    except (ValueError, TypeError, KeyError, RecursionError, OverflowError):
        return False


def 文書を処理(文書: 能力結果, 操作: str, 設定: dict) -> 能力結果:
    try:
        if not 文書記録整合(文書):
            raise 文書境界違反('文書記録と原本からの再構成不一致')
        operations = deepcopy(文書.データ['操作列']) + [{'操作': 操作, '設定': deepcopy(設定)}]
        out = _再構成(deepcopy(文書.データ['原本']), operations)
        return 能力結果(True, out.本文, 参照=文書.参照, データ=out.データ)
    except (ValueError, TypeError, KeyError, RecursionError, OverflowError) as exc:
        return _失敗(exc)

"""利用者が明示した同義述語だけを、項数付きで対応させる。

固有名、帰属・引用内部、未知語の意味は推測しない。語彙知識を内蔵しない。
"""
from __future__ import annotations
from dataclasses import replace
import re
from .命題構造 import 命題式

明示語彙版 = 'MINIDORA-明示述語対応-v0.1'


def 述語別名を検査(rows):
    if type(rows) not in (tuple, list) or len(rows) > 32:
        raise ValueError('述語別名は最大32件の明示定義')
    mapping = {}
    for row in rows:
        if type(row) is not dict or set(row) != {'表記', '正規名', '引数数', '出典'}:
            raise ValueError('述語別名の欄不正')
        if any(type(row[k]) is not str or re.fullmatch(r'[^\W\d]\w{0,79}', row[k]) is None
               for k in ('表記', '正規名')):
            raise ValueError('別名は単一の述語名')
        if type(row['引数数']) is not int or not 0 <= row['引数数'] <= 4:
            raise ValueError('述語別名の引数数は0〜4')
        if (type(row['出典']) is not str or not row['出典'].strip() or len(row['出典']) > 8192
                or any(ord(c) < 32 for c in row['出典'])):
            raise ValueError('述語別名には明示定義の出典が必要')
        key = (row['表記'], row['引数数'])
        if key in mapping or row['表記'] == row['正規名']:
            raise ValueError('述語別名の重複・自己対応')
        mapping[key] = row['正規名']
    if any((name, count) in mapping for (_, count), name in mapping.items()):
        raise ValueError('別名の連鎖・循環は暗黙解消しない。共通の正規名を直接指定する')
    return mapping


def 命題の述語を対応付ける(expression: 命題式, mapping, *, 経路=()):
    if expression.種別 == '帰属':
        # 別名による外側の同一視を、話者の信念や引用内容へ持ち込まない。
        return expression, []
    if expression.種別 == '原子':
        name = mapping.get((expression.述語, len(expression.項)))
        if name is None:
            return expression, []
        return replace(expression, 述語=name), [{'経路': list(経路), '表記': expression.述語,
                                                '正規名': name, '引数数': len(expression.項)}]
    children, 追跡 = [], []
    for index, child in enumerate(expression.子):
        value, rows = 命題の述語を対応付ける(child, mapping, 経路=(*経路, index))
        children.append(value)
        追跡.extend(rows)
    return replace(expression, 子=tuple(children)), 追跡

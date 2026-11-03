'日本語の構造化アルゴリズムからPython ASTを構成する。自由文や試験値からの推測ではない。\n\n数値等の実値は定数資料として分離し、生成コードは定数辞書の参照だけを持つ。\n'
from __future__ import annotations

import ast
from copy import deepcopy
from hashlib import sha256
import json

from .コード能力 import 識別子を確認, 値を確認, _解析, コード能力版
from .製品版.型 import 能力結果

_算術 = {"加算": ast.Add, "減算": ast.Sub, "乗算": ast.Mult, "整数商": ast.FloorDiv, "剰余": ast.Mod}
_比較 = {"等値": ast.Eq, "非等値": ast.NotEq, "未満": ast.Lt, "以下": ast.LtE, "超": ast.Gt, "以上": ast.GtE}
_関数 = {"合計": "sum", "長さ": "len", "範囲": "range", "絶対値": "abs",
         "最小": "min", "最大": "max", "整列": "sorted", "リスト化": "list", "すべて": "all", "いずれか": "any"}


def 関数を生成(仕様: dict, 定数: dict) -> 能力結果:
    count = 0
    def shape(node, keys):
        nonlocal count
        count += 1
        if count > 512 or type(node) is not dict or set(node) != set(keys):
            raise ValueError("構造化仕様の項目または規模不正")

    def name(value):
        識別子を確認(value)
        if value == "定数" or value in _関数.values():
            raise ValueError("予約名の上書き")
        return value

    def expr(node, depth=0):
        if depth > 24 or type(node) is not dict:
            raise ValueError("式の深さ・型不正")
        kind = node.get("種別")
        if kind == "参照":
            shape(node, ("種別", "名前"))
            return ast.Name(id=name(node["名前"]), ctx=ast.Load())
        if kind == "定数参照":
            shape(node, ("種別", "キー"))
            if type(node["キー"]) is not str or node["キー"] not in 定数:
                raise ValueError('必要な定数資料がない')
            return ast.Subscript(value=ast.Name(id="定数", ctx=ast.Load()), slice=ast.Constant(value=node["キー"]), ctx=ast.Load())
        if kind in ("算術", "比較"):
            shape(node, ("種別", "演算", "左", "右"))
            left, right = expr(node["左"], depth+1), expr(node["右"], depth+1)
            if kind == "算術":
                return ast.BinOp(left=left, op=_算術[node["演算"]](), right=right)
            return ast.Compare(left=left, ops=[_比較[node["演算"]]()], comparators=[right])
        if kind == "選択":
            shape(node, ("種別", "条件", "真", "偽"))
            return ast.IfExp(test=expr(node["条件"], depth+1), body=expr(node["真"], depth+1), orelse=expr(node["偽"], depth+1))
        if kind == "リスト":
            shape(node, ("種別", "要素"))
            if type(node["要素"]) is not list:
                raise ValueError("要素はlist")
            return ast.List(elts=[expr(v, depth+1) for v in node["要素"]], ctx=ast.Load())
        if kind == "添字":
            shape(node, ("種別", "対象", "位置"))
            return ast.Subscript(value=expr(node["対象"], depth+1), slice=expr(node["位置"], depth+1), ctx=ast.Load())
        if kind == "呼出":
            shape(node, ("種別", "関数", "引数"))
            if type(node["引数"]) is not list:
                raise ValueError("引数はlist")
            return ast.Call(func=ast.Name(id=_関数[node["関数"]], ctx=ast.Load()), args=[expr(a, depth+1) for a in node["引数"]], keywords=[])
        raise ValueError("未対応のアルゴリズム式")

    def block(rows, depth=0):
        if type(rows) is not list or depth > 16 or len(rows) > 128:
            raise ValueError("手順の型・深さ・数不正")
        結果 = []
        for node in rows:
            if type(node) is not dict:
                raise ValueError("手順型不正")
            kind = node.get("種別")
            if kind == "代入":
                shape(node, ("種別", "名前", "式"))
                結果.append(ast.Assign(targets=[ast.Name(id=name(node["名前"]), ctx=ast.Store())], value=expr(node["式"])))
            elif kind == "返却":
                shape(node, ("種別", "式"))
                結果.append(ast.Return(value=expr(node["式"])))
            elif kind == "分岐":
                shape(node, ("種別", "条件", "真", "偽"))
                結果.append(ast.If(test=expr(node["条件"]), body=block(node["真"], depth+1), orelse=block(node["偽"], depth+1)))
            elif kind == "反復":
                shape(node, ("種別", "変数", "対象", "手順"))
                結果.append(ast.For(target=ast.Name(id=name(node["変数"]), ctx=ast.Store()), iter=expr(node["対象"]), body=block(node["手順"], depth+1), orelse=[]))
            else:
                raise ValueError("未対応のアルゴリズム手順")
        return 結果 or [ast.Pass()]
    try:
        shape(仕様, ("名前", "引数", "手順"))
        値を確認(定数)
        if type(定数) is not dict or type(仕様["引数"]) is not list or len(仕様["引数"]) > 15:
            raise ValueError("定数・引数の型不正")
        args = [name(n) for n in 仕様["引数"]]
        if len(set(args)) != len(args):
            raise ValueError("引数重複")
        tree = ast.Module(body=[ast.FunctionDef(name=name(仕様["名前"]), args=ast.arguments(
            posonlyargs=[], args=[ast.arg(arg=n) for n in args+["定数"]], vararg=None,
            kwonlyargs=[], kw_defaults=[], kwarg=None, defaults=[]), body=block(仕様["手順"]),
            decorator_list=[], returns=None, type_comment=None)], type_ignores=[])
        tree = ast.fix_missing_locations(tree)
        情報源 = ast.unparse(tree) + "\n"
        _解析(情報源)
        return 能力結果(True, 情報源, データ={"版": コード能力版, "用途": "生成した限定Python関数",
            "仕様": deepcopy(仕様), "定数": deepcopy(定数),
            "仕様SHA256": sha256(json.dumps(仕様, ensure_ascii=False, sort_keys=True).encode()).hexdigest(),
            "ソースSHA256": sha256(情報源.encode()).hexdigest(),
            "検証": '構文境界のみ。入出力試験は別モジュール'})
    except (ValueError, TypeError, KeyError, SyntaxError, RecursionError, OverflowError) as exc:
        return 能力結果(False, "", 保留理由="コード生成不成立:" + type(exc).__name__)

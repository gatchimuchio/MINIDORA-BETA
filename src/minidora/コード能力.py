'限定Python関数をASTとして読み、許可した操作だけを独自評価する。\n\n任意コードのexec/評価、import、属性アクセス、外部作用は実行しない。\n通常Pythonの完全な実行器でも、OSの隔離機構でもない。\n'
from __future__ import annotations

import ast
from collections.abc import Callable
from copy import deepcopy
from hashlib import sha256
import json
import keyword
import operator
import unicodedata

from .製品版.型 import 能力結果

コード能力版 = "MINIDORA-コード能力-v0.1"
_算術 = {ast.Add: operator.add, ast.Sub: operator.sub, ast.Mult: operator.mul,
         ast.FloorDiv: operator.floordiv, ast.Mod: operator.mod}
_比較 = {ast.Eq: operator.eq, ast.NotEq: operator.ne, ast.Lt: operator.lt,
         ast.LtE: operator.le, ast.Gt: operator.gt, ast.GtE: operator.ge,
         ast.In: lambda a, b: a in b, ast.NotIn: lambda a, b: a not in b}
_呼出名 = {"abs", "len", "min", "max", "sum", "range", "sorted", "list", "all", "any"}
_許可型 = {ast.Module, ast.FunctionDef, ast.arguments, ast.arg, ast.Return,
           ast.Assign, ast.AugAssign, ast.If, ast.For, ast.Break, ast.Continue,
           ast.Pass, ast.Expr, ast.Name, ast.Constant, ast.List, ast.Dict,
           ast.Subscript, ast.BinOp, ast.UnaryOp, ast.BoolOp, ast.Compare,
           ast.IfExp, ast.Call, ast.Load, ast.Store, ast.UAdd, ast.USub,
           ast.Not, ast.And, ast.Or, *_算術, *_比較}


class コード境界違反(ValueError):
    pass


class _停止(Exception):
    pass


class _返却(Exception):
    def __init__(self, value):
        self.value = value


class _反復制御(Exception):
    def __init__(self, kind):
        self.kind = kind


def 識別子を確認(name: str) -> None:
    if (type(name) is not str or not name.isidentifier() or keyword.iskeyword(name)
            or name.startswith("__") or len(name) > 80
            or name != unicodedata.normalize("NFKC", name)):
        raise コード境界違反("識別子不正")


def 値を確認(value, *, 内部: bool = False) -> None:
    count, text_size = 0, 0
    def visit(x, depth):
        nonlocal count, text_size
        count += 1
        if count > 4096 or depth > 16:
            raise コード境界違反("値の規模上限")
        if x is None or type(x) is bool:
            return
        if type(x) is int and x.bit_length() <= 256:
            return
        if type(x) is str and len(x) <= 8192:
            text_size += len(x.encode("utf-8"))
            if text_size > 65536:
                raise コード境界違反('文字資料合計上限')
            return
        if 内部 and type(x) is range and len(x) <= 2048:
            return
        if type(x) is list and len(x) <= 2048:
            for item in x:
                visit(item, depth + 1)
            return
        if type(x) is dict and len(x) <= 2048 and all(type(k) is str for k in x):
            for key, item in x.items():
                visit(key, depth + 1)
                visit(item, depth + 1)
            return
        raise コード境界違反("未対応の値型または値上限")
    visit(value, 0)


def _解析(情報源: str):
    if type(情報源) is not str or not 情報源.strip() or len(情報源.encode("utf-8")) > 32768:
        raise コード境界違反("ソース型・サイズ不正")
    tree = ast.parse(情報源, mode="exec", feature_version=(3, 11))
    if len(tree.body) != 1 or type(tree.body[0]) is not ast.FunctionDef:
        raise コード境界違反("単一関数だけを指定する")
    function = tree.body[0]
    args = function.args
    if (function.decorator_list or function.returns or function.type_comment
            or getattr(function, "type_params", ()) or args.defaults or args.kw_defaults
            or args.posonlyargs or args.kwonlyargs or args.vararg or args.kwarg):
        raise コード境界違反("デコレータ・注釈・特殊引数は未対応")
    names = [a.arg for a in args.args]
    if len(names) > 16 or len(set(names)) != len(names):
        raise コード境界違反("引数数または重複")
    count = 0
    pending = [(tree, 0, 0)]
    while pending:
        node, depth, loops = pending.pop()
        count += 1
        if count > 2048 or depth > 40:
            raise コード境界違反("構文規模上限")
        if type(node) not in _許可型:
            raise コード境界違反("未対応構文:" + type(node).__name__)
        if isinstance(node, ast.FunctionDef):
            if node is not function:
                raise コード境界違反("入れ子関数は未対応")
            識別子を確認(node.name)
            if node.name in _呼出名:
                raise コード境界違反("予約呼出名の再定義")
        if isinstance(node, (ast.Name, ast.arg)):
            name = node.id if isinstance(node, ast.Name) else node.arg
            識別子を確認(name)
            if isinstance(node, ast.arg) and (node.annotation or node.arg in _呼出名):
                raise コード境界違反("引数注釈または予約呼出名の上書き")
            if isinstance(node, ast.Name) and isinstance(node.ctx, ast.Store) and name in _呼出名:
                raise コード境界違反("予約呼出名の上書き")
        if isinstance(node, ast.Constant):
            値を確認(node.value)
        if isinstance(node, ast.Assign) and (len(node.targets) != 1 or type(node.targets[0]) is not ast.Name):
            raise コード境界違反("代入先は単一変数のみ")
        if isinstance(node, ast.AugAssign) and type(node.target) is not ast.Name:
            raise コード境界違反("累算先は単一変数のみ")
        if isinstance(node, ast.For):
            if type(node.target) is not ast.Name or node.type_comment:
                raise コード境界違反("反復先は単一変数のみ")
        if isinstance(node, (ast.Break, ast.Continue)) and not loops:
            raise コード境界違反("反復外のbreak/continue")
        if isinstance(node, ast.Expr) and not (node is function.body[0] and isinstance(node.value, ast.Constant)
                                                and type(node.value.value) is str):
            raise コード境界違反("単独式は先頭docstringのみ")
        if isinstance(node, ast.Dict) and any(k is None for k in node.keys):
            raise コード境界違反("辞書展開は未対応")
        if isinstance(node, ast.Call) and (type(node.func) is not ast.Name or node.func.id not in _呼出名 or node.keywords):
            raise コード境界違反("未許可の呼出またはキーワード引数")
        for child in ast.iter_child_nodes(node):
            # forのelseは、そのfor自身のbreak/continueの作用域ではない。
            inner = loops + 1 if isinstance(node, ast.For) and child in node.body else loops
            pending.append((child, depth + 1, inner))
    return function, count


def コードを読む(情報源: str) -> 能力結果:
    try:
        function, count = _解析(情報源)
        nodes = []
        for node in ast.walk(function):
            if isinstance(node, (ast.stmt, ast.Call, ast.Compare)):
                nodes.append({"構文": type(node).__name__, "行": node.lineno,
                    "UTF8列": node.col_offset, "終了行": node.end_lineno,
                    "終了UTF8列": node.end_col_offset, "原文": ast.get_source_segment(情報源, node)})
        資料 = {"版": コード能力版, "ソースSHA256": sha256(情報源.encode()).hexdigest(),
                "関数": function.name, "引数": [a.arg for a in function.args.args],
                "構文数": count, "構造": nodes,
                "注意": "限定構文の構造読解。全経路の型・値・仕様適合は未検証"}
        text = f"関数{function.name}。引数は{len(function.args.args)}個。"
        text += f"分岐{sum(type(n) is ast.If for n in ast.walk(function))}、反復{sum(type(n) is ast.For for n in ast.walk(function))}。"
        return 能力結果(True, text, データ=資料)
    except (SyntaxError, ValueError, TypeError, RecursionError, OverflowError) as exc:
        return 能力結果(False, "", 保留理由="コード解析不成立:" + type(exc).__name__,
                        データ={"診断": str(exc) if isinstance(exc, コード境界違反) else type(exc).__name__,
                                "行": getattr(exc, "lineno", None), "列": getattr(exc, "offset", None)})


class _評価器:
    def __init__(self, 予算: int, stop, local_names):
        self.予算, self.stop, self.steps = 予算, stop, 0
        self.line = 0
        self.local_names = local_names
        self.branches = []

    def tick(self, node=None):
        self.steps += 1
        if node is not None:
            self.line = node.lineno
        if self.steps > self.予算:
            raise コード境界違反("評価手数上限")
        if self.stop is not None and (self.steps == 1 or self.steps % 32 == 0):
            self.check_stop()

    def check_stop(self):
        if self.stop is not None:
            value = self.stop()
            if type(value) is not bool:
                raise コード境界違反("停止判定型不正")
            if value:
                raise _停止()

    def binary(self, op, a, b):
        if type(op) in (ast.Sub, ast.FloorDiv, ast.Mod) and (type(a) not in (int, bool) or type(b) not in (int, bool)):
            raise コード境界違反("減算・商・剰余は整数のみ")
        if type(op) is ast.Mult:
            sequence, times = (a, b) if type(a) in (list, str) else (b, a)
            if type(sequence) in (list, str) and type(times) in (int, bool):
                if len(sequence) * max(times, 0) > (2048 if type(sequence) is list else 8192):
                    raise コード境界違反("反復積の値上限")
        if type(op) is ast.Add and type(a) in (list, str) and type(a) is type(b):
            if len(a) + len(b) > (2048 if type(a) is list else 8192):
                raise コード境界違反("結合値上限")
        value = _算術[type(op)](a, b)
        値を確認(value, 内部=True)
        return value

    def expression(self, node, env):
        self.tick(node)
        if type(node) is ast.Constant:
            value = node.value
        elif type(node) is ast.Name:
            if node.id not in env:
                if node.id in self.local_names:
                    raise UnboundLocalError()
                raise NameError()
            value = env[node.id]
        elif type(node) is ast.List:
            value = [self.expression(x, env) for x in node.elts]
        elif type(node) is ast.Dict:
            value = {}
            for k, v in zip(node.keys, node.values):
                key = self.expression(k, env)
                if type(key) is not str:
                    raise コード境界違反("辞書キーは文字列のみ")
                value[key] = self.expression(v, env)
        elif type(node) is ast.BinOp:
            value = self.binary(node.op, self.expression(node.left, env), self.expression(node.right, env))
        elif type(node) is ast.UnaryOp:
            operand = self.expression(node.operand, env)
            value = (not operand) if type(node.op) is ast.Not else (+operand if type(node.op) is ast.UAdd else -operand)
        elif type(node) is ast.BoolOp:
            value = self.expression(node.values[0], env)
            for sub in node.values[1:]:
                if type(node.op) is ast.And and not value or type(node.op) is ast.Or and value:
                    break
                value = self.expression(sub, env)
        elif type(node) is ast.Compare:
            left = self.expression(node.left, env)
            value = True
            for op, sub in zip(node.ops, node.comparators):
                right = self.expression(sub, env)
                if not _比較[type(op)](left, right):
                    value = False
                    break
                left = right
        elif type(node) is ast.IfExp:
            value = self.expression(node.body if self.expression(node.test, env) else node.orelse, env)
        elif type(node) is ast.Subscript:
            value = self.expression(node.value, env)[self.expression(node.slice, env)]
        elif type(node) is ast.Call:
            values = [self.expression(a, env) for a in node.args]
            name = node.func.id
            if name == "range":
                value = range(*values)
            elif name == "abs":
                if len(values) != 1:
                    raise TypeError()
                value = abs(values[0])
            elif name == "len":
                if len(values) != 1:
                    raise TypeError()
                value = len(values[0])
            else:
                if name in ("min", "max") and len(values) > 1:
                    seq = values
                else:
                    if len(values) != 1:
                        raise TypeError()
                    seq = values[0]
                if type(seq) not in (list, str, dict, range) or len(seq) > 2048:
                    raise コード境界違反("集約入力の型・規模不正")
                for _ in seq:
                    self.tick()
                if name == "sum":
                    value = 0
                    for item in seq:
                        if type(item) not in (int, bool):
                            raise TypeError()
                        value = self.binary(ast.Add(), value, item)
                else:
                    value = {"min": min, "max": max, "sorted": sorted, "list": list,
                             "all": all, "any": any}[name](seq)
        else:
            raise コード境界違反("未対応式")
        値を確認(value, 内部=True)
        return value

    def block(self, body, env):
        for node in body:
            self.tick(node)
            if type(node) is ast.Return:
                raise _返却(self.expression(node.value, env) if node.value is not None else None)
            if type(node) is ast.Assign:
                env[node.targets[0].id] = self.expression(node.value, env)
            elif type(node) is ast.AugAssign:
                if node.target.id not in env:
                    raise UnboundLocalError()
                a, b = env[node.target.id], self.expression(node.value, env)
                # list += のalias意味を数値の累算へ読み替えない。
                if type(a) not in (int, bool) or type(b) not in (int, bool):
                    raise コード境界違反("累算は整数のみ")
                env[node.target.id] = self.binary(node.op, a, b)
            elif type(node) is ast.If:
                take = bool(self.expression(node.test, env))
                if len(self.branches) < 128:
                    self.branches.append({"行": node.lineno, "条件": take})
                self.block(node.body if take else node.orelse, env)
            elif type(node) is ast.For:
                seq = self.expression(node.iter, env)
                if type(seq) not in (list, str, dict, range) or len(seq) > 2048:
                    raise コード境界違反("反復入力の型・規模不正")
                for item in seq:
                    self.tick(node)
                    env[node.target.id] = item
                    try:
                        self.block(node.body, env)
                    except _反復制御 as control:
                        if control.kind == "break":
                            break
                else:
                    self.block(node.orelse, env)
            elif type(node) is ast.Break:
                raise _反復制御("break")
            elif type(node) is ast.Continue:
                raise _反復制御("continue")
            elif type(node) not in (ast.Pass, ast.Expr):
                raise コード境界違反("未対応文")


def コードを評価(情報源: str, arguments: dict, *, 最大手数: int = 50000,
                 停止要求: Callable[[], bool] | None = None) -> 能力結果:
    machine = None
    try:
        if type(最大手数) is not int or not 1 <= 最大手数 <= 200000:
            raise コード境界違反("評価予算不正")
        function, _ = _解析(情報源)
        値を確認(arguments)
        if type(arguments) is not dict or set(arguments) != {a.arg for a in function.args.args}:
            raise コード境界違反("引数名が関数宣言と不一致")
        locals_ = set(arguments) | {n.id for n in ast.walk(function) if isinstance(n, ast.Name) and isinstance(n.ctx, ast.Store)}
        machine = _評価器(最大手数, 停止要求, locals_)
        value = None
        machine.check_stop()
        try:
            machine.block(function.body, deepcopy(arguments))
        except _返却 as returned:
            value = returned.value
        machine.check_stop()
        値を確認(value)
        return 能力結果(True, json.dumps(value, ensure_ascii=False), データ={
            "版": コード能力版, "値": deepcopy(value), "手数": machine.steps, "最大手数": 最大手数,
            "入力SHA256": sha256(json.dumps(arguments, ensure_ascii=False, sort_keys=True).encode()).hexdigest(),
            "分岐": machine.branches, "ソースSHA256": sha256(情報源.encode()).hexdigest(),
            "範囲": "限定Python AST評価。OS上で対象コードを実行していない"})
    except _停止:
        return 能力結果(False, "", 保留理由="コード評価中止")
    except Exception as exc:
        return 能力結果(False, "", 保留理由="コード評価不成立:" + type(exc).__name__, データ={
            "診断": str(exc) if isinstance(exc, コード境界違反) else type(exc).__name__,
            "最終評価行": machine.line if machine else getattr(exc, "lineno", None),
            "手数": machine.steps if machine else 0})


def コードを検証(情報源: str, cases: tuple[dict, ...], *, 最大手数: int = 50000) -> 能力結果:
    try:
        _解析(情報源)
        if type(cases) is not tuple or not 1 <= len(cases) <= 32:
            raise コード境界違反("試験数は1〜32")
        for case in cases:
            if type(case) is not dict or set(case) != {"引数", "期待値"}:
                raise コード境界違反("試験項目不正")
            値を確認(case)
        rows = []
        for index, case in enumerate(cases):
            value = コードを評価(情報源, case["引数"], 最大手数=最大手数)
            # boolとint、listと他の型を期待値判定で同一視しない。
            equal = value.成立 and json.dumps(value.データ["値"], sort_keys=True, ensure_ascii=False) == json.dumps(case["期待値"], sort_keys=True, ensure_ascii=False)
            rows.append({"番号": index, "一致": bool(equal), "評価成立": value.成立,
                         "実値": value.データ.get("値"), "診断": value.データ.get("診断"),
                         "手数": value.データ.get("手数", 0)})
        ok = all(row["一致"] for row in rows)
        return 能力結果(ok, "指定試験は全件一致" if ok else "", データ={
            "版": コード能力版, "試験": rows, "試験数": len(rows),
            "ソースSHA256": sha256(情報源.encode()).hexdigest(),
            "試験SHA256": sha256(json.dumps(cases, sort_keys=True, ensure_ascii=False).encode()).hexdigest(),
            "保証範囲": "指定された有限入力の一致。全入力での正しさではない"},
            保留理由="" if ok else "コード試験不一致または評価未成立")
    except Exception as exc:
        return 能力結果(False, "", 保留理由="コード検証不成立:" + type(exc).__name__)

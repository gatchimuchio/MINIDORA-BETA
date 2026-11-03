"""有理数係数の多変数多項式を正確に扱う。一般CASや自由文理解ではない。"""
from __future__ import annotations

import ast
from collections.abc import Callable
from copy import deepcopy
from fractions import Fraction
from hashlib import sha256
import json
import keyword
import re
import unicodedata

from .製品版.型 import 能力結果

数学能力版 = "MINIDORA-数学記号-v0.1"
多項式 = dict[tuple[int, ...], Fraction]


class 数学境界違反(ValueError):
    pass


def _有理(value: str) -> Fraction:
    if type(value) is not str or len(value) > 640 or not re.fullmatch(
            r"[+-]?(?:[0-9]+/[1-9][0-9]*|(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][+-]?[0-9]{1,3})?)", value):
        raise 数学境界違反("数値は整数・小数・分数の文字列")
    if re.search(r"[eE]", value) and abs(int(re.split(r"[eE]", value)[1])) > 128:
        raise 数学境界違反("指数の上限")
    return _係数(Fraction(value))


def _係数(value: Fraction) -> Fraction:
    if value.numerator.bit_length() > 1024 or value.denominator.bit_length() > 1024:
        raise 数学境界違反("有理数の桁数上限")
    return value


def _変数確認(names: tuple[str, ...]) -> None:
    if type(names) is not tuple or len(names) > 8:
        raise 数学境界違反("宣言変数は0〜8個のtuple")
    for name in names:
        if (type(name) is not str or len(name) > 64 or not name.isidentifier()
                or keyword.iskeyword(name) or name.startswith("__")
                or name != unicodedata.normalize("NFKC", name)):
            raise 数学境界違反("変数名不正")
    if len(set(names)) != len(names):
        raise 数学境界違反("変数重複")


class _予算:
    def __init__(self, limit: int, stop: Callable[[], bool] | None = None):
        if type(limit) is not int or not 1 <= limit <= 1_000_000:
            raise 数学境界違反("演算上限不正")
        self.上限, self.停止, self.回数 = limit, stop, 0
        self.停止確認()

    def 停止確認(self):
        if self.停止 is not None:
            value = self.停止()
            if type(value) is not bool:
                raise 数学境界違反("停止判定型不正")
            if value:
                raise 数学境界違反("停止要求")

    def 進める(self):
        self.回数 += 1
        if self.回数 > self.上限:
            raise 数学境界違反("演算数上限")
        if self.回数 % 64 == 0:
            self.停止確認()


class _多項式処理:
    def __init__(self, names: tuple[str, ...], 予算: _予算):
        _変数確認(names)
        self.変数, self.予算 = names, 予算
        self.零次数 = (0,) * len(names)

    def 定数(self, value: Fraction) -> 多項式:
        return {self.零次数: _係数(value)} if value else {}

    def 整理(self, value: 多項式) -> 多項式:
        結果 = {}
        for powers, coefficient in value.items():
            self.予算.進める()
            if len(powers) != len(self.変数) or sum(powers) > 64:
                raise 数学境界違反("総次数上限")
            if coefficient:
                結果[powers] = _係数(coefficient)
                if len(結果) > 256:
                    raise 数学境界違反("多項式項数上限")
        return 結果

    def 加算(self, left: 多項式, right: 多項式, sign=1) -> 多項式:
        out = dict(left)
        for powers, coefficient in right.items():
            self.予算.進める()
            out[powers] = _係数(out.get(powers, Fraction(0)) + sign * coefficient)
        return self.整理(out)

    def 乗算(self, left: 多項式, right: 多項式) -> 多項式:
        out: 多項式 = {}
        for a, x in left.items():
            for b, y in right.items():
                self.予算.進める()
                powers = tuple(i+j for i, j in zip(a, b))
                if sum(powers) > 64:
                    raise 数学境界違反("総次数上限")
                out[powers] = _係数(out.get(powers, Fraction(0)) + _係数(x*y))
                if len(out) > 512:
                    raise 数学境界違反("中間項数上限")
        return self.整理(out)

    def 冪(self, value: 多項式, exponent: int) -> 多項式:
        if type(exponent) is not int or not 0 <= exponent <= 64:
            raise 数学境界違反("冪指数は0〜64の整数定数")
        out = self.定数(Fraction(1))
        for _ in range(exponent):
            out = self.乗算(out, value)
        return out

    def 読む(self, 情報源: str | dict) -> 多項式:
        if type(情報源) is dict:
            return self.復元(情報源)
        if type(情報源) is not str or not 情報源.strip() or len(情報源.encode()) > 8192:
            raise 数学境界違反("式の型・サイズ不正")
        # コメント、改行、暗黙の乗算などを部分的に読み飛ばさない。
        if any(c in 情報源 for c in "#\n\r;\\"):
            raise 数学境界違反("単一の数式のみを指定する")
        text = 情報源.strip()
        tree = ast.parse(text, mode="eval", feature_version=(3, 11))
        allowed = {ast.Expression, ast.BinOp, ast.UnaryOp, ast.Name, ast.Load, ast.Constant,
                   ast.Add, ast.Sub, ast.Mult, ast.Div, ast.Pow, ast.UAdd, ast.USub}
        pending = [(tree, 0)]
        count = 0
        while pending:
            node, depth = pending.pop()
            count += 1
            if count > 512 or depth > 32 or type(node) not in allowed:
                raise 数学境界違反("未対応構文または構文規模上限")
            if type(node) is ast.Name:
                if node.id not in self.変数 or ast.get_source_segment(text, node) != node.id:
                    raise 数学境界違反("未宣言または正規化されていない変数")
            if type(node) is ast.Constant:
                if type(node.value) not in (int, float):
                    raise 数学境界違反("数値以外の定数")
                _有理(ast.get_source_segment(text, node))  # float値ではなく原文を読む。
            if type(node) is ast.BinOp and type(node.op) is ast.Div:
                if any(type(n) is ast.Name for n in ast.walk(node.right)):
                    raise 数学境界違反("変数を含む分母は未対応。定義域を消して約分しない")
            if type(node) is ast.BinOp and type(node.op) is ast.Pow:
                if type(node.right) is not ast.Constant or type(node.right.value) is not int or not 0 <= node.right.value <= 64:
                    raise 数学境界違反("冪指数は0〜64の整数定数")
            pending.extend((child, depth+1) for child in ast.iter_child_nodes(node))

        def visit(node):
            self.予算.進める()
            if type(node) is ast.Constant:
                return self.定数(_有理(ast.get_source_segment(text, node)))
            if type(node) is ast.Name:
                p = list(self.零次数); p[self.変数.index(node.id)] = 1
                return {tuple(p): Fraction(1)}
            if type(node) is ast.UnaryOp:
                x = visit(node.operand)
                return x if type(node.op) is ast.UAdd else {p: -c for p, c in x.items()}
            if type(node) is ast.BinOp:
                left, right = visit(node.left), visit(node.right)
                if type(node.op) is ast.Add:
                    return self.加算(left, right)
                if type(node.op) is ast.Sub:
                    return self.加算(left, right, -1)
                if type(node.op) is ast.Mult:
                    return self.乗算(left, right)
                if type(node.op) is ast.Pow:
                    return self.冪(left, node.right.value)
                divisor = right.get(self.零次数, Fraction(0))
                if not divisor:
                    raise 数学境界違反("ゼロによる除算")
                return self.整理({p: _係数(c/divisor) for p, c in left.items()})
            raise 数学境界違反("式の境界不正")
        return self.整理(visit(tree.body))

    def 保存(self, poly: 多項式) -> dict:
        return {"変数": list(self.変数), "項": [
            {"次数": list(p), "係数": str(c)} for p, c in sorted(poly.items(), reverse=True)]}

    def 復元(self, raw: dict) -> 多項式:
        if (set(raw) != {"変数", "項"} or type(raw["変数"]) is not list
                or tuple(raw["変数"]) != self.変数 or type(raw["項"]) is not list or len(raw["項"]) > 256):
            raise 数学境界違反('多項式資料の項目・宣言変数不一致')
        out = {}
        for row in raw["項"]:
            if type(row) is not dict or set(row) != {"次数", "係数"} or type(row["次数"]) is not list:
                raise 数学境界違反('項資料不正')
            powers = tuple(row["次数"])
            if len(powers) != len(self.変数) or any(type(x) is not int or x < 0 for x in powers) or powers in out:
                raise 数学境界違反("次数不正または重複項")
            coefficient = _有理(row["係数"])
            if not coefficient or str(coefficient) != row["係数"]:
                raise 数学境界違反('係数資料は非零の正規表記')
            out[powers] = coefficient
        out = self.整理(out)
        if self.保存(out) != raw:
            raise 数学境界違反('多項式資料の非正規順序')
        return out

    def 表示(self, poly: 多項式) -> str:
        parts = []
        for powers, c in sorted(poly.items(), reverse=True):
            factors = [name if n == 1 else f"{name}**{n}" for name, n in zip(self.変数, powers) if n]
            magnitude = abs(c)
            term = "*".join(([str(magnitude)] if magnitude != 1 or not factors else []) + factors)
            parts.append(("-" if c < 0 else "", term))
        if not parts:
            return "0"
        first = parts[0][0] + parts[0][1]
        return first + "".join((" - " if sign else " + ") + term for sign, term in parts[1:])

    def 微積分(self, poly: 多項式, name: str, integrate=False) -> 多項式:
        if name not in self.変数:
            raise 数学境界違反("微積分対象の変数が未宣言")
        i = self.変数.index(name)
        out = {}
        for powers, c in poly.items():
            self.予算.進める()
            exponent = powers[i]
            if not integrate and not exponent:
                continue
            new = list(powers); new[i] += 1 if integrate else -1
            out[tuple(new)] = _係数(c / (exponent+1) if integrate else c * exponent)
        return self.整理(out)

    def 代入(self, poly: 多項式, values: dict) -> 多項式:
        if type(values) is not dict or not set(values) <= set(self.変数):
            raise 数学境界違反("未宣言の代入先")
        parsed = {self.変数.index(k): _有理(v) for k, v in values.items()}
        out = {}
        for powers, c in poly.items():
            new = list(powers)
            for i, value in parsed.items():
                for _ in range(powers[i]):
                    self.予算.進める()
                    c = _係数(c * value)
                new[i] = 0
            p = tuple(new)
            out[p] = _係数(out.get(p, Fraction(0)) + c)
        return self.整理(out)


def _署名結果(text: str, 資料: dict, 予算: _予算) -> 能力結果:
    予算.停止確認()
    資料 = {"版": 数学能力版, **資料, "演算数": 予算.回数,
            "最大演算数": 予算.上限, "意味": "宣言された形式数式の処理。世界の事実認定ではない"}
    raw = json.dumps({"本文": text, "データ": 資料}, ensure_ascii=False, sort_keys=True).encode()
    if len(raw) > 1_000_000:
        raise 数学境界違反("数学記録サイズ上限")
    資料["記録SHA256"] = sha256(raw).hexdigest()
    return 能力結果(True, text, データ=資料)


def _失敗(exc: Exception) -> 能力結果:
    return 能力結果(False, "", 保留理由="数学処理不成立:" + type(exc).__name__,
                     データ={"診断": str(exc) if isinstance(exc, 数学境界違反) else type(exc).__name__})


def 記号を処理(式: str | dict, 変数: tuple[str, ...] = (), *, 操作: str = "正規化",
               対象変数: str | None = None, 代入値: dict | None = None,
               比較式: str | dict | None = None, 最大演算数: int = 100000,
               停止要求: Callable[[], bool] | None = None) -> 能力結果:
    try:
        予算 = _予算(最大演算数, 停止要求)
        engine = _多項式処理(変数, 予算)
        required = {"正規化": (False, False, False), "微分": (True, False, False),
                    "積分": (True, False, False), "代入": (False, True, False),
                    "同値比較": (False, False, True)}
        if 操作 not in required or required[操作] != (対象変数 is not None, 代入値 is not None, 比較式 is not None):
            raise 数学境界違反("操作と設定の組合せ不正")
        poly = engine.読む(式)
        out = poly
        extra = {}
        if 操作 in ("微分", "積分"):
            if type(対象変数) is not str:
                raise 数学境界違反("対象変数は文字列")
            out = engine.微積分(poly, 対象変数, 操作 == "積分")
            if 操作 == "積分":
                extra["積分の範囲"] = "原始関数の一つ。対象変数に依存しない任意関数の加算は未出力"
        elif 操作 == "代入":
            out = engine.代入(poly, 代入値)
        elif 操作 == "同値比較":
            out = engine.加算(poly, engine.読む(比較式), -1)
            extra["判定"] = "恒等" if not out else "非恒等"
            extra["比較の範囲"] = "全実数の変数代入に関する多項式恒等性。非恒等は全点不一致を意味しない"
        text = engine.表示(out)
        if 操作 == "同値比較":
            text = extra["判定"] + "。差の多項式: " + text
        elif 操作 == "積分":
            text = "原始関数の一つ: " + text
        constant = all(p == engine.零次数 for p in out)
        return _署名結果(text, {"処理": "記号演算", "入力": {
            "式": deepcopy(式), "変数": list(変数), "操作": 操作, "対象変数": 対象変数,
            "代入値": deepcopy(代入値), "比較式": deepcopy(比較式)},
            "多項式": engine.保存(out), "定数値": str(out.get(engine.零次数, Fraction(0))) if constant else None,
            **extra}, 予算)
    except Exception as exc:
        return _失敗(exc)


def 数学記録整合(結果: 能力結果) -> bool:
    """宣言入力から再計算する。来歴資料の真正性は検査しない。"""
    try:
        if not isinstance(結果, 能力結果) or 結果.成立 is not True or 結果.保留理由 or 結果.根拠:
            return False
        資料 = 結果.データ
        if 資料["版"] != 数学能力版:
            return False
        values = deepcopy(資料["入力"])
        values["変数"] = tuple(values["変数"])
        if 資料["処理"] == "記号演算":
            expected = 記号を処理(**values, 最大演算数=資料["最大演算数"])
        elif 資料["処理"] == "線形方程式":
            from .線形方程式 import 線形を解く
            values["方程式"] = tuple(values["方程式"])
            expected = 線形を解く(**values, 最大演算数=資料["最大演算数"])
        else:
            return False
        return expected.成立 and expected.本文 == 結果.本文 and expected.データ == 資料
    except Exception:
        return False

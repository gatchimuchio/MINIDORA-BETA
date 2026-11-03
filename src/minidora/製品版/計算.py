from __future__ import annotations
import ast
import operator as op
import re
from .型 import 能力結果

計算版 = "safe-arithmetic-v2"
_OPS = {ast.Add:op.add, ast.Sub:op.sub, ast.Mult:op.mul, ast.Div:op.truediv, ast.FloorDiv:op.floordiv, ast.Mod:op.mod, ast.Pow:op.pow, ast.USub:op.neg, ast.UAdd:op.pos}
_式表層 = r"[-+*/%().\d\s]+"

def _評価(node):
    if isinstance(node, ast.Expression): return _評価(node.body)
    if isinstance(node, ast.Constant) and isinstance(node.value, (int,float)): return node.value
    if isinstance(node, ast.BinOp) and type(node.op) in _OPS:
        a, b = _評価(node.left), _評価(node.right)
        if isinstance(node.op, ast.Pow) and abs(b) > 12: raise ValueError("指数が大きすぎる")
        return _OPS[type(node.op)](a,b)
    if isinstance(node, ast.UnaryOp) and type(node.op) in _OPS: return _OPS[type(node.op)](_評価(node.operand))
    raise ValueError("許可されていない式")

def _正規化(text: str) -> str:
    return text.replace("×","*").replace("÷","/").replace("^","**").replace("％","%").replace("，",",").strip()

def _extract(text: str) -> str:
    """要求全体が単一算術式へ閉じる場合だけ返す。部分一致した先頭式を成功扱いしない。"""
    s = _正規化(text)
    if re.fullmatch(_式表層, s):
        return s
    patterns = (
        r"(.+?)(?:を)?計算(?:してください|して下さい|して|しろ|せよ)\s*[。.!！?？]*$",
        r"(.+?)(?:は)?いくつ(?:ですか)?\s*[。.!！?？]*$",
    )
    for pattern in patterns:
        m = re.fullmatch(pattern, s, re.S)
        if m:
            expr = m.group(1).strip()
            if re.fullmatch(_式表層, expr):
                return expr
    return ""

class 計算モジュール:
    版 = 計算版
    def 解釈(self, text: str) -> str:
        return _extract(text)
    def 実行(self, text: str) -> 能力結果:
        expr = self.解釈(text)
        if not expr:
            return 能力結果(False, "", 保留理由="要求全体を単一計算式として解釈できない")
        if len(expr) > 120:
            return 能力結果(False, "", 保留理由="式が長すぎる")
        try:
            value = _評価(ast.parse(expr, mode="eval"))
        except Exception as exc:
            return 能力結果(False, "", 保留理由=f"計算不能:{exc}")
        return 能力結果(True, f"{expr} = {value}", 根拠=("決定論的算術実行",), データ={"式":expr,"値":value})

from __future__ import annotations

import ast
import re
import unicodedata
from dataclasses import dataclass, field
from typing import Any

from .命令 import 作用, 命令, 手順


@dataclass(frozen=True, slots=True)
class 言語計画:
    手順: 手順
    初期状態: dict[str, Any] = field(default_factory=dict)
    参照必須: bool = False
    種別: str = "一般"


class 自然言語器:
    'Legacy互換の決定論的自然言語境界。HDS 構文化器未注入時のみ実行系から利用する。'

    _二項作用 = {
        ast.Add: 作用.加算,
        ast.Sub: 作用.減算,
        ast.Mult: 作用.乗算,
        ast.Div: 作用.除算,
    }
    _比較作用 = {
        ast.Eq: "同値",
        ast.NotEq: "不同",
        ast.Gt: "大",
        ast.Lt: "小",
        ast.GtE: "以上",
        ast.LtE: "以下",
    }

    def 計画(self, 問合せ: str, *, 文脈参照: Any = None) -> 言語計画:
        normalized = unicodedata.normalize("NFKC", 問合せ).strip()
        if not normalized:
            return self._参照計画("空入力")

        contextual = self._文脈計画(normalized, 文脈参照)
        if contextual is not None:
            return contextual

        phrase = self._日本語算術(normalized)
        if phrase is not None:
            return phrase

        expression = self._式候補(normalized)
        if expression is not None:
            compiled = self._式計画(expression)
            if compiled is not None:
                return compiled

        count = re.search(
            r"[「『\"](.+?)[」』\"](?:の)?文字数(?:を)?(?:数えて|教えて|は)",
            normalized,
        )
        if count:
            initial = {"入力0": count.group(1)}
            proc = 手順(
                "文字数計数",
                (
                    命令(
                        "文字数",
                        作用.計数,
                        引数=("$入力0",),
                        更新先="結果",
                        根拠=("自然言語入力",),
                    ),
                ),
                由来="自然言語器",
            )
            return 言語計画(proc, initial, False, "計数")

        return self._参照計画("外部参照")

    def _文脈計画(self, text: str, 文脈参照: Any) -> 言語計画 | None:
        '局所解釈で確定した参照先を資料として束縛し、Pは状態住所だけを参照する。'
        if 文脈参照 is None:
            return None

        ref = r"(?:それ|これ|あれ|その結果|この結果|あの結果)"
        number = r"-?\d+(?:\.\d+)?"
        contextual_number = self._文脈数値(文脈参照)

        if contextual_number is not None:
            patterns = (
                (
                    rf"{ref}\s*(?:と|に)\s*(?P<右辺>{number})\s*を?\s*(?:足して|加えて|足す)",
                    作用.加算,
                    "文脈加算",
                ),
                (
                    rf"{ref}\s*から\s*(?P<右辺>{number})\s*を?\s*(?:引いて|引く)",
                    作用.減算,
                    "文脈減算",
                ),
                (
                    rf"{ref}\s*(?:に|を)\s*(?P<右辺>{number})\s*を?\s*(?:掛けて|かけて|掛ける|かける|乗じて)",
                    作用.乗算,
                    "文脈乗算",
                ),
                (
                    rf"{ref}\s*を\s*(?P<右辺>{number})\s*で\s*(?:割って|割る|除して)",
                    作用.除算,
                    "文脈除算",
                ),
            )
            for pattern, op, name in patterns:
                match = re.search(pattern, text)
                if not match:
                    continue
                initial = {
                    "文脈0": contextual_number,
                    "入力0": self._数値(match.group("右辺")),
                }
                proc = 手順(
                    name,
                    (
                        命令(
                            name,
                            op,
                            引数=("$文脈0", "$入力0"),
                            更新先="結果",
                            根拠=("局所解釈起点", "自然言語入力"),
                        ),
                    ),
                    由来="自然言語器:局所解釈",
                )
                return 言語計画(proc, initial, False, "算術")

        count = re.search(
            rf"{ref}(?:の)?文字数(?:を)?(?:数えて|教えて|は)",
            text,
        )
        if count:
            proc = 手順(
                "文脈文字数計数",
                (
                    命令(
                        "文脈文字数",
                        作用.計数,
                        引数=("$文脈0",),
                        更新先="結果",
                        根拠=("局所解釈起点", "自然言語入力"),
                    ),
                ),
                由来="自然言語器:局所解釈",
            )
            return 言語計画(proc, {"文脈0": 文脈参照}, False, "計数")

        if re.fullmatch(
            rf"{ref}(?:を)?(?:教えて|表示して|見せて|は)?[?？。!！]*",
            text,
        ):
            proc = 手順(
                "文脈参照",
                (
                    命令(
                        "文脈参照取得",
                        作用.取得,
                        対象="文脈0",
                        更新先="結果",
                        根拠=("局所解釈起点", "自然言語入力"),
                    ),
                ),
                由来="自然言語器:局所解釈",
            )
            return 言語計画(proc, {"文脈0": 文脈参照}, False, "文脈参照")
        return None

    def _文脈数値(self, value: Any) -> int | float | None:
        if isinstance(value, bool):
            return None
        if isinstance(value, (int, float)):
            return value
        if isinstance(value, str) and re.fullmatch(r"-?\d+(?:\.\d+)?", value.strip()):
            return self._数値(value.strip())
        return None

    def _参照計画(self, reason: str) -> 言語計画:
        proc = 手順(
            "参照回答",
            (
                命令(
                    "参照列取得",
                    作用.取得,
                    対象="参照",
                    更新先="参照列",
                    根拠=(reason,),
                ),
                命令(
                    "先頭参照抽出",
                    作用.抽出,
                    引数=("$参照列", 0),
                    更新先="参照候補",
                    根拠=(reason,),
                ),
                命令(
                    "参照内容抽出",
                    作用.抽出,
                    引数=("$参照候補", "内容"),
                    更新先="結果",
                    根拠=(reason,),
                ),
            ),
            由来="自然言語器",
        )
        return 言語計画(proc, {}, True, "参照")

    def _日本語算術(self, text: str) -> 言語計画 | None:
        number = r"(-?\d+(?:\.\d+)?)"
        patterns = (
            (
                rf"{number}\s*(?:と|に)\s*{number}\s*を?\s*(?:足して|加えて|足す)",
                作用.加算,
                "加算",
            ),
            (
                rf"{number}\s*から\s*{number}\s*を?\s*(?:引いて|引く)",
                作用.減算,
                "減算",
            ),
            (
                rf"{number}\s*(?:と|に)\s*{number}\s*を?\s*(?:掛けて|かけて|掛ける|かける|乗じて)",
                作用.乗算,
                "乗算",
            ),
            (
                rf"{number}\s*を\s*{number}\s*で\s*(?:割って|割る|除して)",
                作用.除算,
                "除算",
            ),
        )
        for pattern, op, name in patterns:
            match = re.search(pattern, text)
            if not match:
                continue
            values = tuple(self._数値(x) for x in match.groups())
            initial = {f"入力{i}": value for i, value in enumerate(values)}
            proc = 手順(
                name,
                (
                    命令(
                        name,
                        op,
                        引数=("$入力0", "$入力1"),
                        更新先="結果",
                        根拠=("自然言語入力",),
                    ),
                ),
                由来="自然言語器",
            )
            return 言語計画(proc, initial, False, "算術")
        return None

    def _式候補(self, text: str) -> str | None:
        text = text.replace("×", "*").replace("÷", "/")
        trimmed = re.sub(r"(?:は)?[?？。!！]+$", "", text).strip()
        trimmed = re.sub(
            r"(?:を)?(?:計算して|計算|求めて|求める)$",
            "",
            trimmed,
        ).strip()
        if re.fullmatch(r"[\d\s.+\-*/()<>!=]+", trimmed) and any(
            op in trimmed for op in "+-*/<>=!"
        ):
            return trimmed

        candidates = re.findall(
            r"[-+]?\d+(?:\.\d+)?(?:\s*[+\-*/<>=!]\s*[-+]?\d+(?:\.\d+)?)+",
            text,
        )
        return max(candidates, key=len).strip() if candidates else None

    def _式計画(self, expression: str) -> 言語計画 | None:
        try:
            tree = ast.parse(expression, mode="eval")
        except SyntaxError:
            return None

        commands: list[命令] = []
        initial: dict[str, Any] = {}
        counter = {'資料': 0, "tmp": 0}

        def 資料_ref(value: int | float) -> str:
            key = f"入力{counter['資料']}"
            counter['資料'] += 1
            initial[key] = value
            return f"${key}"

        def compile_node(node: ast.AST, target: str | None = None) -> str | None:
            if (
                isinstance(node, ast.Constant)
                and isinstance(node.value, (int, float))
                and not isinstance(node.value, bool)
            ):
                return 資料_ref(node.value)
            if (
                isinstance(node, ast.UnaryOp)
                and isinstance(node.op, (ast.USub, ast.UAdd))
                and isinstance(node.operand, ast.Constant)
                and isinstance(node.operand.value, (int, float))
            ):
                value = -node.operand.value if isinstance(node.op, ast.USub) else node.operand.value
                return 資料_ref(value)
            if isinstance(node, ast.BinOp) and type(node.op) in self._二項作用:
                left = compile_node(node.left)
                right = compile_node(node.right)
                if left is None or right is None:
                    return None
                out = target or f"中間{counter['tmp']}"
                counter["tmp"] += 1
                commands.append(
                    命令(
                        f"式{len(commands) + 1}",
                        self._二項作用[type(node.op)],
                        引数=(left, right),
                        更新先=out,
                        根拠=("自然言語入力",),
                    )
                )
                return f"${out}"
            if (
                isinstance(node, ast.Compare)
                and len(node.ops) == 1
                and len(node.comparators) == 1
                and type(node.ops[0]) in self._比較作用
            ):
                left = compile_node(node.left)
                right = compile_node(node.comparators[0])
                if left is None or right is None:
                    return None
                out = target or f"中間{counter['tmp']}"
                counter["tmp"] += 1
                commands.append(
                    命令(
                        f"比較{len(commands) + 1}",
                        作用.比較,
                        引数=(left, self._比較作用[type(node.ops[0])], right),
                        更新先=out,
                        根拠=("自然言語入力",),
                    )
                )
                return f"${out}"
            return None

        root_ref = compile_node(tree.body, "結果")
        if root_ref is None:
            return None
        if not commands:
            commands.append(
                命令(
                    "結果取得",
                    作用.取得,
                    対象=root_ref[1:],
                    更新先="結果",
                    根拠=("自然言語入力",),
                )
            )
        return 言語計画(
            手順("数式計算", tuple(commands), 由来="自然言語器"),
            initial,
            False,
            "算術",
        )

    def _数値(self, raw: str) -> int | float:
        value = float(raw)
        return int(value) if value.is_integer() else value

    def 表面化(self, 値: Any, 状態: str, 理由: tuple[str, ...]) -> str:
        """互換入口。意味解析はここに残し、表現実体だけを出力系へ渡す。"""
        from .出力系.互換 import 既存日本語値を表現
        return 既存日本語値を表現(値, 状態, 理由)

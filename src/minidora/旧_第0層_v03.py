from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Mapping

from .命令 import 作用, 命令, 手順


# MINIDORAはLayer-0の意味をこのリポジトリ内で独自再定義しない。
# 下記外部リポジトリを論理上位契約とし、参照commitは現行実装の再現用pinである。
LAYER0正本リポジトリ = "https://github.com/gatchimuchio/LLM-Layer-0-Functional-Compliance-Specification"
LAYER0参照コミット = "4adf86d13d7beb99fe5eaa9e240b22996ba3d3bc"
LAYER0仕様版 = "v4.0-provisional"
LAYER0機能責任 = (
    "LINGUISTIC_ADDRESSABILITY",
    '文脈_BOUND_状態',
    'TRANSFORMATION_OR_COMPOSITION_模型核',
    '文脈_DEPENDENT_結果_FORMATION',
    '結果_SURFACE',
)


@dataclass(slots=True)
class 実行文脈:
    状態: dict[str, Any] = field(default_factory=dict)
    履歴: list[dict[str, Any]] = field(default_factory=list)
    停止済み: bool = False


class Layer0:
    """Layer-0 v4上位契約をMINIDORAへ実装する最小命令実行機構。

    機能責任の意味は ``LAYER0正本リポジトリ`` を正本とする。
    このクラスはMINIDORA固有の実装であり、特定モデルの物理構造を
    Layer-0責任そのものへ昇格させない。
    """

    def 実行(self, 手順_: 手順, 初期状態: dict[str, Any] | None = None) -> 実行文脈:
        文脈 = 実行文脈(dict(初期状態 or {}))
        for 命令_ in 手順_.命令列:
            if 文脈.停止済み:
                break
            self._適用(文脈, 命令_)
        return 文脈

    def _値(self, 文脈: 実行文脈, value: Any) -> Any:
        if isinstance(value, str) and value.startswith("$"):
            return 文脈.状態.get(value[1:])
        return value

    def _適用(self, 文脈: 実行文脈, 命令_: 命令) -> None:
        args = tuple(self._値(文脈, value) for value in 命令_.引数)
        op = 命令_.作用
        結果: Any = None

        if op == 作用.設定:
            if 命令_.更新先 is None or not args:
                raise ValueError("設定には更新先と値が必要")
            結果 = args[0]
            文脈.状態[命令_.更新先] = 結果
        elif op == 作用.取得:
            if 命令_.対象 is None:
                raise ValueError("取得には対象が必要")
            結果 = 文脈.状態.get(命令_.対象)
            if 命令_.更新先:
                文脈.状態[命令_.更新先] = 結果
        elif op == 作用.抽出:
            if len(args) != 2:
                raise ValueError("抽出は 対象, キー/位置 を取る")
            情報源, key = args
            if isinstance(情報源, Mapping):
                結果 = 情報源.get(key)
            elif isinstance(key, int) and isinstance(情報源, (tuple, list, str)):
                結果 = 情報源[key] if -len(情報源) <= key < len(情報源) else None
            elif isinstance(key, str) and hasattr(情報源, key):
                結果 = getattr(情報源, key)
            else:
                try:
                    結果 = 情報源[key]
                except (KeyError, IndexError, TypeError):
                    結果 = None
            if 命令_.更新先:
                文脈.状態[命令_.更新先] = 結果
        elif op in {作用.加算, 作用.減算, 作用.乗算, 作用.除算}:
            if len(args) < 2:
                raise ValueError(f"{op}には2値以上が必要")
            結果 = args[0]
            for value in args[1:]:
                if op == 作用.加算:
                    結果 += value
                elif op == 作用.減算:
                    結果 -= value
                elif op == 作用.乗算:
                    結果 *= value
                else:
                    if value == 0:
                        raise ValueError("0では除算できない")
                    結果 /= value
            if 命令_.更新先:
                文脈.状態[命令_.更新先] = 結果
        elif op == 作用.比較:
            if len(args) != 3:
                raise ValueError("比較は 左, 演算子, 右 を取る")
            左, 演算子, 右 = args
            比較表 = {
                "同値": 左 == 右,
                "不同": 左 != 右,
                "大": 左 > 右,
                "小": 左 < 右,
                "以上": 左 >= 右,
                "以下": 左 <= 右,
            }
            if 演算子 not in 比較表:
                raise ValueError(f"未対応比較: {演算子}")
            結果 = 比較表[演算子]
            if 命令_.更新先:
                文脈.状態[命令_.更新先] = 結果
        elif op == 作用.計数:
            if len(args) != 1:
                raise ValueError("計数には一対象が必要")
            結果 = len(args[0])
            if 命令_.更新先:
                文脈.状態[命令_.更新先] = 結果
        elif op == 作用.結合:
            結果 = tuple(args)
            if 命令_.更新先:
                文脈.状態[命令_.更新先] = 結果
        elif op == 作用.交換:
            if len(args) != 2 or not all(isinstance(x, str) for x in args):
                raise ValueError("交換は状態キー2個を取る")
            a, b = args
            文脈.状態[a], 文脈.状態[b] = 文脈.状態.get(b), 文脈.状態.get(a)
            結果 = (文脈.状態[a], 文脈.状態[b])
        elif op == 作用.反転:
            if len(args) != 1:
                raise ValueError("反転には一値が必要")
            結果 = not bool(args[0])
            if 命令_.更新先:
                文脈.状態[命令_.更新先] = 結果
        elif op == 作用.停止:
            文脈.停止済み = True
            結果 = True
        else:
            raise ValueError(f"未対応作用: {op}")

        文脈.履歴.append({
            "名称": 命令_.名称,
            "作用": op.value,
            "対象": 命令_.対象,
            "引数": args,
            "結果": 結果,
            "根拠": 命令_.根拠,
        })

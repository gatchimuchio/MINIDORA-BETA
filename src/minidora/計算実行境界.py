from __future__ import annotations

from collections.abc import Mapping
from typing import Any

from .計算中間表現 import (
    計算中間表現,
    計算作用,
    計算値,
    計算値種別,
    計算履歴,
    計算実行結果,
)


計算実行境界版 = "計算実行境界-v1"


class 計算実行境界:
    """計算中間表現だけを消費する決定論的実行境界。

    自然言語、HDS、模型関係、外部参照を解釈しない。入力済みの計算作用と
    型付き値参照だけを実行する。
    """

    版 = 計算実行境界版

    @staticmethod
    def _値(値_: 計算値, 状態: Mapping[str, Any]) -> Any:
        if 値_.種別 == 計算値種別.即値:
            return 値_.内容
        if 値_.種別 == 計算値種別.状態値:
            return 状態.get(str(値_.内容))
        if 値_.種別 == 計算値種別.状態住所:
            return str(値_.内容)
        raise ValueError(f"未対応計算値種別: {値_.種別}")

    @staticmethod
    def _通常値限定(命令入力: tuple[計算値, ...]) -> None:
        if any(item.種別 == 計算値種別.状態住所 for item in 命令入力):
            raise ValueError("状態住所は交換以外の通常値入力へ使用できない")

    def 検証(self, 中間表現: 計算中間表現) -> None:
        for 命令_ in 中間表現.命令列:
            op = 命令_.作用
            count = len(命令_.入力)

            if op == 計算作用.設定:
                self._通常値限定(命令_.入力)
                if count < 1 or 命令_.出力住所 is None:
                    raise ValueError("設定には1値以上と出力住所が必要")
            elif op == 計算作用.取得:
                if 命令_.対象住所 is None:
                    raise ValueError("取得には対象住所が必要")
                if count:
                    raise ValueError("取得は値入力を取らない")
            elif op == 計算作用.抽出:
                self._通常値限定(命令_.入力)
                if count != 2:
                    raise ValueError("抽出は対象とキー/位置の2入力を取る")
            elif op in {計算作用.加算, 計算作用.減算, 計算作用.乗算, 計算作用.除算}:
                self._通常値限定(命令_.入力)
                if count < 2:
                    raise ValueError(f"{op.value}には2値以上が必要")
            elif op == 計算作用.比較:
                self._通常値限定(命令_.入力)
                if count != 3:
                    raise ValueError("比較は左・演算子・右の3入力を取る")
            elif op == 計算作用.計数:
                self._通常値限定(命令_.入力)
                if count != 1:
                    raise ValueError("計数には1入力が必要")
            elif op == 計算作用.結合:
                self._通常値限定(命令_.入力)
            elif op == 計算作用.交換:
                if count != 2 or any(item.種別 != 計算値種別.状態住所 for item in 命令_.入力):
                    raise ValueError("交換は状態住所2個を取る")
            elif op == 計算作用.反転:
                self._通常値限定(命令_.入力)
                if count != 1:
                    raise ValueError("反転には1入力が必要")
            elif op == 計算作用.停止:
                if count:
                    raise ValueError("停止は値入力を取らない")
            else:
                raise ValueError(f"未対応計算作用: {op}")

    @staticmethod
    def _比較(left: Any, operator: Any, right: Any) -> bool:
        """要求された比較だけを評価し、不要な演算を先行実行しない。"""
        if operator == "同値":
            return left == right
        if operator == "不同":
            return left != right
        if operator == "大":
            return left > right
        if operator == "小":
            return left < right
        if operator == "以上":
            return left >= right
        if operator == "以下":
            return left <= right
        raise ValueError(f"未対応比較: {operator}")

    def 実行(
        self,
        中間表現: 計算中間表現,
        初期状態: Mapping[str, Any] | None = None,
    ) -> 計算実行結果:
        self.検証(中間表現)
        状態: dict[str, Any] = dict(初期状態 or {})
        履歴: list[計算履歴] = []
        停止済み = False

        for 命令_ in 中間表現.命令列:
            if 停止済み:
                break

            op = 命令_.作用
            inputs = tuple(self._値(item, 状態) for item in 命令_.入力)
            結果: Any = None

            if op == 計算作用.設定:
                結果 = inputs[0]
                assert 命令_.出力住所 is not None
                状態[命令_.出力住所] = 結果
            elif op == 計算作用.取得:
                assert 命令_.対象住所 is not None
                結果 = 状態.get(命令_.対象住所)
                if 命令_.出力住所 is not None:
                    状態[命令_.出力住所] = 結果
            elif op == 計算作用.抽出:
                情報源, key = inputs
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
                if 命令_.出力住所 is not None:
                    状態[命令_.出力住所] = 結果
            elif op in {計算作用.加算, 計算作用.減算, 計算作用.乗算, 計算作用.除算}:
                結果 = inputs[0]
                for value in inputs[1:]:
                    # in-place演算はlist等の入力状態を破壊し得るため使わない。
                    if op == 計算作用.加算:
                        結果 = 結果 + value
                    elif op == 計算作用.減算:
                        結果 = 結果 - value
                    elif op == 計算作用.乗算:
                        結果 = 結果 * value
                    else:
                        if value == 0:
                            raise ValueError("0では除算できない")
                        結果 = 結果 / value
                if 命令_.出力住所 is not None:
                    状態[命令_.出力住所] = 結果
            elif op == 計算作用.比較:
                left, operator, right = inputs
                結果 = self._比較(left, operator, right)
                if 命令_.出力住所 is not None:
                    状態[命令_.出力住所] = 結果
            elif op == 計算作用.計数:
                結果 = len(inputs[0])
                if 命令_.出力住所 is not None:
                    状態[命令_.出力住所] = 結果
            elif op == 計算作用.結合:
                結果 = tuple(inputs)
                if 命令_.出力住所 is not None:
                    状態[命令_.出力住所] = 結果
            elif op == 計算作用.交換:
                left_address, right_address = (str(item.内容) for item in 命令_.入力)
                状態[left_address], 状態[right_address] = 状態.get(right_address), 状態.get(left_address)
                結果 = (状態[left_address], 状態[right_address])
                if 命令_.出力住所 is not None:
                    状態[命令_.出力住所] = 結果
            elif op == 計算作用.反転:
                結果 = not bool(inputs[0])
                if 命令_.出力住所 is not None:
                    状態[命令_.出力住所] = 結果
            elif op == 計算作用.停止:
                停止済み = True
                結果 = True
            else:
                raise ValueError(f"未対応計算作用: {op}")

            履歴.append(
                計算履歴(
                    命令_.命令ID,
                    命令_.名称,
                    op,
                    命令_.対象住所,
                    inputs,
                    結果,
                    命令_.出力住所,
                    命令_.根拠,
                )
            )

        return 計算実行結果(
            状態,
            tuple(履歴),
            停止済み,
            状態.get(中間表現.出力住所),
            中間表現.版,
        )


標準計算実行境界 = 計算実行境界


__all__ = ["計算実行境界版", "計算実行境界", "標準計算実行境界"]

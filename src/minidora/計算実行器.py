from __future__ import annotations

from typing import Any

from .命令 import 手順
from .命令計算降下 import 命令計算降下
from .旧_第0層_v03 import Layer0 as _旧Layer0, 実行文脈
from .計算中間表現 import 計算中間表現, 計算実行結果
from .計算実行境界 import 計算実行境界 as 計算実行境界型


class 計算実行器(_旧Layer0):
    """計算中間表現をABI v1で実行する汎用計算器。

    v0.3までLayer0と呼ばれていた公開APIは互換入口として維持する。ただし
    ``手順`` を直接解釈して実行せず、必ず ``計算中間表現`` へ降下してから
    ``計算実行境界`` を通す。

    大規模言語模型の模型中核ではない。言語模型性は ``模型.MINIDORA模型核`` が担う。
    """

    def __init__(self, 計算実行境界_: 計算実行境界型 | None = None) -> None:
        self.計算実行境界 = 計算実行境界_ or 計算実行境界型()

    def 計算化(self, 手順_: 手順) -> 計算中間表現:
        return 命令計算降下(手順_)

    def 計算実行(
        self,
        中間表現: 計算中間表現,
        初期状態: dict[str, Any] | None = None,
    ) -> 計算実行結果:
        return self.計算実行境界.実行(中間表現, 初期状態)

    def 実行(
        self,
        手順または中間表現: 手順 | 計算中間表現,
        初期状態: dict[str, Any] | None = None,
    ) -> 実行文脈:
        中間表現 = (
            手順または中間表現
            if isinstance(手順または中間表現, 計算中間表現)
            else self.計算化(手順または中間表現)
        )
        result = self.計算実行(中間表現, 初期状態)
        legacy_history = [
            {
                "名称": item.名称,
                "作用": item.作用.value,
                "対象": item.対象住所,
                "引数": item.入力値,
                "結果": item.結果,
                "根拠": item.根拠,
            }
            for item in result.履歴
        ]
        return 実行文脈(dict(result.状態), legacy_history, result.停止済み)


__all__ = ["計算実行器", "実行文脈"]

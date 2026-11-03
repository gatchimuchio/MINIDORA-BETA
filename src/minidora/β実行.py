"""MINIDORAの既存三機構を一度ずつ接続するβ公開入口。

意味決定、駆動、最終採否、表現を再実装しない。外部送達も暗黙実行しない。
"""
from __future__ import annotations

from dataclasses import dataclass
from threading import RLock

from .HDS実行主体 import HDS実行結果
from .HDS駆動コア import HDS駆動コア
from .入力系.契約 import 入力束
from .入力系.統合 import HDS入力系
from .共通契約.出力 import 出力束, 表現契約, 表現結果
from .出力系.統合 import HDS出力系


β実行契約版 = "MINIDORA-ENGINE-BETA-1"


@dataclass(frozen=True, slots=True)
class β処理結果:
    入力: 入力束
    駆動: HDS実行結果
    出力: 出力束
    表現: 表現結果


class ミニドラβ実行機:
    """一所有主体内で順次実行する接続面。各機構の既存責任を変更しない。

    同じ入力版を再駆動しない。再試行する場合は入力版を明示更新する。
    実際の外部送信は出力機の明示的な送達操作へ委ねる。
    """

    def __init__(self, *, 入力機: HDS入力系 | None = None,
                 本体: HDS駆動コア | None = None,
                 出力機: HDS出力系 | None = None) -> None:
        self.入力機 = 入力機 if 入力機 is not None else HDS入力系.標準()
        self.本体 = 本体 if 本体 is not None else HDS駆動コア()
        self.出力機 = 出力機 if 出力機 is not None else HDS出力系()
        if not isinstance(self.入力機, HDS入力系) or not isinstance(self.本体, HDS駆動コア) or not isinstance(self.出力機, HDS出力系):
            raise TypeError("既存の入力機・ミニドラ本体・出力機が必要")
        self._保護 = RLock()
        self._駆動済み: set[tuple[str, str, int]] = set()
        self._結果: dict[tuple[str, str, int], β処理結果] = {}

    def 実行(self, 本文: str, *, 案件ID: str, 入力ID: str,
           要求状態=(), 追加作用=(), 成果名: str | None = None,
           入力版: int = 1, 出力ID: str = "回答", 判断版: int = 1,
           表現: 表現契約 | None = None, **駆動条件) -> β処理結果:
        鍵 = (案件ID, 入力ID, 入力版)
        with self._保護:
            if 鍵 in self._駆動済み:
                raise ValueError("同一入力版を再駆動しない。参照または入力版更新を使用する")
            入力 = self.入力機.受理(本文, 案件ID=案件ID, 入力ID=入力ID, 版=入力版)
            # 失敗した作用を同じ入力版で無断再開しないため、駆動前に登録する。
            self._駆動済み.add(鍵)
            駆動 = self.本体.入力実行(
                入力, 要求状態=tuple(要求状態), 追加作用=tuple(追加作用), **駆動条件
            )
            出力 = self.本体.出力を構成(
                駆動, 案件ID=案件ID, 出力ID=出力ID, 判断版=判断版,
                表現=表現, 成果名=成果名, 入力束=入力,
            )
            表現結果 = self.出力機.表現(出力)
            結果 = β処理結果(入力, 駆動, 出力, 表現結果)
            self._結果[鍵] = 結果
            return 結果

    def 結果を参照(self, *, 案件ID: str, 入力ID: str, 入力版: int = 1) -> β処理結果:
        with self._保護:
            return self._結果[(案件ID, 入力ID, 入力版)]

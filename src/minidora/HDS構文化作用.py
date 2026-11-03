from __future__ import annotations

from hashlib import sha256
import inspect
from typing import Sequence

from .HDS実行主体 import HDS作用機会, HDS作用結果, HDS作用状態, HDS実行状態
from .HDS中間表現 import HDSIR
from .HDSコア入力 import HDSコア入力束
from .HDSコア入力射影 import HDSコア入力へ


class HDS構文化作用:
    """自然言語入力をMINIDORA Coreが消費するHDS入力へ構文化する知覚作用。

    正本成果は `HDSコア入力`。Core-native構文化器ではLegacy IRを形成しない。
    Legacy構文化器だけ互換用HDSIRを併置する。
    構文化器は作用選択・実行計画・最終採否・回答生成を行わない。
    """

    def __init__(
        self,
        構文化器,
        入力: str,
        *,
        前回結果: object = None,
        HDS履歴: tuple[HDSIR, ...] = (),
        文脈=None,
        入力状態: Sequence[str] = (),
        出力状態: str = "HDS意味構文化済み",
        解消対象: Sequence[str] = ("入力未構文化",),
        実行閉包要求: bool = False,
    ) -> None:
        if not any(callable(getattr(構文化器, 名, None)) for 名 in ("コンパイル束", "コア入力コンパイル", "コンパイル")):
            raise TypeError("HDS構文化作用にはCore入力又はLegacy構文化が可能な構文化器が必要")
        self.構文化器 = 構文化器
        self.入力 = str(入力)
        self.前回結果 = 前回結果
        self.HDS履歴 = tuple(HDS履歴)
        self.文脈 = 文脈
        self.入力状態 = frozenset(str(x) for x in 入力状態)
        self.出力状態 = str(出力状態)
        self.解消対象 = frozenset(str(x) for x in 解消対象)
        self.実行閉包要求 = bool(実行閉包要求)
        self.作用ID = "HDS構文化"
        署名材料 = repr((
            self.入力,
            self.前回結果,
            self.HDS履歴,
            self.文脈,
            self.実行閉包要求,
            type(構文化器).__module__,
            type(構文化器).__qualname__,
        )).encode("utf-8")
        self._入力印 = sha256(署名材料).hexdigest()

    def 機会(self, 状態: HDS実行状態) -> HDS作用機会 | None:
        if not self.入力状態.issubset(状態.成立状態):
            return None
        if self.出力状態 in 状態.成立状態:
            return None
        return HDS作用機会(
            self.作用ID,
            self._入力印,
            self.入力状態,
            frozenset({self.出力状態}),
            self.解消対象,
            1,
            1.0,
            True,
            ("HDSコア入力構文化器",),
            作用定義ID="HDS構文化",
            意味入力署名=self._入力印,
        )

    def _追加引数(self, 呼出関数) -> dict:
        引数 = inspect.signature(呼出関数).parameters
        可変引数 = any(項目.kind is inspect.Parameter.VAR_KEYWORD for 項目 in 引数.values())
        追加引数 = {}
        if "前回結果" in 引数 or 可変引数:
            追加引数["前回結果"] = self.前回結果
        if "HDS履歴" in 引数 or 可変引数:
            追加引数["HDS履歴"] = self.HDS履歴
        if "文脈" in 引数 or 可変引数:
            追加引数["文脈"] = self.文脈
        return 追加引数

    def _コンパイル(self) -> tuple[HDSコア入力束, HDSIR | None]:
        # Core-native入口を常に最優先する。Legacy成果の形成を正本入力の前提にしない。
        コア関数 = getattr(self.構文化器, "コア入力コンパイル", None)
        if callable(コア関数):
            コア入力 = コア関数(self.入力, **self._追加引数(コア関数))
            if not isinstance(コア入力, HDSコア入力束):
                raise TypeError("Core入力コンパイルの戻り型不正")
            return コア入力, None

        # Core-native入口を持たない束形式だけ、互換的に意味IRから射影する。
        束関数 = getattr(self.構文化器, "コンパイル束", None)
        if callable(束関数):
            束 = 束関数(self.入力, **self._追加引数(束関数))
            コア入力 = getattr(束, "コア入力", None)
            意味IR = getattr(束, "意味IR", None)
            if コア入力 is None and isinstance(意味IR, HDSIR):
                コア入力 = HDSコア入力へ(意味IR)
            if not isinstance(コア入力, HDSコア入力束):
                raise TypeError("コンパイル束にCore入力正本がない")
            if 意味IR is not None and not isinstance(意味IR, HDSIR):
                raise TypeError("コンパイル束のLegacy意味IR型不正")
            return コア入力, 意味IR

        互換関数 = getattr(self.構文化器, "コンパイル")
        意味IR = 互換関数(self.入力, **self._追加引数(互換関数))
        if not isinstance(意味IR, HDSIR):
            raise TypeError("Legacy構文化器の戻り型不正")
        return HDSコア入力へ(意味IR), 意味IR

    def 実行(self, 状態: HDS実行状態) -> HDS作用結果:
        try:
            コア入力, 意味IR = self._コンパイル()
        except Exception as exc:
            return HDS作用結果(
                HDS作用状態.失敗,
                追加残差=frozenset({f"構文化失敗:{type(exc).__name__}"}),
                理由=("HDSコア入力構文化失敗", type(exc).__name__),
            )

        残差群 = {
            f"HDS残差:{項目.種別}:{項目.理由}"
            for 項目 in コア入力.残差
        }
        if self.実行閉包要求 and 意味IR is not None:
            残差群.update(f"HDS実行阻害:{項目}" for 項目 in 意味IR.実行阻害理由)
        理由群 = ["HDSコア入力を状態へ接続"]
        if 残差群:
            理由群.append("HDSコア入力に残差あり")
        else:
            理由群.append("HDSコア入力契約閉包")

        成果 = [("HDSコア入力", コア入力)]
        if 意味IR is not None:
            成果.append(("HDS_IR", 意味IR))
        return HDS作用結果(
            HDS作用状態.成立,
            追加状態=frozenset({self.出力状態}),
            解消残差=self.解消対象,
            追加残差=frozenset(残差群),
            成果=tuple(成果),
            主体状態差分=(("HDSコア入力署名", コア入力.意味署名),),
            理由=tuple(理由群),
        )


__all__ = ["HDS構文化作用"]

from __future__ import annotations

import inspect
from dataclasses import dataclass
from typing import Any

from .HDS適合器 import HDSコンパイラProtocol, HDS文脈
from .HDS中間表現 import HDSIR
from .採否 import 実行状態, 採否結果


@dataclass(frozen=True, slots=True)
class Trinity記憶監査:
    版: int
    操作: str
    対象: str
    値: Any
    理由: str


class 記憶主体:
    """TrinityのM。確定した結果・焦点・IR・未解残差をturn間で保持する。"""

    def __init__(self) -> None:
        self._版 = 0
        self._現在焦点: Any = None
        self._直前結果: Any = None
        self._直前IR: HDSIR | None = None
        self._未解残差: tuple[tuple[str, str], ...] = ()
        self._IR履歴: list[HDSIR] = []
        self._監査: list[Trinity記憶監査] = []

    @property
    def 版(self) -> int:
        return self._版

    @property
    def IR履歴(self) -> tuple[HDSIR, ...]:
        return tuple(self._IR履歴)

    @property
    def 監査履歴(self) -> tuple[Trinity記憶監査, ...]:
        return tuple(self._監査)

    def 文脈(self) -> HDS文脈:
        refs: list[str] = []
        if self._現在焦点 is not None:
            refs.append('作業:current_focus')
        if self._直前結果 is not None:
            refs.append('作業:last_結果')
        if self._直前IR is not None:
            refs.append('作業:last_ir')
        if self._未解残差:
            refs.append('作業:unresolved')
        return HDS文脈(
            記憶版=self._版,
            現在焦点=self._現在焦点,
            直前結果=self._直前結果,
            直前IR=self._直前IR,
            未解残差=self._未解残差,
            記憶引用=tuple(refs),
        )

    def _記録(self, 操作: str, 対象: str, 値: Any, 理由: str) -> None:
        self._版 += 1
        self._監査.append(Trinity記憶監査(self._版, 操作, 対象, 値, 理由))

    def IRを保持(self, ir: HDSIR) -> None:
        self._直前IR = ir
        self._IR履歴.append(ir)
        self._記録("REVISE", '作業:last_ir', ir, "現在turnのHDS-IRを保持")

    def 結果を確定(self, 値: Any) -> None:
        self._直前結果 = 値
        self._現在焦点 = 値
        self._未解残差 = ()
        self._記録("REVISE", '作業:last_結果', 値, "採用結果を保持")
        self._記録("REVISE", '作業:current_focus', 値, "採用結果を現在焦点へ更新")

    def 未解を保持(self, ir: HDSIR) -> None:
        residuals = tuple((item.種別, item.理由) for item in ir.残差)
        if residuals:
            self._未解残差 = residuals
            self._記録("REVISE", '作業:unresolved', residuals, "SUSPENDした未解意味を保持")


class HDS判断主体:
    'TrinityのJ。Mの文脈を構文化器へ引用し、Cの結果をMへ確定帰還する。'

    def __init__(self, 記憶: 記憶主体 | None = None) -> None:
        self.記憶 = 記憶 or 記憶主体()

    def 文脈(self) -> HDS文脈:
        return self.記憶.文脈()

    def コンパイル(self, 構文化器: HDSコンパイラProtocol, 入力: str) -> HDSIR:
        文脈 = self.文脈()
        compile_fn = 構文化器.コンパイル
        params = inspect.signature(compile_fn).parameters
        kwargs: dict[str, Any] = {
            "前回結果": 文脈.直前結果,
            "HDS履歴": self.記憶.IR履歴,
        }
        if "文脈" in params or any(p.kind is inspect.Parameter.VAR_KEYWORD for p in params.values()):
            kwargs["文脈"] = 文脈
        return compile_fn(入力, **kwargs)

    def 帰還(self, 判定: 採否結果, 値: Any, ir: HDSIR | None) -> None:
        if ir is not None:
            self.記憶.IRを保持(ir)
        if 判定.状態 == 実行状態.合格 and 値 is not None:
            self.記憶.結果を確定(値)
        elif 判定.状態 == 実行状態.保留 and ir is not None:
            self.記憶.未解を保持(ir)


class Trinity文脈系:
    '公開実行系のJ/M循環。計算主体Cは既存Layer-0 実行系が担う。'

    def __init__(self, 判断主体: HDS判断主体 | None = None) -> None:
        self.判断主体 = 判断主体 or HDS判断主体()

    @property
    def 記憶主体(self) -> 記憶主体:
        return self.判断主体.記憶

    def コンパイル(self, 構文化器: HDSコンパイラProtocol, 入力: str) -> HDSIR:
        return self.判断主体.コンパイル(構文化器, 入力)

    def 帰還(self, 判定: 採否結果, 値: Any, ir: HDSIR | None) -> None:
        self.判断主体.帰還(判定, 値, ir)


__all__ = [
    "Trinity記憶監査",
    "記憶主体",
    "HDS判断主体",
    "Trinity文脈系",
]

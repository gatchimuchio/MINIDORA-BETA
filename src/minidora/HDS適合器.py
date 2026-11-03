from __future__ import annotations

import inspect
from dataclasses import dataclass
from typing import Any, Protocol

from .HDS中間表現 import HDSIR
from .HDSコア入力 import HDSコア入力束
from .HDSコア入力射影 import HDSコア入力へ

@dataclass(frozen=True, slots=True)
class HDS文脈:
    '実行系局所作業状態またはTrinity Mから構文化器へ引用する現在文脈。'
    記憶版: int = 0
    現在焦点: Any = None
    直前結果: Any = None
    直前IR: HDSIR | None = None
    未解残差: tuple[tuple[str,str],...] = ()
    記憶引用: tuple[str,...] = ()
    直前入力: str | None = None
    直前採否: str | None = None

class HDSコンパイラProtocol(Protocol):
    def コンパイル(self, 入力: str, *, 前回結果: Any=None, HDS履歴: tuple[HDSIR,...]=(), 文脈: HDS文脈|None=None) -> HDSIR: ...
class HDSカーネルコンパイラProtocol(Protocol):
    def コンパイル束(self, 入力: str, *, 前回結果: Any=None, HDS履歴: tuple[HDSIR,...]=(), 文脈: HDS文脈|None=None): ...
class HDSコア入力コンパイラProtocol(Protocol):
    def コア入力コンパイル(self, 入力: str, *, 前回結果: Any=None, HDS履歴: tuple[HDSIR,...]=(), 文脈: HDS文脈|None=None) -> HDSコア入力束: ...

def _独立呼出(compile_fn, 入力: str):
    params=inspect.signature(compile_fn).parameters
    has_kwargs=any(p.kind is inspect.Parameter.VAR_KEYWORD for p in params.values())
    kwargs: dict[str,Any]={}
    if "前回結果" in params or has_kwargs: kwargs["前回結果"]=None
    if "HDS履歴" in params or has_kwargs: kwargs["HDS履歴"]=()
    if "文脈" in params or has_kwargs: kwargs["文脈"]=HDS文脈()
    return compile_fn(入力,**kwargs)

def HDS独立カーネルコンパイル(構文化器, 入力: str):
    bundle_fn=getattr(構文化器,"コンパイル束",None)
    if not callable(bundle_fn): return None
    束=_独立呼出(bundle_fn,入力)
    if not hasattr(束,"意味IR") or not hasattr(束,"コア入力"): raise TypeError("コンパイル束のKernel契約が不足")
    return 束

def HDS独立コンパイル(構文化器: HDSコンパイラProtocol, 入力: str) -> HDSIR:
    束=HDS独立カーネルコンパイル(構文化器,入力)
    if 束 is not None: return 束.意味IR
    compile_fn=getattr(構文化器,"意味コンパイル",None)
    if not callable(compile_fn): compile_fn=構文化器.コンパイル
    return _独立呼出(compile_fn,入力)

def HDS独立コア入力コンパイル(構文化器, 入力: str) -> HDSコア入力束:
    束=HDS独立カーネルコンパイル(構文化器,入力)
    if 束 is not None:
        正本=getattr(束,"コア入力",None)
        if not isinstance(正本,HDSコア入力束): raise TypeError("Kernel Core射影の戻り型不正")
        return 正本
    compile_fn=getattr(構文化器,"コア入力コンパイル",None)
    if callable(compile_fn):
        結果=_独立呼出(compile_fn,入力)
        if not isinstance(結果,HDSコア入力束): raise TypeError("Core入力コンパイルの戻り型不正")
        return 結果
    return HDSコア入力へ(HDS独立コンパイル(構文化器,入力))

__all__=["HDS文脈","HDSコンパイラProtocol","HDSカーネルコンパイラProtocol","HDSコア入力コンパイラProtocol","HDS独立カーネルコンパイル","HDS独立コンパイル","HDS独立コア入力コンパイル"]

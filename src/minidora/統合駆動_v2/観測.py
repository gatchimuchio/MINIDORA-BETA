"""未観測関係から観測要求を構成し、仮説を分別する観測を優先する。"""
from __future__ import annotations
from dataclasses import dataclass, field
from typing import Callable
from .値 import 文字, 文字列組, 整数, 不変値
from .認識 import HDS出典, HDS認識項目, 認識区分
from .仮説 import HDS仮説, 識別対数
from .記憶 import HDS資料


@dataclass(frozen=True, slots=True)
class HDS観測要求:
    ID: str
    対象: str
    関係: str
    必要性: tuple[str, ...] = ()
    範囲: str = "未指定"
    時点: str = "未指定"
    語群: tuple[str, ...] = ()
    依存: tuple[str, ...] = ()
    解消残差: tuple[str, ...] = ()
    手段: tuple[str, ...] = ("参照", "計算", "質問")

    def __post_init__(self):
        for n in ("ID", "対象", "関係", "範囲", "時点"):
            文字(getattr(self, n), n)
        for n in ("必要性", "語群", "依存", "解消残差", "手段"):
            文字列組(getattr(self, n), n)

    @property
    def 問合せ(self) -> str:
        return f"対象={self.対象}\n関係={self.関係}\n範囲={self.範囲}\n時点={self.時点}"


@dataclass(frozen=True, slots=True)
class HDS観測値:
    値: object
    出典群: tuple[HDS出典, ...]
    対象: str
    関係: str
    範囲: str = "未指定"
    時点: str = "未指定"
    資料群: tuple[HDS資料, ...] = ()

    def __post_init__(self):
        if not isinstance(self.資料群, tuple) or any(not isinstance(x, HDS資料) for x in self.資料群): raise TypeError("観測原資料の型が不正")
        不変値(self.値)
        if not isinstance(self.出典群, tuple) or not self.出典群 or any(not isinstance(x, HDS出典) for x in self.出典群): raise ValueError("観測値には出典が必要")
        for n in ("対象", "関係", "範囲", "時点"): 文字(getattr(self, n), n)


@dataclass(frozen=True, slots=True)
class HDS観測器:
    ID: str
    取得: Callable = field(repr=False, compare=False, metadata={"意味": False})
    検証: Callable = field(repr=False, compare=False, metadata={"意味": False})
    版: str = "v1"
    検証版: str = "v1"
    手段: str = "参照"
    資源負荷: int = 1
    必要権限: tuple[str, ...] = ()

    def __post_init__(self):
        for n in ("ID", "版", "検証版", "手段"): 文字(getattr(self, n), n)
        if not callable(self.取得) or not callable(self.検証): raise TypeError("取得・検証には明示的な関数が必要")
        整数(self.資源負荷, "観測資源負荷"); 文字列組(self.必要権限)


def 必要観測を構成(認識群: tuple[HDS認識項目, ...], 要求認識: frozenset[str], 仮説群: tuple[HDS仮説, ...] = (), 既存: tuple[HDS観測要求, ...] = (), *, 最大件数: int = 128) -> tuple[HDS観測要求, ...]:
    整数(最大件数, "最大観測要求", 1)
    認識={x.ID:x for x in 認識群}; 要求={x.ID:x for x in 既存}; 必要=set(要求認識)
    for h in 仮説群:
        if h.区分 not in (認識区分.失効, 認識区分.棄却): 必要.update(x.観測ID for x in h.予測 if 識別対数(仮説群,x.観測ID)>0)
    for ID in sorted(必要):
        x=認識.get(ID)
        if x and x.区分==認識区分.確定: continue
        if ID in 要求: continue
        if x is None: continue
        要求[ID]=HDS観測要求(ID,x.対象,x.関係,("要求認識の充足" if ID in 要求認識 else "競合仮説の識別",),x.範囲,x.時点,(x.対象,x.関係),x.依存)
    未完=[x for x in 要求.values() if x.ID not in 認識 or 認識[x.ID].区分!=認識区分.確定]
    if len(未完)>最大件数: raise ValueError("観測要求数が政策上限を超えた。切捨ては行わない")
    return tuple(sorted(未完,key=lambda x:(-識別対数(仮説群,x.ID),x.ID)))


def 観測範囲が一致(要求: HDS観測要求, 値: HDS観測値) -> bool:
    return (要求.対象==値.対象 and 要求.関係==値.関係 and (要求.範囲=="未指定" or 要求.範囲==値.範囲) and (要求.時点=="未指定" or 要求.時点==値.時点))

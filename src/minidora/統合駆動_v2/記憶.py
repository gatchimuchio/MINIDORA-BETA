"""正本・作業状態・索引・参照計画を分離する。圧縮物は証拠へ昇格しない。"""
from __future__ import annotations
from dataclasses import dataclass, replace, field
from hashlib import sha256
from .値 import 文字, 文字列組, 整数, 署名
from .認識 import HDS出典

@dataclass(frozen=True, slots=True)
class HDS資料:
    ID:str;版:str;本文:str;出所:str;時点:str="未指定";語彙:tuple[str,...]=()
    def __post_init__(self):
        for n in ("ID","版","本文","出所","時点"):文字(getattr(self,n),n)
        文字列組(self.語彙)
    @property
    def 内容署名(self):return sha256(self.本文.encode("utf-8")).hexdigest()
    def 出典(self,範囲="全体"):return HDS出典(self.ID,self.版,self.内容署名,範囲,self.時点)

@dataclass(frozen=True, slots=True)
class HDS参照索引:
    語:str;資料ID群:tuple[str,...]
    def __post_init__(self):文字(self.語);文字列組(self.資料ID群)

@dataclass(frozen=True, slots=True)
class HDS参照計画:
    問い署名:str;正本署名:str;証拠署名:str;資料ID群:tuple[str,...];残使用回数:int=field(default=3,metadata={"意味":False})
    def __post_init__(self):
        for n in ("問い署名","正本署名","証拠署名"):文字(getattr(self,n),n)
        文字列組(self.資料ID群);整数(self.残使用回数,"参照残使用回数")
    def 再利用可能(self,問い,記憶,証拠署名):return self.残使用回数>0 and self.問い署名==署名(問い) and self.正本署名==記憶.正本署名 and self.証拠署名==証拠署名 and set(self.資料ID群).issubset(記憶.正本辞書())
    def 消費(self,問い,記憶,証拠署名):
        if not self.再利用可能(問い,記憶,証拠署名):raise ValueError("参照計画が失効している")
        正本=記憶.正本辞書();return tuple(正本[x] for x in self.資料ID群),replace(self,残使用回数=self.残使用回数-1)

@dataclass(frozen=True, slots=True)
class HDS圧縮記憶:
    ID:str;要約:str;出典群:tuple[HDS出典,...];抽出範囲:tuple[tuple[int,int],...]=();未収録範囲:tuple[tuple[int,int],...]=();方式:str="外部提供";契約署名:str=""
    def __post_init__(self):
        文字(self.ID);文字(self.要約)
        if not isinstance(self.出典群,tuple) or not self.出典群 or any(not isinstance(x,HDS出典) for x in self.出典群):raise ValueError("圧縮記憶には元正本の出典が必要")
    def 正本へ戻る(self,記憶):
        結果=[]
        for 元 in self.出典群:
            資料=記憶.正本辞書().get(元.資料ID)
            if 資料 is None or 資料.版!=元.版 or 資料.内容署名!=元.内容署名:raise ValueError("圧縮記憶の原本が更新/削除されている")
            結果.append(資料)
        return tuple(結果)

@dataclass(frozen=True, slots=True)
class HDS記憶:
    正本:tuple[HDS資料,...]=();旧版:tuple[HDS資料,...]=();圧縮:tuple[HDS圧縮記憶,...]=();計画:HDS参照計画|None=None
    def __post_init__(self):
        for 群,型 in ((self.正本,HDS資料),(self.旧版,HDS資料),(self.圧縮,HDS圧縮記憶)):
            if not isinstance(群,tuple) or any(not isinstance(x,型) for x in 群):raise TypeError("記憶要素型が不正")
        if len({x.ID for x in self.正本})!=len(self.正本):raise ValueError("現行正本IDの重複")
        if self.計画 is not None and not isinstance(self.計画,HDS参照計画):raise TypeError("参照計画型が不正")
    @property
    def 正本署名(self):return 署名(tuple(sorted((x.ID,x.版,x.内容署名,x.出所,x.時点,x.語彙) for x in self.正本)))
    def 正本辞書(self):return {x.ID:x for x in self.正本}
    def 更新(self,資料群):
        文字列組(tuple(x.ID for x in 資料群),"更新資料ID");現行=self.正本辞書();旧=list(self.旧版)
        for x in 資料群:
            前=現行.get(x.ID)
            if 前==x:continue
            if 前 and 前.版==x.版:raise ValueError("同一資料版の無言上書きは禁止")
            if 前:旧.append(前)
            現行[x.ID]=x
        新=replace(self,正本=tuple(現行[k] for k in sorted(現行)),旧版=tuple(旧));return replace(新,計画=None) if 新.正本署名!=self.正本署名 else 新
    def 索引(self):
        束={}
        for 資料 in self.正本:
            for 語 in 資料.語彙:束.setdefault(語.casefold(),set()).add(資料.ID)
        return tuple(HDS参照索引(k,tuple(sorted(v))) for k,v in sorted(束.items()))
    def 計画する(self,問い,語群,証拠署名,再利用回数=3):
        整数(再利用回数,"参照再利用回数",1);索引={x.語:x.資料ID群 for x in self.索引()};候補={ID for 語 in 語群 for ID in 索引.get(語.casefold(),())}
        return HDS参照計画(署名(問い),self.正本署名,証拠署名,tuple(sorted(候補)),再利用回数)

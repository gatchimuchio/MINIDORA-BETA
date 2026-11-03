"""採用前の差を失わない認識状態。分類は実装用の暫定契約でありHDSの総体ではない。"""
from __future__ import annotations
from dataclasses import dataclass,replace
from enum import StrEnum
from .値 import 文字,文字列組,整数,不変値,署名

class 認識区分(StrEnum):
    確定="確定";暫定="暫定";弱支持="弱支持";競合="競合";条件付き="条件付き";未観測="未観測";保留="保留";棄却="棄却";失効="失効"

@dataclass(frozen=True,slots=True)
class HDS出典:
    資料ID:str;版:str;内容署名:str;範囲:str="全体";取得時点:str="未指定";種別:str="外部資料"
    def __post_init__(self):
        for 名 in ("資料ID","版","内容署名","範囲","取得時点","種別"):文字(getattr(self,名),名)

@dataclass(frozen=True,slots=True)
class HDS認識項目:
    ID:str;対象:str;関係:str;値:object=None;区分:認識区分=認識区分.未観測;根拠:tuple[HDS出典,...]=();反証:tuple[HDS出典,...]=();条件:tuple[str,...]=();依存:tuple[str,...]=();範囲:str="未指定";時点:str="未指定";検証契約:str="";改訂:int=0
    def __post_init__(self):
        for 名 in ("ID","対象","関係","範囲","時点"):文字(getattr(self,名),名)
        if not isinstance(self.区分,認識区分):raise TypeError("認識区分型が必要")
        不変値(self.値);整数(self.改訂,"認識改訂");文字列組(self.条件,"条件");文字列組(self.依存,"依存")
        for 群 in (self.根拠,self.反証):
            if not isinstance(群,tuple) or any(not isinstance(x,HDS出典) for x in 群):raise TypeError("出典tupleが必要")
            if len(set(群))!=len(群):raise ValueError("同一出典の二重計上")
        if not isinstance(self.検証契約,str):raise TypeError("検証契約は文字列")
        if self.区分==認識区分.確定 and (not self.検証契約 or (not self.根拠 and not self.依存) or self.条件 or self.反証):raise ValueError("確定には検証契約と根拠/依存が必要。未充足条件・反証は残せない")
    @property
    def 意味署名(self):return 署名((self.対象,self.関係,self.値,self.区分,tuple(sorted(self.根拠,key=署名)),tuple(sorted(self.反証,key=署名)),self.条件,self.依存,self.範囲,self.時点,self.検証契約))
    def 失効させる(self):return replace(self,区分=認識区分.失効,改訂=self.改訂+1)

@dataclass(frozen=True,slots=True)
class HDS認識差:
    ID:str;変更項目:tuple[str,...];前署名:str|None;後署名:str|None

def 認識差分(前,後):
    a,b={x.ID:x for x in 前},{x.ID:x for x in 後};比較項目=("対象","関係","値","区分","根拠","反証","条件","依存","範囲","時点","検証契約");差=[]
    for ID in sorted(a.keys()|b.keys()):
        if ID not in a or ID not in b:差.append(HDS認識差(ID,("追加" if ID in b else "削除",),a[ID].意味署名 if ID in a else None,b[ID].意味署名 if ID in b else None))
        elif a[ID].意味署名!=b[ID].意味署名:差.append(HDS認識差(ID,tuple(k for k in 比較項目 if 署名(getattr(a[ID],k))!=署名(getattr(b[ID],k))),a[ID].意味署名,b[ID].意味署名))
    return tuple(差)

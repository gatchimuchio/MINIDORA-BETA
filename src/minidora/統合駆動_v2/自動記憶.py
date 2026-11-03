"""原文範囲を持つ抽出的な多解像度記憶。未収録範囲と原資料は消さない。"""
from __future__ import annotations
from dataclasses import replace
import re
from .値 import 署名, 整数
from .記憶 import HDS圧縮記憶


def 原資料を圧縮(資料, 語群=(), *, 最大文字数=512):
    整数(最大文字数,"圧縮文字数",32,1_000_000)
    segments=[(m.start(),m.end(),m.group()) for m in re.finditer(r"[^。！？\n]+[。！？\n]?",資料.本文)]
    scored=sorted(segments,key=lambda x:(-sum(t in x[2] for t in 語群),-int(any(t in x[2] for t in ("ただし","場合","ない","除く"))),x[0]))
    selected=[]; count=0
    for a,b,text in scored:
        extra=len(text)+int(bool(selected))
        if count+extra<=最大文字数:selected.append((a,b));count+=extra
    selected.sort(); omitted=[];pos=0
    for a,b in selected:
        if pos<a:omitted.append((pos,a))
        pos=b
    if pos<len(資料.本文):omitted.append((pos,len(資料.本文)))
    text="\n".join(資料.本文[a:b] for a,b in selected)
    if not text:text="原資料参照が必要（収録可能な完全文なし）"
    return HDS圧縮記憶("自動圧縮:"+資料.ID,text,(資料.出典(),),tuple(selected),tuple(omitted),"原文範囲付き抽出-v1",署名((語群,最大文字数)))


class 自動記憶圧縮作用:
    作用ID="内的/記憶圧縮"
    def __init__(self,閾値=1024,最大文字数=512):self.閾値,self.最大文字数=閾値,最大文字数
    def _更新(self,状態):
        terms=tuple(sorted({t for x in 状態.認識 for t in (x.対象,x.関係)}|set(状態.目的)))
        return tuple(原資料を圧縮(d,terms,最大文字数=self.最大文字数) for d in 状態.記憶.正本 if len(d.本文)>=self.閾値)
    def 機会(self,状態):
        from ..HDS実行主体 import HDS作用機会
        expected=self._更新(状態);present=tuple(x for x in 状態.記憶.圧縮 if x.ID.startswith("自動圧縮:"))
        if expected==present:return None
        return HDS作用機会(self.作用ID,署名((状態.記憶.正本署名,expected)),資源負荷=1,種別="記憶圧縮")
    def 実行(self,状態):
        from ..HDS実行主体 import HDS作用結果,HDS作用状態
        other=tuple(x for x in 状態.記憶.圧縮 if not x.ID.startswith("自動圧縮:"))
        return HDS作用結果(HDS作用状態.成立,記憶更新=replace(状態.記憶,圧縮=other+self._更新(状態)),理由=("原資料を保持した抽出圧縮。未収録範囲を明示し、圧縮物を確定証拠にしない",))

"""原入力の取得と隔離。内容を正規化せず、既存コンパイラへ原文のまま渡す。"""
from __future__ import annotations
from .契約 import 入力原本, 入力政策, 入力出所
from .封緘 import 封緘する


def 原本を取得(本文, *, 案件ID: str, 入力ID: str, 出所=入力出所.指示, 版=1, 選択肢=(),
             由来='利用者提供', 取得時点='未指定', 政策=入力政策()) -> 入力原本:
    if not isinstance(政策,入力政策): raise TypeError('入力政策が必要')
    符号化='Unicode'
    if type(本文) is bytes:
        if len(本文)>政策.最大入力バイト: raise ValueError('原入力容量上限')
        符号化='UTF-8'
        本文=本文.decode('utf-8',errors='strict')
    if type(本文) is not str: raise TypeError('原入力は文字列またはUTF-8バイト列')
    if len(本文.encode('utf-8'))>政策.最大入力バイト: raise ValueError('原入力容量上限')
    if isinstance(選択肢,(str,bytes)) or not isinstance(選択肢,(tuple,list)):
        raise TypeError('選択肢は文字列の列')
    if any(type(x) is not str for x in 選択肢): raise TypeError('選択肢をstr化で補完しない')
    if sum(len(x.encode('utf-8')) for x in 選択肢)>政策.最大選択総バイト: raise ValueError('選択肢総容量上限')
    return 入力原本(案件ID,入力ID,本文,出所,版,tuple(選択肢),由来,取得時点,符号化)


def 文脈を取得(*, 前回結果=None, HDS履歴=(), 文脈=None, 政策=入力政策()):
    if not isinstance(HDS履歴,(tuple,list)): raise TypeError('HDS履歴は列')
    return 封緘する({'前回結果':前回結果,'HDS履歴':tuple(HDS履歴),'文脈':文脈},
                  最大要素=政策.最大封緘要素,最大バイト=政策.最大封緘バイト)

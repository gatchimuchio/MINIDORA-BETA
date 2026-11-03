"""既存HDSコンパイラによる意味変換。関係の再解析・別パーサ・正答選択を追加しない。"""
from __future__ import annotations
from dataclasses import fields, is_dataclass
from ..HDSコア入力 import HDSコア入力束
from .封緘 import 封緘する
from .契約 import 入力診断


カーネル必須 = ('意味IR','計算計画','コア入力','参照観測要求','候補意味IR','候補検証契約',
              '数量計算契約','失敗署名候補','チェックリスト','認知世界差分','監査参照候補','作用差分構造','版')


def 既成入力を検査(原本, 値):
    """ホストが注入したコンパイラの返却データを検査する。Python実装の認証器ではない。"""
    if isinstance(値,HDSコア入力束):
        核=値; 方式='コア入力'
    elif is_dataclass(値) and not isinstance(値,type) and set(カーネル必須)<={f.name for f in fields(値)}:
        核=値.コア入力; 方式='カーネル'
        if not isinstance(getattr(値,'カーネル署名',None),str) or not 値.カーネル署名:
            raise TypeError('カーネル署名契約が不足')
        if not isinstance(核,HDSコア入力束): raise TypeError('カーネルのコア入力型不正')
        if getattr(値.意味IR,'原文',None)!=原本.本文: raise ValueError('カーネルと受理原文が異なる')
        if getattr(値.意味IR,'認知世界ID',None)!=核.認知世界ID: raise ValueError('カーネル内の世界が不一致')
    else: raise TypeError('HDSカーネル契約またはHDSコア入力束が必要')
    if 核.原文!=原本.本文: raise ValueError('コア入力と受理原文が異なる')
    核.コア需要を検査()
    診断=[]
    意味={x.ID for x in 核.意味項目}; 条件={x.ID for x in 核.条件}
    for x in (*核.意味項目,*核.条件,*核.目的,*核.作用要求,*核.実行制約,*核.表現制約.要求):
        span=getattr(x,'原文範囲',None)
        if span is not None and (not isinstance(span,tuple) or len(span)!=2 or any(type(i) is not int for i in span) or not 0<=span[0]<=span[1]<=len(原本.本文)):
            診断.append(入力診断('原文範囲不整合','原文範囲が受理原文の外にある',(x.ID,)))
    for r in 核.関係:
        不足=(set(r.始点)|set(r.終点))-意味
        不足条件=set(r.条件ID)-条件
        if 不足 or 不足条件: 診断.append(入力診断('参照未閉包','関係の意味参照または条件参照が欠落',(r.ID,*sorted(不足|不足条件))))
    if 方式=='カーネル':
        主IR=値.意味IR
        候補座標=tuple(str(x.内容) for x in sorted((x for x in 主IR.座標 if x.座標ID.startswith('選択肢:')),key=lambda x:x.座標ID))
        if 原本.選択肢:
            if 候補座標!=原本.選択肢: raise ValueError('問題束の選択肢・順序が受理入力と異なる')
            名=tuple(k for k,_ in 値.候補意味IR)
            if len(set(名))!=len(名): raise ValueError('候補意味IRのラベル重複')
            if 名 and 名!=tuple(chr(65+i) for i in range(len(原本.選択肢))): raise ValueError('候補意味IRの対応不一致')
            if 名 and tuple(x.原文 for _,x in 値.候補意味IR)!=原本.選択肢: raise ValueError('候補意味IRの原文不一致')
        elif 候補座標 or 値.候補意味IR: raise ValueError('一般入力に別の選択問題束を流用できない')
    elif 原本.選択肢:
        raise ValueError('一問一束を保証できない旧問題二重構文化方式は新入力系で使用しない')
    return 方式,tuple(診断)


def 既存構文化器で変換(原本, 文脈封緘, コンパイラ, *, カーネル正本=None, 入力正本=None):
    if カーネル正本 is not None or 入力正本 is not None:
        元文脈=文脈封緘.復元()
        if 元文脈!={'前回結果':None,'HDS履歴':(),'文脈':None}:
            raise ValueError('既成正本の元文脈を検証できない。別の解釈文脈を後付けしない')
    if カーネル正本 is not None:
        if 入力正本 is not None and 入力正本.意味署名!=カーネル正本.コア入力.意味署名:
            raise ValueError('二つの入力正本が異なる')
        値=カーネル正本
    elif 入力正本 is not None:
        値=入力正本
    else:
        if コンパイラ is None: raise ValueError('意味変換には既存HDSコンパイラが必要')
        文脈=文脈封緘.復元()
        if 原本.選択肢:
            if any(文脈[n] not in (None,()) for n in ('前回結果','HDS履歴','文脈')):
                raise ValueError('現行問題コンパイル束にない文脈引数を黙って破棄しない')
            関数=getattr(コンパイラ,'問題コンパイル束',None)
            if not callable(関数): raise TypeError('新入力系の選択入力には問題コンパイル束が必要')
            値=関数(原本.本文,原本.選択肢)
        else:
            関数=getattr(コンパイラ,'コンパイル束',None)
            if not callable(関数): 関数=getattr(コンパイラ,'コア入力コンパイル',None)
            if not callable(関数): raise TypeError('コンパイル束またはコア入力コンパイルが必要')
            値=関数(原本.本文,**文脈)
        if 封緘する(文脈).署名!=文脈封緘.署名:
            raise ValueError('コンパイラが呼出文脈を書き換えた')
    方式,診断=既成入力を検査(原本,値)
    return 値,方式,診断

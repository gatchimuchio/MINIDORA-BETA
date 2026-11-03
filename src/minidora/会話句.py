"""会話の句境界。引用内部の句読点や数式を分割せず、原文位置を返す。"""
from __future__ import annotations


def 句を分割(text: str):
    if type(text) is not str or len(text)>8192:
        raise ValueError('会話句の入力型・上限')
    start=0;quoted=False
    for i,char in enumerate(text):
        if char=='「':
            if quoted: raise ValueError('入れ子の引用は未対応')
            quoted=True
        elif char=='」':
            if not quoted: raise ValueError('引用の閉じが過剰')
            quoted=False
        elif not quoted and char in '、。；\n！？!?':
            if text[start:i].strip(): yield start,i
            start=i+1
    if quoted: raise ValueError('引用が閉じていない')
    if text[start:].strip(): yield start,len(text)

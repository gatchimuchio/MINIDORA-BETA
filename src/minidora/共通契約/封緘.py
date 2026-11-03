"""ネイティブ契約を変更せず、可変の内部値を不変木へ退避する。動的import/pickleは使わない。"""
from __future__ import annotations
from dataclasses import dataclass, fields, is_dataclass
from enum import Enum
from hashlib import sha256
import json
import math


@dataclass(frozen=True, slots=True)
class 封緘値:
    木: tuple
    型表: tuple[tuple[str, type], ...]
    署名: str

    def _戻す(self, 節):
        型 = dict(self.型表)
        def 戻す(節):
            種, 値 = 節
            if 種 == '基本': return 値
            if 種 == '浮動': return float.fromhex(値)
            if 種 == 'バイト': return bytes.fromhex(値)
            if 種 in ('tuple', 'list', 'set', 'frozenset'):
                return {'tuple':tuple, 'list':list, 'set':set, 'frozenset':frozenset}[種](戻す(x) for x in 値)
            if 種 == 'dict': return {戻す(k):戻す(v) for k,v in 値}
            if 種 == 'enum': return 型[値[0]](戻す(値[1]))
            if 種 == '構造': return 型[値[0]](**{k:戻す(v) for k,v in 値[1]})
            raise ValueError('封緘値の未定義タグ')
        if _木署名(self.木) != self.署名:
            raise ValueError('封緘値の内容不一致')
        return 戻す(節)

    def 復元(self):
        return self._戻す(self.木)

    def 欄を復元(self, 名前):
        if self.木[0]!='構造': raise TypeError('データ構造の欄だけ射影できる')
        for 鍵,節 in self.木[1][1]:
            if 鍵==名前: return self._戻す(節)
        raise ValueError('保持されていない構造欄: '+str(名前))

    @property
    def バイト数(self):
        return len(json.dumps(self.木,ensure_ascii=False,separators=(',',':'),allow_nan=False).encode('utf-8'))


def _木署名(木) -> str:
    return sha256(json.dumps(木, ensure_ascii=False, separators=(',', ':'), allow_nan=False).encode('utf-8')).hexdigest()


def 封緘する(元, *, 最大要素: int = 200000, 最大深さ: int = 96, 最大バイト: int = 16000000) -> 封緘値:
    """対応する値だけを完全保存。未知型をrepr/strに縮退させない。型表は信頼されたホスト由来。"""
    for n in (最大要素, 最大深さ, 最大バイト):
        if type(n) is not int or n < 1: raise ValueError('封緘資源は正の整数')
    型 = {}; 訪問 = set(); 数 = 0
    def 取る(値, 深さ):
        nonlocal 数
        数 += 1
        if 数 > 最大要素 or 深さ > 最大深さ: raise ValueError('入力封緘の資源上限')
        if isinstance(値, Enum):
            鍵 = type(値).__module__ + '.' + type(値).__qualname__
            if 鍵 in 型 and 型[鍵] is not type(値): raise ValueError('入力型名が競合')
            型[鍵] = type(値)
            return ('enum', (鍵, 取る(値.value, 深さ+1)))
        if 値 is None or type(値) in (str,int,bool): return ('基本', 値)
        if type(値) is float:
            if not math.isfinite(値): raise ValueError('非有限値は入力できない')
            return ('浮動', 値.hex())
        if type(値) is bytes: return ('バイト', 値.hex())
        識別 = id(値)
        if 識別 in 訪問: raise ValueError('循環する可変入力は封緘できない')
        訪問.add(識別)
        try:
            if type(値) in (tuple,list,set,frozenset):
                行 = tuple(取る(x,深さ+1) for x in 値)
                if type(値) in (set,frozenset): 行 = tuple(sorted(行,key=lambda x:json.dumps(x,ensure_ascii=False)))
                return (type(値).__name__,行)
            if type(値) is dict:
                return ('dict',tuple((取る(k,深さ+1),取る(v,深さ+1)) for k,v in 値.items()))
            if is_dataclass(値) and not isinstance(値,type):
                鍵=type(値).__module__+'.'+type(値).__qualname__
                if 鍵 in 型 and 型[鍵] is not type(値): raise ValueError('入力型名が競合')
                型[鍵]=type(値)
                if any(not f.init for f in fields(値)): raise TypeError('非初期化フィールドを持つ入力型は未対応')
                return ('構造',(鍵,tuple((f.name,取る(getattr(値,f.name),深さ+1)) for f in fields(値))))
            raise TypeError('入力封緘の未対応型: '+type(値).__qualname__)
        finally: 訪問.remove(識別)
    木 = 取る(元,0)
    文 = json.dumps(木,ensure_ascii=False,separators=(',',':'),allow_nan=False).encode('utf-8')
    if len(文)>最大バイト: raise ValueError('入力封緘の容量上限')
    return 封緘値(木,tuple(sorted(型.items())),sha256(文).hexdigest())

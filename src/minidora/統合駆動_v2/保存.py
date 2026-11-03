"""明示型だけをJSONで保存・復元する。pickle・動的import・実行コードを受け入れない。"""
from __future__ import annotations
import json
from dataclasses import fields, is_dataclass
from enum import Enum
from .値 import 整数


def _型表():
    from . import 認識, 依存, 記憶, 観測, 仮説, 計画, 検証, 形成, 政策, 入力境界, 意味構成, 未来, 診断
    # 親packageがクラスを同名exportする構成でも、モジュールを明示的に取得する。
    import importlib
    核 = importlib.import_module("minidora.HDS実行主体")
    from ..駆動系 import 契約 as 関係契約, 学習 as 関係学習
    表 = {}
    for モジュール in (認識, 依存, 記憶, 観測, 仮説, 計画, 検証, 形成, 政策, 入力境界, 意味構成, 未来, 診断, 核, 関係契約, 関係学習):
        for obj in vars(モジュール).values():
            if isinstance(obj, type) and obj.__module__ == モジュール.__name__ and (is_dataclass(obj) or issubclass(obj, Enum)):
                # Callableを持つ実行部品は保存しない。再開時に現行契約を明示注入する。
                if is_dataclass(obj) and any(f.metadata.get("意味") is False and f.name in ("取得", "検証") for f in fields(obj)):
                    continue
                表[obj.__module__ + "." + obj.__qualname__] = obj
    return 表


def 保存する(値, *, 最大バイト: int = 8_000_000) -> str:
    整数(最大バイト, "最大保存バイト", 1, 100_000_000)
    表 = _型表()
    関係形式 = [False]
    def enc(x, 深さ=0):
        if 深さ > 96:
            raise ValueError("保存深さ上限")
        if type(x).__module__.startswith("minidora.駆動系."):
            関係形式[0] = True
        if isinstance(x, Enum):
            key = type(x).__module__ + "." + type(x).__qualname__
            if key not in 表:
                raise TypeError("未登録enum")
            return {"enum": key, "value": enc(x.value, 深さ+1)}
        if x is None or type(x) in (bool, int, str):
            return x
        if type(x) is float:
            import math
            if not math.isfinite(x):
                raise ValueError("非有限値")
            return {"float": x.hex()}
        if isinstance(x, (tuple, list, set, frozenset)):
            seq = [enc(v,深さ+1) for v in x]
            if isinstance(x, (set, frozenset)):
                seq.sort(key=lambda v:json.dumps(v,sort_keys=True,ensure_ascii=False))
            return {type(x).__name__: seq}
        if isinstance(x, dict):
            seq = [[enc(k,深さ+1),enc(v,深さ+1)] for k,v in x.items()]
            seq.sort(key=lambda v:json.dumps(v[0],sort_keys=True,ensure_ascii=False))
            return {"dict":seq}
        if is_dataclass(x) and not isinstance(x,type):
            key=type(x).__module__+"."+type(x).__qualname__
            if key not in 表:
                raise TypeError("未登録成果型は保存できない: "+key)
            return {"type":key,"fields":{f.name:enc(getattr(x,f.name),深さ+1) for f in fields(x)}}
        raise TypeError("保存できない外部型: "+type(x).__qualname__)
    内容 = enc(値)
    形式 = "MINIDORA-RELATION-DRIVE-v1" if 関係形式[0] else "MINIDORA-HDS-STATE-v3"
    payload=json.dumps({"format":形式,"内容":内容},ensure_ascii=False,sort_keys=True,separators=(",",":"),allow_nan=False)
    if len(payload.encode('utf-8'))>最大バイト:
        raise ValueError("保存容量上限")
    return payload


def 復元する(文字列: str, *, 最大バイト: int = 8_000_000):
    整数(最大バイト,"最大保存バイト",1,100_000_000)
    if not isinstance(文字列,str) or len(文字列.encode('utf-8'))>最大バイト:
        raise ValueError("復元入力型または容量上限")
    def unique(pairs):
        d={}
        for k,v in pairs:
            if k in d:raise ValueError("JSON鍵の重複")
            d[k]=v
        return d
    root=json.loads(文字列,object_pairs_hook=unique,parse_constant=lambda _:(_ for _ in ()).throw(ValueError("非有限数")))
    if not isinstance(root,dict) or set(root)!={"format","内容"} or root['format'] not in ('MINIDORA-HDS-STATE-v3','MINIDORA-RELATION-DRIVE-v1'):
        raise ValueError("保存形式不一致")
    表=_型表()
    def dec(x,深さ=0):
        if 深さ>96:raise ValueError("復元深さ上限")
        if x is None or type(x) in (str,int,bool):return x
        if not isinstance(x,dict):raise ValueError("未定義の保存要素")
        if set(x)=={'enum','value'}:
            t=表.get(x['enum'])
            if not isinstance(t,type) or not issubclass(t,Enum):raise ValueError("未登録enum")
            return t(dec(x['value'],深さ+1))
        if set(x)=={'type','fields'}:
            if root['format']=='MINIDORA-HDS-STATE-v3' and str(x['type']).startswith('minidora.駆動系.'):
                raise ValueError('関係状態の旧形式偽装')
            t=表.get(x['type'])
            if t is None or not is_dataclass(t):raise ValueError("未登録構造型")
            if not isinstance(x['fields'],dict) or set(x['fields'])!={f.name for f in fields(t)}:raise ValueError("構造フィールド不一致")
            return t(**{k:dec(v,深さ+1) for k,v in x['fields'].items()})
        if len(x)!=1:raise ValueError("未定義のタグ")
        tag,value=next(iter(x.items()))
        if tag=='float':
            import math
            f=float.fromhex(value)
            if not math.isfinite(f):raise ValueError("非有限値")
            return f
        if tag in ('tuple','list','set','frozenset'):
            if not isinstance(value,list):raise ValueError("列形式不一致")
            return {'tuple':tuple,'list':list,'set':set,'frozenset':frozenset}[tag](dec(v,深さ+1) for v in value)
        if tag=='dict':
            if not isinstance(value,list):raise ValueError("写像形式不一致")
            out={}
            for pair in value:
                if not isinstance(pair,list) or len(pair)!=2:raise ValueError("写像要素不一致")
                k,v=(dec(y,深さ+1) for y in pair)
                if k in out:raise ValueError("写像鍵の重複")
                out[k]=v
            return out
        raise ValueError("未定義タグ")
    return dec(root['内容'])

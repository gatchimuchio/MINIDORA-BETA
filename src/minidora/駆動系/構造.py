"""構造対応は具体対象同一性と分離する。単なる索引一致を同型性の証明にしない。"""
from __future__ import annotations
from dataclasses import replace
from itertools import permutations, product
from .契約 import (関係項,関係節,関係束,関係写像,構造照合結果,関係資源契約,演算予算,関係資源超過,関係演算停止)
from ..コア.値 import 署名


def 関係を束縛(型:関係節,値:関係節,初期=None):
    if 型.骨格!=値.骨格: return None
    env=dict(初期 or {})
    for (_,pattern),(_,actual) in zip(型.引数,値.引数):
        if pattern.型!=actual.型 or pattern.単位!=actual.単位: return None
        if pattern.変数:
            if pattern in env and env[pattern]!=actual: return None
            env[pattern]=actual
        elif pattern!=actual: return None
    return env


def 関係を具体化(節:関係節,束縛):
    values=[]
    for 引数役割,value in 節.引数:
        if value.変数:
            if value not in 束縛: return None
            actual=束縛[value]
            if actual.型!=value.型 or actual.単位!=value.単位: raise ValueError('束縛の型・単位不一致')
            value=actual
        values.append((引数役割,value))
    return replace(節,引数=tuple(values))


def 構造を照合(左:関係束,右:関係束,*,述語対応=(),部分=False,固定対象=(),資源=関係資源契約(),停止要求=None,予算=None):
    """左を右の部分構造へ単射で写す。対称な候補を残し、予算未完了を不一致としない。"""
    if not isinstance(左,関係束) or not isinstance(右,関係束): raise TypeError('二つの関係束が必要')
    if type(部分) is not bool: raise TypeError('部分照合はbool')
    pred=dict(述語対応)
    if len(pred)!=len(tuple(述語対応)) or len(set(pred.values()))!=len(pred): raise ValueError('述語対応は単射')
    frozen=frozenset(固定対象)
    if len(左.節)>min(資源.最大深さ,256): return 構造照合結果((),False,0)
    if not 部分 and len(左.節)!=len(右.節): return 構造照合結果((),True,0)
    演算資源=予算 or 演算予算(資源,停止要求); results={}
    ordered=tuple(sorted(左.節,key=lambda s:(s.骨格,s.署名)))
    def visit(index,env,used):
        if index==len(ordered):
            mapping=関係写像(tuple(sorted(env.items())),tuple(sorted(pred.items())))
            if 署名(mapping) not in results:
                if len(results)>=資源.最大写像: raise 関係資源超過('写像数上限')
                results[署名(mapping)]=mapping
            return
        元関係=ordered[index]
        for pos,target in enumerate(右.節):
            演算資源.消費()
            if pos in used or replace(元関係,述語=pred.get(元関係.述語,元関係.述語)).骨格!=target.骨格: continue
            e=dict(env); inverse={v:k for k,v in e.items()}; valid=True
            for (_,a),(_,b) in zip(元関係.引数,target.引数):
                if (a.型,a.単位)!=(b.型,b.単位) or (a in frozen and a!=b): valid=False;break
                if (a in e and e[a]!=b) or (b in inverse and inverse[b]!=a): valid=False;break
                e[a]=b; inverse[b]=a
            if valid: visit(index+1,e,used|{pos})
    try: visit(0,{},set()); complete=True
    except 関係資源超過: complete=False
    return 構造照合結果(tuple(results[k] for k in sorted(results)),complete,演算資源.照合数)


def 写像で射影(束:関係束,写像:関係写像,世界ID:str):
    """明示された対応だけを適用する。部分対応の未束縛対象は勝手に生成しない。"""
    env=dict(写像.対応);pred=dict(写像.述語対応)
    if len(env)!=len(写像.対応) or len(set(env.values()))!=len(env): raise ValueError('対象写像は単射')
    out=[]
    for atom in 束.節:
        args=[]
        for 引数役割,t in atom.引数:
            if t not in env: raise ValueError('未束縛の射影対象: '+t.名前)
            u=env[t]
            if (t.型,t.単位)!=(u.型,u.単位): raise ValueError('射影の型・単位不一致')
            args.append((引数役割,u))
        out.append(replace(atom,引数=tuple(args),述語=pred.get(atom.述語,atom.述語)))
    return 関係束(tuple(out),世界ID,束.由来)


def 構造を抽象化(束:関係束,可変対象:tuple[関係項,...],*,資源=関係資源契約(),停止要求=None,予算=None):
    """宣言対象の名前だけを役割変数へ変える。厳密な最小正規形を有界列挙する。"""
    if not isinstance(可変対象,tuple) or len(set(可変対象))!=len(可変対象): raise ValueError('可変対象は一意tuple')
    nodes={t for s in 束.節 for _,t in s.引数}
    if not set(可変対象)<=nodes: raise ValueError('可変対象が関係束に存在しない')
    if len(可変対象)>9: raise 関係資源超過('厳密正規形の対象数上限9')
    演算資源=予算 or 演算予算(資源,停止要求); best=None; bestmap=None
    groups={}
    for t in 可変対象: groups.setdefault((t.型,t.単位),[]).append(t)
    keys=sorted(groups);offset={};total=0
    for key in keys: offset[key]=total;total+=len(groups[key])
    # 各型群の順列は最大9!。容量を先に限定し、その内側も予算で停止する。
    def arrangements(index=0,selected=()):
        if index==len(keys):
            yield selected
            return
        for order in permutations(sorted(groups[keys[index]])):
            yield from arrangements(index+1,selected+(order,))
    for selection in arrangements():
        演算資源.消費(); mapping={t:t for t in nodes}
        for key,order in zip(keys,selection):
            for i,t in enumerate(order): mapping[t]=関係項('役割'+str(offset[key]+i),t.型,t.単位,True,'構造')
        対応候補=写像で射影(束,関係写像(tuple(sorted(mapping.items()))),'抽象構造')
        canonical=tuple(sorted((s.骨格,tuple((r,t.名前,t.型,t.単位,t.変数,t.束縛域) for r,t in s.引数)) for s in 対応候補.節))
        if best is None or canonical<best: best=canonical;bestmap=tuple(sorted(mapping.items()))
    if bestmap is None: bestmap=tuple((t,t) for t in sorted(nodes))
    abstract=写像で射影(束,関係写像(bestmap),'抽象構造')
    return abstract,関係写像(bestmap)


def 不変部分を取得(原束:関係束,対象束群:tuple[関係束,...],写像群:tuple[関係写像,...]):
    """確認された対応の共通部分を抽出する。対象IDの単純集合積ではない。"""
    if len(対象束群)!=len(写像群) or not 対象束群: raise ValueError('比較束と写像が必要')
    retained=[]
    for atom in 原束.節:
        supported=True
        for target,mapping in zip(対象束群,写像群):
            try: projected=写像で射影(関係束((atom,),原束.世界ID),mapping,target.世界ID).節[0]
            except ValueError: supported=False;break
            if projected not in target.節: supported=False;break
        if supported: retained.append(atom)
    return 関係束(tuple(retained),原束.世界ID,原束.由来)


def 最大共通構造を取得(原束,対象束群,*,資源=関係資源契約(),停止要求=None):
    """有界な最大共通部分関係束。対応してから比較し、同順位の不変部分を残す。"""
    from itertools import combinations
    if len(原束.節)>12:
        raise 関係資源超過('最大共通構造の原関係数上限12')
    演算資源=演算予算(資源,停止要求);局所結果=[]
    try:
        for size in range(len(原束.節),0,-1):
            for subset in combinations(原束.節,size):
                演算資源.消費()
                対応候補=関係束(subset,原束.世界ID,原束.由来)
                matches=True
                for target in 対象束群:
                    matched=構造を照合(対応候補,target,部分=True,資源=資源,予算=演算資源)
                    if not matched.完了: return tuple(局所結果),False,演算資源.照合数
                    if not matched.写像: matches=False;break
                if matches:
                    if len(局所結果)>=資源.最大写像: return tuple(局所結果),False,演算資源.照合数
                    局所結果.append(対応候補)
            if 局所結果: return tuple(局所結果),True,演算資源.照合数
        return (),True,演算資源.照合数
    except 関係資源超過:
        return tuple(局所結果),False,演算資源.照合数

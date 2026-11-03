"""実行された関係変換を合成し、同一所有主体の後続要求へ反映する。

これは観測例だけから科学法則を発明する帰納器ではない。明示規則の合成証明を
保持する学習方式である。型・条件・定数を落とさず、答えの記憶へ縮退させない。
"""
from __future__ import annotations
from dataclasses import dataclass, replace
from .契約 import 関係項,関係節,関係変換契約,関係保証
from ..コア.値 import 署名,文字,文字列組
from ..統合駆動_v2.形成 import HDS形成採用状態


def _改名(rule,prefix):
    variables=sorted({v for p in (*rule.前提,rule.結論) for v in p.変数群})
    env={v:replace(v,名前='v'+str(i),束縛域=prefix) for i,v in enumerate(variables)}
    def atom(p): return replace(p,引数=tuple((引数役割,env.get(v,v)) for 引数役割,v in p.引数))
    return tuple(atom(p) for p in rule.前提),atom(rule.結論)


def 変換を合成(左:関係変換契約,右:関係変換契約,接続前提:int):
    """標準化された変数による一段の分解・再結合。条件は全て前提として残す。"""
    if type(接続前提) is not int or not 0<=接続前提<len(右.前提): raise ValueError('接続前提の位置が不正')
    lp,lh=_改名(左,'左');rp,rh=_改名(右,'右');join=rp[接続前提]
    if lh.骨格!=join.骨格: return None
    env={}
    def resolve(t):
        seen=set()
        while t in env:
            if t in seen: raise ValueError('変数束縛が循環')
            seen.add(t);t=env[t]
        return t
    for (_,a),(_,b) in zip(lh.引数,join.引数):
        a,b=resolve(a),resolve(b)
        if (a.型,a.単位)!=(b.型,b.単位): return None
        if a==b: continue
        if a.変数: env[a]=b
        elif b.変数: env[b]=a
        else: return None
    patterns=lp+tuple(p for i,p in enumerate(rp) if i!=接続前提)
    if len(patterns)>32: return None
    renamed={}
    def atom(p):
        args=[]
        for 引数役割,t in p.引数:
            t=resolve(t)
            if t.変数:
                if t not in renamed: renamed[t]=replace(t,名前='役割'+str(len(renamed)),束縛域='合成')
                t=renamed[t]
            args.append((引数役割,t))
        return replace(p,引数=tuple(args))
    premises=tuple(dict.fromkeys(atom(p) for p in patterns)); conclusion=atom(rh)
    # 同じ結論を前提に要求するだけの自己循環を新能力として形成しない。
    if conclusion in premises: return None
    dependencies={**dict(左.依存契約),**dict(右.依存契約)}
    if not 左.依存契約: dependencies[左.ID]=左.署名
    if not 右.依存契約: dependencies[右.ID]=右.署名
    key=署名((premises,conclusion,tuple(sorted(dependencies.items()))))
    guarantee=関係保証.導出 if 左.保証==右.保証==関係保証.導出 else 関係保証.仮説
    return 関係変換契約('形成変換:'+key[:24],premises,conclusion,
        tuple(sorted(set(左.由来)|set(右.由来))),'合成v1',guarantee,tuple(sorted(dependencies.items())))


@dataclass(frozen=True,slots=True)
class 関係形成:
    ID: str
    契約: 関係変換契約
    左: 関係変換契約
    右: 関係変換契約
    接続前提: int
    支持根拠: tuple[str,...]
    支持経験: tuple[str,...]
    採用状態: HDS形成採用状態 = HDS形成採用状態.有効
    反例: tuple[str,...] = ()
    審査履歴: tuple[tuple[str,str,str],...] = ()
    再検証署名: str = ''
    版: int = 1

    def __post_init__(self):
        文字(self.ID)
        for rows in (self.支持根拠,self.支持経験,self.反例): 文字列組(rows)
        if self.契約!=変換を合成(self.左,self.右,self.接続前提): raise ValueError('形成物の合成証明不一致')
        if self.ID!=self.契約.ID: raise ValueError('形成ID不一致')
        if not isinstance(self.採用状態,HDS形成採用状態): raise TypeError('採用状態型が必要')
        if self.契約.保証!=関係保証.導出: raise ValueError('未検証仮説を有効な変換形成へ昇格しない')


@dataclass(frozen=True,slots=True)
class 関係学習状態:
    形成: tuple[関係形成,...] = ()
    残差: tuple[str,...] = ()

    def __post_init__(self):
        if not isinstance(self.形成,tuple) or any(not isinstance(x,関係形成) for x in self.形成): raise TypeError('形成tupleが必要')
        文字列組(tuple(x.ID for x in self.形成),'形成ID')
        文字列組(self.残差)
        if len(self.形成)>128: raise ValueError('構造形成の保持上限128')

    @property
    def 署名(self): return 署名(self)


def 有効形成を取得(状態:関係学習状態,規則群):
    roots={r.ID:r.署名 for r in 規則群}; forms={r.ID:r for r in 状態.形成}
    def valid(record,seen=frozenset()):
        if record.ID in seen or record.採用状態!=HDS形成採用状態.有効: return False
        if any(roots.get(k)!=v for k,v in record.契約.依存契約): return False
        for parent in (record.左,record.右):
            if parent.依存契約:
                old=forms.get(parent.ID)
                if old is None or old.契約!=parent or not valid(old,seen|{record.ID}): return False
            elif roots.get(parent.ID)!=parent.署名: return False
        return True
    return tuple(x.契約 for x in 状態.形成 if valid(x))


def 実行経験を形成(状態:関係学習状態,要求,変換結果,出力):
    """実際に回答へ使った契約対だけから、証明付きの共通変換を形成する。"""
    if 出力.要求署名!=要求.署名 or 変換結果.要求署名!=要求.署名: raise ValueError('学習経験の要求不一致')
    rows={x.ID:x for x in 状態.形成}
    if 出力.状態=='競合':
        for key in 出力.使用形成:
            if key in rows:
                old=rows[key]; counter=署名((要求.署名,出力.未充足))
                rows[key]=replace(old,採用状態=HDS形成採用状態.隔離,反例=tuple(sorted(set(old.反例)|{counter})),再検証署名='',版=old.版+1)
        return 関係学習状態(tuple(rows[k] for k in sorted(rows)),状態.残差)
    if not 要求.学習 or 出力.状態!='成立': return 状態
    rules={x.ID:x for x in 変換結果.取得.変換}
    used={k for a in 出力.回答 for k in a.使用契約}
    records=[x for x in 変換結果.導出 if x.契約ID in used and not x.仮説]
    by_atom={}
    for r in records: by_atom.setdefault(r.節.署名,[]).append(r)
    roots=tuple(sorted({k for a in 出力.回答 for k in a.根拠}))
    # 入力証拠の根拠集合が同じなら言い換え・再実行を独立経験へ水増ししない。
    experience=署名(tuple(sorted({root for e in 要求.証拠 for root in e.根拠})))
    形成残差=set(状態.残差)
    # 有効形成を実際に使った新しい経験を帰還する。再表現は独立支持へ数えない。
    for ID in 出力.使用形成:
        old=rows.get(ID)
        if old is not None and old.採用状態==HDS形成採用状態.有効 and experience not in old.支持経験:
            if len(old.支持経験)>=256 or len(set(old.支持根拠)|set(roots))>2048:
                形成残差.add('形成支持記録容量')
            else:
                rows[ID]=replace(old,支持経験=tuple(sorted((*old.支持経験,experience))),支持根拠=tuple(sorted(set(old.支持根拠)|set(roots))))
    for right in records:
        for pos,key in enumerate(right.前提署名):
            for left in by_atom.get(key,()):
                if left.契約ID==right.契約ID and left.節.署名==right.節.署名: continue
                combined=変換を合成(rules[left.契約ID],rules[right.契約ID],pos)
                if combined is None or combined.保証!=関係保証.導出: continue
                old=rows.get(combined.ID)
                if old is not None:
                    if old.採用状態!=HDS形成採用状態.有効: continue
                    if experience not in old.支持経験:
                        if len(old.支持経験)>=256 or len(set(old.支持根拠)|set(roots))>2048:
                            形成残差.add('形成支持記録容量');continue
                        rows[old.ID]=replace(old,支持経験=tuple(sorted((*old.支持経験,experience))),支持根拠=tuple(sorted(set(old.支持根拠)|set(roots))))
                elif len(rows)<128:
                    rows[combined.ID]=関係形成(combined.ID,combined,rules[left.契約ID],rules[right.契約ID],pos,roots,(experience,))
                else: 形成残差.add('関係形成保持容量')
    return 関係学習状態(tuple(rows[k] for k in sorted(rows)),tuple(sorted(形成残差)))


def 関係形成を隔離(状態:関係学習状態,ID:str,反例:str):
    文字(反例)
    rows={x.ID:x for x in 状態.形成}
    if ID not in rows: raise KeyError(ID)
    old=rows[ID]
    rows[ID]=replace(old,採用状態=HDS形成採用状態.隔離,反例=tuple(sorted(set(old.反例)|{反例})),再検証署名='',版=old.版+1)
    return 関係学習状態(tuple(rows[k] for k in sorted(rows)),状態.残差)


def 関係形成を再検証(状態:関係学習状態,ID:str,要求):
    from .取得 import 関係を取得
    from .変換 import 関係を変換
    from .射影 import 関係結果を射影
    rows={x.ID:x for x in 状態.形成};old=rows[ID]
    if old.採用状態!=HDS形成採用状態.隔離: raise ValueError('隔離中の形成だけ再検証できる')
    trial=replace(old,採用状態=HDS形成採用状態.有効)
    temp=replace(状態,形成=tuple(trial if x.ID==ID else x for x in 状態.形成))
    additions=有効形成を取得(temp,要求.変換)
    if ID not in {x.ID for x in additions}: raise ValueError('現行の依存契約が不一致')
    acquisition=関係を取得(要求,additions);conversion=関係を変換(要求,acquisition)
    output=関係結果を射影(要求,conversion,additions)
    if output.状態!='成立' or ID not in output.使用形成: raise ValueError('再実行で対象形成が使用・成立していない')
    rows[ID]=replace(old,再検証署名=署名((要求.署名,output,old.反例,old.版)))
    return 関係学習状態(tuple(rows[k] for k in sorted(rows)),状態.残差)


def 関係形成を審査(状態:関係学習状態,ID:str,処置:str,理由:str,承認主体:str,反例参照:tuple[str,...]):
    文字(理由);文字(承認主体);文字列組(反例参照)
    rows={x.ID:x for x in 状態.形成};old=rows[ID]
    if old.採用状態!=HDS形成採用状態.隔離 or set(反例参照)!=set(old.反例): raise ValueError('現行の隔離反例を明示する必要がある')
    if 処置=='復帰':
        if not old.再検証署名: raise ValueError('隔離後の実再検証が必要')
        status=HDS形成採用状態.有効
    elif 処置=='棄却': status=HDS形成採用状態.棄却
    else: raise ValueError('処置は復帰または棄却')
    rows[ID]=replace(old,採用状態=status,審査履歴=(*old.審査履歴,(処置,理由,承認主体)),再検証署名='',版=old.版+1)
    return 関係学習状態(tuple(rows[k] for k in sorted(rows)),状態.残差)

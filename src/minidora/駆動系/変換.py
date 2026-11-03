"""駆動系の変換。既存作用は契約・例外分類・入力不変条件を維持して実行する。"""
from __future__ import annotations
from copy import deepcopy
from ..統合駆動_v2.政策 import HDS作用失敗
from ..統合駆動_v2.診断 import 例外を診断


def 作用を変換(作用, 選択, 現在):
    """現状態の複製へ作用させ、検証前の局所結果を返す。状態採用は行わない。"""
    from ..HDS実行主体 import HDS作用結果, HDS作用状態
    前 = deepcopy(現在)
    前署名 = 前.状態署名
    try:
        結果 = 作用.実行(前)
        if 前.状態署名 != 前署名:
            raise ValueError("作用器が受け取った主体状態を直接変更した")
        if not isinstance(結果, HDS作用結果):
            raise TypeError("作用結果契約違反")
    except HDS作用失敗 as exc:
        結果 = HDS作用結果(HDS作用状態.保留 if exc.阻害.修復可能 else HDS作用状態.失敗,
                           理由=(exc.阻害.詳細,), 阻害=exc.阻害,
                           停止要求=not exc.阻害.修復可能)
    except Exception as exc:
        診断, 阻害 = 例外を診断(exc, 作用, 選択, 現在)
        結果 = HDS作用結果(HDS作用状態.保留 if 阻害.修復可能 else HDS作用状態.失敗,
                           理由=(診断.観測記述,), 停止要求=not 阻害.修復可能,
                           阻害=阻害, 診断=診断)
    return 結果


def 関係を変換(要求,取得,*,停止要求=None):
    """関数記号を持たない明示規則を有界適用し、各適用の証明を保持する。"""
    from .契約 import (関係取得結果,関係変換結果,関係導出,演算予算,
                     関係資源超過,関係演算停止,関係保証)
    from .構造 import 関係を束縛,関係を具体化
    from ..コア.値 import 署名
    from .契約 import 構造要求
    if isinstance(要求,構造要求):
        return _構造を変換(要求,取得,停止要求=停止要求)
    if not isinstance(取得,関係取得結果) or 取得.要求署名!=要求.署名:
        raise ValueError('取得結果の目的・入力版が異なる')
    演算資源=演算予算(要求.資源,停止要求)
    facts={x.節.署名:x.節 for x in 取得.証拠}
    depth={key:0 for key in facts}
    proved=set(facts); records={}; complete=True; reason='有限閉包'
    try:
        changed=True
        while changed:
            changed=False
            for rule in 取得.変換:
                candidates=[({},())]
                for premise in rule.前提:
                    new=[]
                    for env,parents in candidates:
                        for key,fact in tuple(sorted(facts.items())):
                            演算資源.消費()
                            bound=関係を束縛(premise,fact,env)
                            if bound is not None: new.append((bound,parents+(key,)))
                    candidates=new
                    if not candidates: break
                for env,parents in candidates:
                    演算資源.消費()
                    atom=関係を具体化(rule.結論,env)
                    if atom is None or atom.変数群: raise ValueError('導出に未束縛変数')
                    d=1+max(depth[k] for k in parents)
                    if d>min(要求.資源.最大深さ,256): raise 関係資源超過('導出深さ上限')
                    hypothetical=rule.保証==関係保証.仮説 or any(k not in proved for k in parents)
                    record=関係導出(atom,rule.ID,rule.署名,parents,tuple(sorted(env.items())),rule.由来,d,hypothetical)
                    key=署名((atom.署名,rule.ID,parents,tuple(sorted(env.items()))))
                    if key not in records:
                        if len(records)>=要求.資源.最大導出: raise 関係資源超過('導出記録上限')
                        records[key]=record
                    if atom.署名 not in facts:
                        if len(facts)>=要求.資源.最大事実: raise 関係資源超過('事実数上限')
                        facts[atom.署名]=atom;depth[atom.署名]=d;changed=True
                    elif d<depth[atom.署名]:
                        depth[atom.署名]=d
                    if not hypothetical and atom.署名 not in proved:
                        proved.add(atom.署名); changed=True
    except 関係資源超過 as exc: complete=False;reason=str(exc)
    except 関係演算停止 as exc: complete=False;reason='明示停止:'+str(exc)
    return 関係変換結果(要求.署名,取得,tuple(records[k] for k in sorted(records)),complete,reason,演算資源.照合数,len(facts))


def _構造を変換(要求,取得,*,停止要求=None):
    from .契約 import 構造取得結果,構造変換結果,関係資源超過,関係演算停止,演算予算
    from .構造 import 構造を照合,構造を抽象化,写像で射影,最大共通構造を取得
    if not isinstance(取得,構造取得結果) or 取得.要求!=要求:
        raise ValueError('構造取得結果の要求が異なる')
    if 停止要求 is not None and 停止要求():
        return 構造変換結果(要求.署名,要求.操作,完了=False,理由=('明示停止',))
    try:
        if 要求.操作=='照合':
            value=構造を照合(要求.原束,要求.対象束[0],述語対応=要求.述語対応,部分=要求.部分,資源=要求.資源,停止要求=停止要求)
            return 構造変換結果(要求.署名,要求.操作,写像群=value.写像,
                一致=bool(value.写像) if value.完了 else None,完了=value.完了,
                理由=() if value.完了 else ('構造対応探索が未完了',),照合数=value.照合数)
        if 要求.操作=='抽象化':
            演算資源=演算予算(要求.資源,停止要求)
            bundle,mapping=構造を抽象化(要求.原束,要求.可変対象,資源=要求.資源,停止要求=停止要求,予算=演算資源)
            return 構造変換結果(要求.署名,要求.操作,(bundle,),(mapping,),照合数=演算資源.照合数)
        if 要求.操作=='射影':
            bundle=写像で射影(要求.原束,要求.写像[0],要求.世界ID)
            return 構造変換結果(要求.署名,要求.操作,(bundle,),要求.写像)
        bundles,complete,count=最大共通構造を取得(要求.原束,要求.対象束,資源=要求.資源,停止要求=停止要求)
        return 構造変換結果(要求.署名,要求.操作,bundles,完了=complete,
            理由=() if complete else ('最大共通構造の探索が未完了',),照合数=count)
    except 関係資源超過 as exc:
        return 構造変換結果(要求.署名,要求.操作,完了=False,理由=(str(exc),))
    except 関係演算停止:
        return 構造変換結果(要求.署名,要求.操作,完了=False,理由=('明示停止',))

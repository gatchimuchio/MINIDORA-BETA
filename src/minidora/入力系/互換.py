"""旧入口の入力準備実体。意味決定は既存HDSコンパイラへ委ねる互換入口。"""
from __future__ import annotations
from dataclasses import dataclass


@dataclass(frozen=True, slots=True)
class 入力準備結果:
    作用群: tuple
    残差群: frozenset
    成果初期値: tuple
    主体初期値: tuple
    成立初期値: frozenset
    目的初期値: tuple


def 既存入力を準備(問合せ, コンパイラ, *, カーネル正本=None, 入力正本=None,
                前回結果=None, HDS履歴=(), 文脈=None, 作用群=(), 残差群=(),
                成果初期値=(), 主体初期値=(), 成立初期値=(), 目的初期値=()):
    """元の準備ブロックを移設。返却後の状態所有・適用・採否は駆動系が行う。"""
    if カーネル正本 is not None or コンパイラ is not None:
        from ..HDS構文化処理系列_v1_4 import HDSカーネル束
    if 入力正本 is not None or カーネル正本 is not None or コンパイラ is not None:
        from ..HDSコア入力 import HDSコア入力束
    作用群=list(作用群); 残差群=set(残差群)
    成果初期値=dict(成果初期値); 主体初期値=dict(主体初期値)
    成立初期値=set(成立初期値); 目的初期値=list(目的初期値)
    kernel = カーネル正本
    if kernel is not None and not isinstance(kernel, HDSカーネル束):
        raise TypeError("カーネル正本はHDSカーネル束である必要がある")
    コア入力 = 入力正本
    if コア入力 is not None and not isinstance(コア入力, HDSコア入力束):
        raise TypeError("入力正本はHDSコア入力束である必要がある")

    if kernel is None and コンパイラ is not None:
        kernel_fn = getattr(コンパイラ, "コンパイル束", None)
        if callable(kernel_fn):
            kernel = kernel_fn(問合せ, 前回結果=前回結果, HDS履歴=tuple(HDS履歴), 文脈=文脈)
            if not isinstance(kernel, HDSカーネル束):
                raise TypeError("Compiler KernelがHDSカーネル束を返さなかった")
        elif コア入力 is None:
            コア関数 = getattr(コンパイラ, "コア入力コンパイル", None)
            if callable(コア関数):
                コア入力 = コア関数(問合せ, 前回結果=前回結果, HDS履歴=tuple(HDS履歴), 文脈=文脈)
                if not isinstance(コア入力, HDSコア入力束):
                    raise TypeError("Legacy Core入力構文化器がHDSコア入力束を返さなかった")
            else:
                from ..HDS構文化作用 import HDS構文化作用
                残差群.add("入力未構文化")
                作用群.append(HDS構文化作用(コンパイラ, 問合せ, 前回結果=前回結果, HDS履歴=tuple(HDS履歴), 文脈=文脈))

    if kernel is not None:
        if コア入力 is not None and コア入力.意味署名 != kernel.コア入力.意味署名:
            raise ValueError("Kernelと明示Core入力が同一意味正本からの射影ではない")
        コア入力 = kernel.コア入力
        if "HDSカーネル束" in 成果初期値 or "HDSカーネル署名" in 主体初期値:
            raise ValueError("HDSカーネルの予約初期キーは呼出側から上書きできない")
        成果初期値["HDSカーネル束"] = kernel
        主体初期値["HDSカーネル署名"] = kernel.カーネル署名
        成立初期値.add("HDSカーネル形成済み")

    if コア入力 is not None:
        if "HDSコア入力" in 成果初期値 or "HDSコア入力署名" in 主体初期値 or "HDS目的正本" in 主体初期値:
            raise ValueError("HDSコア入力・目的正本の予約初期キーは呼出側から上書きできない")
        成果初期値["HDSコア入力"] = コア入力
        主体初期値["HDSコア入力署名"] = コア入力.意味署名
        # Compilerが抽出した目的の内容・対象参照をID文字列へ縮退させず、Core寿命中の目的正本として保持する。
        主体初期値["HDS目的正本"] = tuple(コア入力.目的)
        成立初期値.add("HDSコア入力済み")
        残差群.update(f"HDS残差:{項目.種別}:{項目.理由}" for 項目 in コア入力.残差)
        目的初期値.extend(f"HDS目的:{項目.ID}:{項目.種別}" for 項目 in コア入力.目的)

    return 入力準備結果(tuple(作用群),frozenset(残差群),tuple(成果初期値.items()),
                       tuple(主体初期値.items()),frozenset(成立初期値),tuple(目的初期値))


def 受理入力を準備(束, *, 作用群=(), 残差群=(), 成果初期値=(), 主体初期値=(), 成立初期値=(), 目的初期値=()):
    """新契約の受理済み束。Compiler具体実装をロードせず同じ駆動初期化形式へ渡す。"""
    from .射影 import 指示束を検査
    内容 = 指示束を検査(束)
    核 = 内容.コア入力 if 束.方式=='カーネル' else 内容
    成果=dict(成果初期値); 主体=dict(主体初期値)
    成立=set(成立初期値); 残差=set(残差群); 目的=list(目的初期値)
    予約成果={'HDSコア入力','HDSカーネル束'}
    予約主体={'HDSコア入力署名','HDSカーネル署名','HDS目的正本','HDS入力受理署名','HDS入力案件'}
    if 予約成果&成果.keys() or 予約主体&主体.keys(): raise ValueError('受理済み入力の予約キーは上書きできない')
    if 束.方式=='カーネル':
        成果['HDSカーネル束']=内容
        # 旧署名は維持する。全内容と原本は入力受理署名で別途束縛している。
        主体['HDSカーネル署名']=内容.カーネル署名
        成立.add('HDSカーネル形成済み')
    成果['HDSコア入力']=核
    主体['HDSコア入力署名']=核.意味署名
    主体['HDS目的正本']=tuple(核.目的)
    主体['HDS入力受理署名']=束.全域署名
    主体['HDS入力案件']=束.原本.鍵
    成立.add('HDSコア入力済み')
    残差.update(f'HDS残差:{x.種別}:{x.理由}' for x in 核.残差)
    残差.update('入力境界:'+x.種別+':'+x.理由 for x in 束.診断)
    目的.extend(f'HDS目的:{x.ID}:{x.種別}' for x in 核.目的)
    return 入力準備結果(tuple(作用群),frozenset(残差),tuple(成果.items()),tuple(主体.items()),frozenset(成立),tuple(目的))

"""HDSコア入力から既存関係駆動へ、明示対応だけを射影する。原文を再解析しない。"""
from __future__ import annotations
from dataclasses import dataclass
from ..コア.値 import 文字, 文字列組
from ..駆動系.契約 import 関係項, 関係節, 関係証拠, 関係保証, 関係要求, 関係変換契約, 関係資源契約
from .契約 import 入力束
from .射影 import 指示束を検査
from .封緘 import 封緘する


@dataclass(frozen=True, slots=True)
class 座標対応票:
    座標ID: str
    内容署名: str
    項: 関係項
    根拠: tuple[str,...]

    def __post_init__(self):
        文字(self.座標ID); 文字(self.内容署名); 文字列組(self.根拠)
        if not self.根拠 or not isinstance(self.項,関係項): raise ValueError('座標対応は関係項と明示根拠が必要')


@dataclass(frozen=True, slots=True)
class 入力関係契約:
    """駆動側が指定する消費面。前提IDは問題内の明示前提であり、外部観測の承認ではない。

    問い・前提・対象対応・規則を指定する。正解を入力から推測して埋める契約ではない。
    契約は入力束の全域署名へ固定し、別入力・改訂への無言流用を拒否する。
    """
    入力署名: str
    問いID: str
    前提ID: tuple[str,...] = ()
    座標対応: tuple[座標対応票,...] = ()
    変換: tuple[関係変換契約,...] = ()
    資源: 関係資源契約 = 関係資源契約()
    学習: bool = True

    def __post_init__(self):
        文字(self.入力署名);文字(self.問いID);文字列組(self.前提ID)
        if self.問いID in self.前提ID: raise ValueError('問い自体を前提へ採用できない')
        if not isinstance(self.座標対応,tuple) or any(not isinstance(x,座標対応票) for x in self.座標対応): raise TypeError('座標対応票tupleが必要')
        文字列組(tuple(x.座標ID for x in self.座標対応))
        if not isinstance(self.変換,tuple) or any(not isinstance(x,関係変換契約) for x in self.変換): raise TypeError('変換契約tupleが必要')
        if not isinstance(self.資源,関係資源契約) or type(self.学習) is not bool: raise TypeError('資源・学習指定不正')


# これらは元のHDSコア入力射影が分類する照合用メタデータ。世界の条件と混ぜない。
from ..HDSコア入力射影 import _関係メタ鍵群 as _照合メタ
_問い専用 = frozenset({'問い適合','説明適合','命題適合','同定','数量同定'})


def 関係要求へ射影(束: 入力束, 契約: 入力関係契約) -> 関係要求:
    指示束を検査(束)
    if not isinstance(契約,入力関係契約): raise TypeError('入力関係契約が必要')
    if 束.全域署名!=契約.入力署名: raise ValueError('関係射影契約が別入力版を参照している')
    if 束.原本.選択肢: raise ValueError('候補集合の意味を失わないため選択入力は既存の選択経路へ渡す')
    核=束.コア入力
    if 核.残差 or 束.診断:
        raise ValueError('未解決入力を関係要求へ無言で縮退できない: '+','.join(x.ID for x in 核.残差))
    if 核.実行制約 or 核.作用要求 or 核.要求成果 or 核.表現制約.要求:
        raise ValueError('この関係要求型で実行・成果・表現要求を保持適用できない。完全な入力束を既存入口へ渡す')
    座標={x.ID:x for x in 核.意味項目}; 関係={x.ID:x for x in 核.関係}; 条件={x.ID:x for x in 核.条件}
    if not {契約.問いID,*契約.前提ID}<=関係.keys(): raise ValueError('指定された問いまたは前提が存在しない')
    対応={x.座標ID:x for x in 契約.座標対応}
    関連ID={k for r in (関係[契約.問いID],*(関係[k] for k in 契約.前提ID)) for k in (*r.始点,*r.終点)}
    if not set(対応)<=関連ID: raise ValueError('関係に使われない座標の対応を混入できない')
    for ID,票 in 対応.items():
        if ID not in 座標 or 封緘する(座標[ID]).署名!=票.内容署名: raise ValueError('古い座標対応票')
        if 票.項.変数 and not (座標[ID].状態=='未観測' and 座標[ID].種別.startswith('目的.未知')):
            raise ValueError('具体的な既知座標を変数へ一般化しない')
        if not 票.項.変数 and 座標[ID].状態!='確定': raise ValueError('未確定座標の値を対応票で補完しない')

    def 節を作る(r, 問い=False):
        if not 問い and (r.状態!='確定' or r.種別 in _問い専用):
            raise ValueError('問い・未確定関係を問題前提へ昇格できない')
        if 問い and r.種別 in _問い専用:
            raise ValueError('問い専用の述語対応は既存候補検証経路が所有する。単なる構造同名へ変換しない')
        if not 問い and any('不足位置=' in x for x in r.制約): raise ValueError('未知端点を持つ関係は前提にできない')
        限定=[]; 極性=[]; 不足=[]
        for raw in r.制約:
            k,sep,v=raw.partition('=');k=k.strip();v=v.strip()
            if sep and k=='極性':
                if v not in ('肯定','否定'): raise ValueError('未知の極性指定')
                極性.append(v); continue
            if sep and k=='不足位置': 不足.append(v)
            if sep and k=='選択意図' and v not in ('通常',''):
                raise ValueError('選択意図の反転を通常関係問合せへ読み替えない')
            if sep and k in _照合メタ: continue
            限定.append(raw)
        if len(set(極性))>1: raise ValueError('関係の極性が競合')
        for ID in r.条件ID:
            c=条件.get(ID)
            if c is None or c.状態!='確定': raise ValueError('未確定または欠落した関係条件')
            # 条件の作用域を失わない。同じ表層だけで別条件IDを同一視しない。
            限定.append('条件:'+ID+':'+封緘する((c.種別,c.内容)).署名)
        引数=[]; 範囲=set(); 時点=set()
        for side,群 in (('始点',r.始点),('終点',r.終点)):
            for i,ID in enumerate(群):
                x=座標.get(ID)
                if x is None: raise ValueError('関係端点の参照欠落')
                if x.範囲!='未指定': 範囲.add(x.範囲)
                if x.時点!='未指定': 時点.add(x.時点)
                if ID in 対応: 項=対応[ID].項
                elif 問い and x.状態=='未観測' and x.種別.startswith('目的.未知') and side in 不足:
                    項=関係項(ID,型='HDS座標',変数=True,束縛域='入力:'+束.全域署名)
                elif x.状態=='確定':
                    # 文字列一致は具体同一性の証明ではない。元座標IDを保持する。
                    項=関係項(核.認知世界ID+':'+ID,型='HDS座標')
                else: raise ValueError('変数宣言のない未確定端点')
                if 項.変数 and not 問い: raise ValueError('前提に変数を含められない')
                引数.append((side if len(群)==1 else side+':'+str(i),項))
        if len(範囲)>1 or len(時点)>1: raise ValueError('異なる端点範囲・時点を一関係へ縮退できない')
        return 関係節(r.種別,tuple(引数),肯定=not 極性 or 極性[0]!='否定',条件=tuple(sorted(set(限定))),
                     範囲=next(iter(範囲),'問題内'),時点=next(iter(時点),'未指定'))

    問い=節を作る(関係[契約.問いID],True)
    # 原本・文脈・全Kernelは入力全域署名へ束縛済み。各根拠へ全メタデータを重複展開しない。
    証拠=tuple(関係証拠('入力:'+束.全域署名+':'+ID,節を作る(関係[ID]),
                       ('入力関係:'+束.全域署名+':'+ID,
                        *(k for 票 in 契約.座標対応 for k in 票.根拠)),関係保証.前提) for ID in 契約.前提ID)
    return 関係要求('入力:'+束.原本.案件ID+':'+束.原本.入力ID,束.原本.本文,核.認知世界ID,問い,証拠,
                   契約.変換,版=str(束.原本.版)+':'+束.全域署名,資源=契約.資源,学習=契約.学習)

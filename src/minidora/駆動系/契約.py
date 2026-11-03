"""入力系・出力系から独立した、不変の関係操作契約。外部の実行コードを保持しない。"""
from __future__ import annotations
from dataclasses import dataclass, field
from enum import StrEnum
from ..コア.値 import 文字, 整数, 文字列組, 署名


class 関係保証(StrEnum):
    前提 = '問題内前提'
    観測 = '提供観測'
    導出 = '契約内導出'
    仮説 = '未検証仮説'


@dataclass(frozen=True, slots=True, order=True)
class 関係項:
    """対象の同一性と型を保持する。変数化は明示指定だけ。数量の単位も落とさない。"""
    名前: str
    型: str = '対象'
    単位: str = ''
    変数: bool = False
    束縛域: str = ''

    def __post_init__(self):
        文字(self.名前); 文字(self.型)
        if not isinstance(self.単位, str) or not isinstance(self.束縛域, str):
            raise TypeError('単位・束縛域は文字列')
        if type(self.変数) is not bool:
            raise TypeError('変数指定はbool')
        if self.変数 and not self.束縛域:
            raise ValueError('変数には明示束縛域が必要')
        if not self.変数 and self.束縛域:
            raise ValueError('定数へ変数束縛域を付けない')


@dataclass(frozen=True, slots=True)
class 関係節:
    """役割付き多項関係。条件・量化・様相・範囲・時点は厳密に一致させる。"""
    述語: str
    引数: tuple[tuple[str, 関係項], ...]
    肯定: bool = True
    条件: tuple[str, ...] = ()
    範囲: str = '問題内'
    時点: str = '未指定'
    様相: str = '断言'
    量化: str = '個別'

    def __post_init__(self):
        for name in ('述語','範囲','時点','様相','量化'): 文字(getattr(self,name),name)
        if type(self.肯定) is not bool: raise TypeError('極性はbool')
        if not isinstance(self.引数,tuple) or not 1 <= len(self.引数) <= 32:
            raise ValueError('役割付き引数は1..32項のtuple')
        for row in self.引数:
            if not isinstance(row,tuple) or len(row)!=2 or not isinstance(row[1],関係項):
                raise TypeError('役割と関係項の組が必要')
            文字(row[0],'役割')
        文字列組(tuple(x[0] for x in self.引数),'役割')
        文字列組(self.条件,'条件')
        object.__setattr__(self,'引数',tuple(sorted(self.引数)))
        object.__setattr__(self,'条件',tuple(sorted(self.条件)))

    @property
    def 変数群(self) -> frozenset[関係項]:
        return frozenset(v for _,v in self.引数 if v.変数)

    @property
    def 署名(self) -> str: return 署名(self)

    @property
    def 骨格(self) -> tuple:
        return (self.述語,tuple(k for k,_ in self.引数),self.肯定,self.条件,self.範囲,self.時点,self.様相,self.量化)


@dataclass(frozen=True, slots=True)
class 関係束:
    節: tuple[関係節, ...]
    世界ID: str
    由来: tuple[str, ...] = ()

    def __post_init__(self):
        文字(self.世界ID); 文字列組(self.由来)
        if not isinstance(self.節,tuple) or any(not isinstance(x,関係節) for x in self.節):
            raise TypeError('関係束は関係節tuple')
        if len(self.節)>4096: raise ValueError('関係束の容量上限')
        # 関係の記述順を世界の順序関係へ取り違えない。順序は関係として明示する。
        unique={x.署名:x for x in self.節}
        object.__setattr__(self,'節',tuple(unique[k] for k in sorted(unique)))


@dataclass(frozen=True, slots=True)
class 関係証拠:
    ID: str
    節: 関係節
    根拠: tuple[str, ...]
    保証: 関係保証 = 関係保証.前提

    def __post_init__(self):
        文字(self.ID)
        if not isinstance(self.節,関係節) or self.節.変数群: raise ValueError('証拠は具体的な関係節')
        文字列組(self.根拠)
        if not self.根拠: raise ValueError('根拠参照が必要')
        if self.保証 not in (関係保証.前提,関係保証.観測):
            raise ValueError('自己生成した導出・仮説を新規入力証拠へ転用できない')
        if not isinstance(self.保証,関係保証): raise TypeError('関係保証型が必要')


@dataclass(frozen=True, slots=True)
class 関係変換契約:
    ID: str
    前提: tuple[関係節, ...]
    結論: 関係節
    由来: tuple[str, ...]
    版: str = 'v1'
    保証: 関係保証 = 関係保証.導出
    依存契約: tuple[tuple[str, str], ...] = ()

    def __post_init__(self):
        文字(self.ID); 文字(self.版); 文字列組(self.由来)
        if not self.由来: raise ValueError('変換契約の由来が必要')
        if not isinstance(self.前提,tuple) or not 1 <= len(self.前提) <= 32 or any(not isinstance(x,関係節) for x in self.前提):
            raise ValueError('変換前提は1..32の関係節tuple')
        if not isinstance(self.結論,関係節): raise TypeError('結論関係節が必要')
        bound=frozenset(v for p in self.前提 for v in p.変数群)
        if not self.結論.変数群 <= bound: raise ValueError('結論の未束縛変数・新規対象生成は禁止')
        declarations={}
        for v in bound:
            key=(v.束縛域,v.名前)
            if key in declarations and declarations[key]!=(v.型,v.単位): raise ValueError('同じ変数の型・単位が競合')
            declarations[key]=(v.型,v.単位)
        if self.保証 not in (関係保証.導出,関係保証.仮説) or not isinstance(self.保証,関係保証):
            raise TypeError('変換保証は導出または仮説')
        if not isinstance(self.依存契約,tuple): raise TypeError('契約依存はtuple')
        for k,v in self.依存契約: 文字(k); 文字(v)
        文字列組(tuple(k for k,_ in self.依存契約),'契約依存ID')

    @property
    def 署名(self) -> str: return 署名(self)


@dataclass(frozen=True, slots=True)
class 関係資源契約:
    最大照合: int = 20000
    最大事実: int = 512
    最大導出: int = 1024
    最大深さ: int = 32
    最大写像: int = 128

    def __post_init__(self):
        for name in ('最大照合','最大事実','最大導出','最大深さ','最大写像'):
            整数(getattr(self,name),name,1,1_000_000)


@dataclass(frozen=True, slots=True)
class 関係要求:
    ID: str
    目的: str
    世界ID: str
    問い: 関係節
    証拠: tuple[関係証拠, ...]
    変換: tuple[関係変換契約, ...] = ()
    版: str = 'v1'
    資源: 関係資源契約 = 関係資源契約()
    学習: bool = True

    def __post_init__(self):
        for name in ('ID','目的','世界ID','版'): 文字(getattr(self,name),name)
        if not isinstance(self.問い,関係節): raise TypeError('問いは関係節')
        for name,typ in (('証拠',関係証拠),('変換',関係変換契約)):
            rows=getattr(self,name)
            if not isinstance(rows,tuple) or any(not isinstance(x,typ) for x in rows): raise TypeError(name+'の型不正')
            文字列組(tuple(x.ID for x in rows),name+'ID')
        if not isinstance(self.資源,関係資源契約) or type(self.学習) is not bool: raise TypeError('資源・学習指定の型不正')
        if len(self.証拠)>self.資源.最大事実 or len(self.変換)>256: raise ValueError('入力容量上限')
        # 自由変数は問いの解答変数であり、規則変数とは別の束縛域を持つ。

    @property
    def 署名(self) -> str: return 署名(self)


@dataclass(frozen=True, slots=True)
class 関係導出:
    節: 関係節
    契約ID: str
    契約署名: str
    前提署名: tuple[str, ...]
    束縛: tuple[tuple[関係項,関係項], ...]
    根拠: tuple[str, ...]
    深さ: int
    仮説: bool = False


@dataclass(frozen=True, slots=True)
class 関係取得結果:
    要求署名: str
    証拠: tuple[関係証拠, ...]
    変換: tuple[関係変換契約, ...]
    関連述語: tuple[str, ...]


@dataclass(frozen=True, slots=True)
class 関係変換結果:
    要求署名: str
    取得: 関係取得結果
    導出: tuple[関係導出, ...]
    完了: bool
    停止理由: str
    照合数: int
    事実数: int


@dataclass(frozen=True, slots=True)
class 関係回答:
    節: 関係節
    束縛: tuple[tuple[関係項,関係項], ...]
    根拠: tuple[str, ...]
    使用契約: tuple[str, ...]
    保証: 関係保証


@dataclass(frozen=True, slots=True)
class 関係出力束:
    要求ID: str
    要求署名: str
    状態: str
    回答: tuple[関係回答, ...]
    仮説: tuple[関係回答, ...] = ()
    未充足: tuple[str, ...] = ()
    照合数: int = 0
    使用形成: tuple[str, ...] = ()

    def __post_init__(self):
        if self.状態 not in ('成立','未観測','競合','予算枯渇','停止'):
            raise ValueError('出力状態不正')
        if self.状態!='成立' and self.回答: raise ValueError('未成立出力は確定回答を持てない')
        if self.状態=='成立' and not self.回答: raise ValueError('成立には回答が必要')


@dataclass(frozen=True, slots=True)
class 関係写像:
    対応: tuple[tuple[関係項,関係項], ...]
    述語対応: tuple[tuple[str,str], ...] = ()


@dataclass(frozen=True, slots=True)
class 構造照合結果:
    写像: tuple[関係写像, ...]
    完了: bool
    照合数: int


class 関係資源超過(Exception):
    pass


class 関係演算停止(Exception):
    pass


class 演算予算:
    """一回の操作群で共有する予算。再帰呼出しでリセットしない。"""
    def __init__(self,契約:関係資源契約,停止要求=None):
        self.契約=契約; self.照合数=0; self.停止要求=停止要求

    def 消費(self):
        if self.停止要求 is not None and self.停止要求(): raise 関係演算停止('明示停止')
        if self.照合数>=self.契約.最大照合: raise 関係資源超過('関係照合予算')
        self.照合数+=1


@dataclass(frozen=True, slots=True)
class 構造要求:
    ID: str
    目的: str
    世界ID: str
    操作: str
    原束: 関係束
    対象束: tuple[関係束, ...] = ()
    写像: tuple[関係写像, ...] = ()
    可変対象: tuple[関係項, ...] = ()
    述語対応: tuple[tuple[str, str], ...] = ()
    部分: bool = False
    資源: 関係資源契約 = 関係資源契約()
    版: str = 'v1'

    def __post_init__(self):
        for name in ('ID','目的','世界ID','版'): 文字(getattr(self,name))
        if self.操作 not in ('照合','抽象化','射影','共通化'): raise ValueError('未知の構造操作')
        if not isinstance(self.原束,関係束): raise TypeError('原関係束が必要')
        for name,typ in (('対象束',関係束),('写像',関係写像),('可変対象',関係項)):
            seq=getattr(self,name)
            if not isinstance(seq,tuple) or any(not isinstance(x,typ) for x in seq): raise TypeError(name+'の型不正')
        if type(self.部分) is not bool or not isinstance(self.資源,関係資源契約): raise TypeError('構造要求の条件型不正')
        if self.操作=='照合' and len(self.対象束)!=1: raise ValueError('照合先は一つ')
        if self.操作=='射影' and len(self.写像)!=1: raise ValueError('射影写像は一つ')
        if self.操作=='共通化' and not self.対象束: raise ValueError('共通化先が必要')
        if len(self.対象束)>16: raise ValueError('構造比較束の容量上限')
        if not isinstance(self.述語対応,tuple): raise TypeError('述語対応はtuple')
        for k,v in self.述語対応: 文字(k);文字(v)

    @property
    def 署名(self): return 署名(self)


@dataclass(frozen=True, slots=True)
class 構造取得結果:
    要求: 構造要求


@dataclass(frozen=True, slots=True)
class 構造変換結果:
    要求署名: str
    操作: str
    関係束群: tuple[関係束, ...] = ()
    写像群: tuple[関係写像, ...] = ()
    一致: bool | None = None
    完了: bool = True
    理由: tuple[str,...] = ()
    照合数: int = 0


@dataclass(frozen=True, slots=True)
class 構造出力束:
    要求ID: str
    要求署名: str
    状態: str
    構造成果: 構造変換結果 | None
    未充足: tuple[str,...] = ()
    照合数: int = 0
    保証: str = '指定構造内の変換結果。別世界の観測事実ではない'

    @property
    def 回答(self):
        return (self.構造成果,) if self.状態=='成立' and self.構造成果 is not None else ()

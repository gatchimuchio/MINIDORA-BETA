'会話の目的・対象役割と、計画時の予定状態。モジュール名を意味入力に置かない。'
from __future__ import annotations
from copy import deepcopy
from dataclasses import asdict, dataclass, field
from hashlib import sha256
from .能力合成 import _符号化

会話意味版 = 'MINIDORA-会話意味-v0.4'


def 意味指紋(value) -> str:
    return sha256(_符号化(value)).hexdigest()


@dataclass(frozen=True, slots=True)
class 比較対象:
    資料: str
    行条件: tuple[tuple[str, str], ...] = ()


@dataclass(frozen=True, slots=True)
class 会話要求:
    原文: str
    行為: str
    対象: tuple[比較対象, ...] = ()
    属性: str = ''
    単位: str = ''
    詳細: bool = False
    時点差: bool = False
    外部禁止: bool = False
    補助: dict = field(default_factory=dict)
    対応: tuple[tuple[str, int, int], ...] = ()

    def 固定複製(self):
        if type(self.原文) is not str or not 0 < len(self.原文) <= 8192:
            raise ValueError('会話原文の型・上限')
        if self.行為 not in ('比較', '集合', '取得', '既存目的', '再表現', '訂正', '確認返答', '登録', '更新', '初期化', '会話', '命題照合', '命題選択', '命題訂正', '資料命題選択', '命題取得'):
            raise ValueError('未対応の会話行為')
        if type(self.対象) is not tuple or len(self.対象) > 8:
            raise ValueError('対象役割の数・型')
        for t in self.対象:
            if type(t) is not 比較対象 or type(t.資料) is not str or not 0 < len(t.資料) <= 128:
                raise ValueError('対象資料名不正')
            if type(t.行条件) is not tuple or len(t.行条件) > 4:
                raise ValueError('行選択条件不正')
            if any(type(p) is not tuple or len(p) != 2 or any(type(x) is not str or not x or len(x)>128 for x in p) for p in t.行条件):
                raise ValueError('行選択条件の値不正')
            if len(dict(t.行条件)) != len(t.行条件):
                raise ValueError('行選択条件の重複')
        for x in (self.属性, self.単位):
            if type(x) is not str or len(x) > 80 or any(ord(c)<32 for c in x):
                raise ValueError('属性又は単位不正')
        if any(type(x) is not bool for x in (self.詳細, self.時点差, self.外部禁止)):
            raise ValueError('会話条件はbool')
        if type(self.補助) is not dict or len(_符号化(asdict(self))) > 1100000:
            raise ValueError('会話意味の資料上限')
        for kind,a,b in self.対応:
            if type(kind) is not str or type(a) is not int or type(b) is not int or not 0 <= a < b <= len(self.原文):
                raise ValueError('原文対応不正')
        return deepcopy(self)


@dataclass(frozen=True, slots=True)
class 意味目的:
    種別: str
    引数: dict

    def 鍵(self):
        if type(self.種別) is not str or not self.種別 or type(self.引数) is not dict:
            raise ValueError('意味目的の型不正')
        raw = {'種別': self.種別, '引数': self.引数}
        if len(_符号化(raw)) > 32000:
            raise ValueError('意味目的の上限')
        return 意味指紋(raw)

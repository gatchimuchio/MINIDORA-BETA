"""命題の構造。語彙・固有名・世界知識を推論器へ焼き込まない。

原子、否定、連言、選言、前向き含意、量化を保持する有限の表現契約。
時点・様相は原子の照合軸であり、未指定を全時点や現実の事実へ昇格しない。
"""
from __future__ import annotations

from dataclasses import asdict, dataclass, replace
from .会話意味 import 意味指紋

命題版 = 'MINIDORA-命題-v0.2'


@dataclass(frozen=True, slots=True)
class 命題項:
    名前: str
    種別: str = '定数'

    def __post_init__(self):
        if (type(self.名前) is not str or not 0 < len(self.名前) <= 80
                or any(ord(c) < 32 for c in self.名前)):
            raise ValueError('命題項の名前不正')
        if self.種別 not in ('定数', '変数', '存在証人', '任意個体'):
            raise ValueError('命題項の種別不正')


@dataclass(frozen=True, slots=True)
class 命題式:
    種別: str
    述語: str = ''
    項: tuple[命題項, ...] = ()
    子: tuple['命題式', ...] = ()
    変数: str = ''
    時点: str = '未指定'
    様相: str = '記載'

    def 検査(self, 深さ=0, 束縛=frozenset(), *, 自由変数=False):
        if 深さ > 24:
            raise ValueError('命題式の深さ上限')
        if type(self.項) is not tuple or type(self.子) is not tuple:
            raise ValueError('命題式の列型不正')
        if type(self.時点) is not str or not 0 < len(self.時点) <= 80:
            raise ValueError('命題時点不正')
        if self.様相 not in ('記載', '可能', '義務'):
            raise ValueError('命題様相不正')
        if self.種別 == '帰属':
            if (self.述語 not in ('発言', '過去発言', '信念', '過去信念', '伝聞')
                    or len(self.項) != 1 or len(self.子) != 1 or self.変数
                    or type(self.項[0]) is not 命題項 or type(self.子[0]) is not 命題式
                    or self.様相 != '記載'):
                raise ValueError('発言・信念・伝聞の帰属構造不正')
            self.項[0].__post_init__()
            if self.項[0].種別 == '変数' and self.項[0].名前 not in 束縛 and not 自由変数:
                raise ValueError('帰属主体の自由変数')
            # 引用内容は独立した作用域。外側の変数や仮定を捕捉しない。
            count = 1 + self.子[0].検査(深さ + 1)
            if count > 256: raise ValueError('命題式の節点上限')
            return count
        if self.種別 == '原子':
            if (type(self.述語) is not str or not 0 < len(self.述語) <= 80
                    or any(ord(c) < 32 for c in self.述語)
                    or self.子 or self.変数 or len(self.項) > 4):
                raise ValueError('原子命題不正')
            for t in self.項:
                if type(t) is not 命題項:
                    raise ValueError('項型不正')
                t.__post_init__()
                if t.種別 == '変数' and t.名前 not in 束縛 and not 自由変数:
                    raise ValueError('自由変数が未束縛')
            return 1
        arity = {'否定': (1, 1), '連言': (2, 8), '選言': (2, 8),
                 '含意': (2, 2), '全称': (1, 1), '存在': (1, 1)}
        if self.種別 not in arity or self.述語 or self.項:
            raise ValueError('複合命題の種別不正')
        low, high = arity[self.種別]
        if not low <= len(self.子) <= high or any(type(c) is not 命題式 for c in self.子):
            raise ValueError('複合命題の子不正')
        if self.時点 != '未指定' or self.様相 != '記載':
            raise ValueError('時点・様相は原子へ射影する')
        bound = 束縛
        if self.種別 in ('全称', '存在'):
            命題項(self.変数, '変数')
            if self.変数 in 束縛:
                raise ValueError('変数の二重束縛は未対応')
            bound = 束縛 | {self.変数}
        elif self.変数:
            raise ValueError('量化外の変数宣言')
        count = 1 + sum(c.検査(深さ + 1, bound, 自由変数=自由変数) for c in self.子)
        if count > 256:
            raise ValueError('命題式の節点上限')
        return count

    def 辞書(self):
        self.検査(自由変数=True)
        return asdict(self)

    def 鍵(self):
        return 意味指紋(self.辞書())


def 原子(述語: str, *項: str | 命題項, 時点='未指定', 様相='記載') -> 命題式:
    out = 命題式('原子', 述語, tuple(命題項(t) if type(t) is str else t for t in 項),
                 時点=時点, 様相=様相)
    out.検査(自由変数=True)
    return out


def 結合(種別: str, *子: 命題式, 変数='') -> 命題式:
    out = 命題式(種別, 子=tuple(子), 変数=変数)
    out.検査(自由変数=True)
    return out


def 反対(式: 命題式) -> 命題式:
    return 式.子[0] if 式.種別 == '否定' else 結合('否定', 式)


def 置換(式: 命題式, 束縛: dict[str, 命題項]) -> 命題式:
    if 式.種別 in ('原子', '帰属'):
        return replace(式, 項=tuple(束縛.get(t.名前, t) if t.種別 == '変数' else t for t in 式.項))
    # 内側で束縛する変数には外側の置換を適用しない。
    local = {k: v for k, v in 束縛.items() if k != 式.変数}
    return replace(式, 子=tuple(置換(c, local) for c in 式.子))


def 文脈を付す(式: 命題式, *, 時点=None, 様相=None) -> 命題式:
    if 式.種別 in ('原子', '帰属'):
        if 式.種別 == '帰属' and 様相 is not None:
            raise ValueError('帰属全体への様相付与は未対応')
        if 時点 is not None and 式.時点 not in ('未指定', 時点):
            raise ValueError('異なる時点の入れ子は上書きしない')
        if 様相 is not None and 式.様相 != '記載':
            raise ValueError('様相の入れ子は単層へ潰さない')
        return replace(式, 時点=式.時点 if 時点 is None else 時点,
                       様相=式.様相 if 様相 is None else 様相)
    return replace(式, 子=tuple(文脈を付す(c, 時点=時点, 様相=様相) for c in 式.子))


def 原子群(式: 命題式):
    if 式.種別 in ('原子', '帰属'):
        yield 式
        return
    for c in 式.子:
        yield from 原子群(c)


def 命題を復元(value, *, 内部許可=False, 深さ=0) -> 命題式:
    keys = {'種別', '述語', '項', '子', '変数', '時点', '様相'}
    if 深さ > 24 or type(value) is not dict or set(value) != keys:
        raise ValueError('命題記録の型・未知欄・深さ')
    if type(value['項']) not in (list, tuple) or type(value['子']) not in (list, tuple):
        raise ValueError('命題記録の列型')
    if len(value['項']) > 4 or len(value['子']) > 8:
        raise ValueError('命題記録の列上限')
    terms = []
    for row in value['項']:
        if type(row) is not dict or set(row) != {'名前', '種別'}:
            raise ValueError('項記録の型・未知欄')
        if not 内部許可 and row['種別'] not in ('定数', '変数'):
            raise ValueError('利用者が存在証人・任意個体を作らない')
        terms.append(命題項(**row))
    out = 命題式(value['種別'], value['述語'], tuple(terms),
                 tuple(命題を復元(c, 内部許可=内部許可, 深さ=深さ + 1) for c in value['子']),
                 value['変数'], value['時点'], value['様相'])
    out.検査(自由変数=深さ != 0)
    return out


@dataclass(frozen=True, slots=True)
class 命題候補:
    式: 命題式
    原文: str
    範囲: tuple[int, int]
    読み: str


@dataclass(frozen=True, slots=True)
class 命題記載:
    識別子: str
    式: 命題式
    資料: str
    原文: str
    範囲: tuple[int, int]

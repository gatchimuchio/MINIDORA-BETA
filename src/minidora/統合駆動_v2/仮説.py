"""仮説の形成・競合・並列作業域。仮説生成を事実の採用と同一視しない。"""
from __future__ import annotations
from dataclasses import dataclass, replace
from .認識 import HDS認識項目, 認識区分
from .値 import 文字, 文字列組, 不変値, 署名


@dataclass(frozen=True, slots=True)
class HDS予測:
    観測ID: str
    値: object

    def __post_init__(self):
        文字(self.観測ID)
        不変値(self.値)


@dataclass(frozen=True, slots=True)
class HDS仮説:
    ID: str
    内容: str
    予測: tuple[HDS予測, ...] = ()
    条件: tuple[str, ...] = ()
    依存: tuple[str, ...] = ()
    区分: 認識区分 = 認識区分.暫定
    支持: tuple[str, ...] = ()
    反証: tuple[str, ...] = ()
    排他群: str = ""

    def __post_init__(self):
        文字(self.ID)
        文字(self.内容)
        for n in ("条件", "依存", "支持", "反証"):
            文字列組(getattr(self, n), n)
        if not isinstance(self.予測, tuple) or any(not isinstance(x, HDS予測) for x in self.予測):
            raise TypeError("仮説予測の型が不正")
        if len({x.観測ID for x in self.予測}) != len(self.予測):
            raise ValueError("一仮説の同一観測に複数予測を置けない")
        if not isinstance(self.区分, 認識区分):
            raise TypeError("仮説区分型が必要")
        if self.区分 == 認識区分.確定:
            raise ValueError("仮説はそれ自体では確定事実へ昇格しない")

    def 再照合(self, 認識群: tuple[HDS認識項目, ...]) -> HDS仮説:
        認識 = {x.ID: x for x in 認識群}
        支持, 反証 = [], []
        for p in self.予測:
            観測 = 認識.get(p.観測ID)
            if 観測 and 観測.区分 == 認識区分.確定:
                (支持 if 署名(p.値) == 署名(観測.値) else 反証).append(観測.ID)
        if any(x not in 認識 or 認識[x].区分 in (認識区分.失効, 認識区分.棄却) for x in self.依存):
            区分 = 認識区分.失効
        elif 反証:
            区分 = 認識区分.棄却
        else:
            区分 = 認識区分.条件付き if self.条件 else 認識区分.暫定
        return replace(self, 区分=区分, 支持=tuple(sorted(支持)), 反証=tuple(sorted(反証)))


@dataclass(frozen=True, slots=True)
class HDS仮説雛型:
    ID: str
    必要認識: tuple[str, ...]
    候補群: tuple[HDS仮説, ...]

    def __post_init__(self):
        文字(self.ID)
        文字列組(self.必要認識)
        if not self.候補群 or any(not isinstance(x, HDS仮説) for x in self.候補群):
            raise ValueError("明示的な候補構造が必要")
        文字列組(tuple(x.ID for x in self.候補群), "候補ID")

    def 生成(self, 認識群: tuple[HDS認識項目, ...]) -> tuple[HDS仮説, ...]:
        既知 = {x.ID for x in 認識群 if x.区分 == 認識区分.確定}
        if not set(self.必要認識).issubset(既知):
            return ()
        return tuple(replace(x, 依存=tuple(sorted(set(x.依存) | set(self.必要認識)))).再照合(認識群) for x in self.候補群)


def 識別対数(仮説群: tuple[HDS仮説, ...], 観測ID: str) -> int:
    候補 = [h for h in 仮説群 if h.区分 not in (認識区分.棄却, 認識区分.失効)]
    数 = 0
    for i, a in enumerate(候補):
        pa = {x.観測ID: x.値 for x in a.予測}
        for b in 候補[i + 1:]:
            if not a.排他群 or a.排他群 != b.排他群:
                continue
            pb = {x.観測ID: x.値 for x in b.予測}
            if 観測ID in pa and 観測ID in pb and 署名(pa[観測ID]) != 署名(pb[観測ID]):
                数 += 1
    return 数


@dataclass(frozen=True, slots=True)
class HDS作業枝:
    ID: str
    仮定: tuple[str, ...] = ()
    認識: tuple[HDS認識項目, ...] = ()

    def __post_init__(self):
        文字(self.ID)
        文字列組(self.仮定)
        if not isinstance(self.認識, tuple) or any(not isinstance(x, HDS認識項目) for x in self.認識):
            raise TypeError("枝の認識型が不正")
        文字列組(tuple(x.ID for x in self.認識))


def 枝を合流(枝群: tuple[HDS作業枝, ...]) -> tuple[HDS認識項目, ...]:
    """分岐由来のものは条件付きで保持。未確定値の数値平均はしない。"""
    出力 = []
    for 枝 in sorted(枝群, key=lambda x: x.ID):
        for x in 枝.認識:
            条件 = tuple(sorted(set(x.条件) | set(枝.仮定)))
            出力.append(replace(x, ID=f"枝/{枝.ID}/{x.ID}", 区分=認識区分.条件付き if 条件 else x.区分, 条件=条件))
    return tuple(出力)

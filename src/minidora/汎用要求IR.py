"""目的と素材を能力名から分離する局所IR。種別は型宣言であり事実認定ではない。"""
from __future__ import annotations
from copy import deepcopy
from dataclasses import dataclass
from graphlib import TopologicalSorter, CycleError
from .能力合成 import _結果辞書, _符号化
from .製品版.型 import 能力結果

要求IR版 = 'MINIDORA-汎用要求IR-v0.2'

@dataclass(frozen=True, slots=True)
class 目的指定:
    識別子: str
    対象: str  # 名前付き素材、又は同じ要求中の目的ID
    成果種別: str
    引数参照: str
    原文範囲: tuple[int, int]

@dataclass(frozen=True, slots=True)
class 要求被覆項:
    """要求IRの各明示要素が、どの目的・引数・出力へ接続したかを保持する。"""
    種別: str
    識別子: str
    参照先: tuple[str, ...] = ()
    原文範囲: tuple[int, int] | None = None

@dataclass(frozen=True, slots=True)
class 汎用要求IR:
    原文: str
    素材: dict[str, 能力結果]
    素材種別: dict[str, str]
    目的: tuple[目的指定, ...]
    引数資料: dict[str, dict]
    出力目的: tuple[str, ...]
    残差: tuple[str, ...] = ()

    def _検証(self) -> tuple[str, ...]:
        if type(self.原文) is not str or not self.原文.strip() or len(self.原文) > 8192:
            raise ValueError('要求原文の範囲外')
        if type(self.残差) is not tuple or any(type(x) is not str for x in self.残差):
            raise ValueError('残差型不正')
        if self.残差:
            raise ValueError('未解消の要求残差')
        if type(self.素材) is not dict or type(self.素材種別) is not dict or set(self.素材) != set(self.素材種別):
            raise ValueError('素材の型宣言不一致')
        if type(self.目的) is not tuple or not 1 <= len(self.目的) <= 16 or len(self.素材) > 32:
            raise ValueError('目的又は素材の数が範囲外')
        def 名前(x):
            if type(x) is not str or not x.strip() or len(x) > 128:
                raise ValueError('識別子不正')
        for name, value in self.素材.items():
            名前(name); 名前(self.素材種別[name]); _結果辞書(value)
            if not value.成立:
                raise ValueError('不成立の素材')
        if type(self.引数資料) is not dict or any(type(v) is not dict for v in self.引数資料.values()):
            raise ValueError('引数資料不正')
        names = set(self.素材)
        seen_goals = set()
        for goal in self.目的:
            if type(goal) is not 目的指定:
                raise ValueError('目的型不正')
            for x in (goal.識別子, goal.対象, goal.成果種別, goal.引数参照): 名前(x)
            if goal.識別子 in names or goal.識別子 in seen_goals or goal.引数参照 not in self.引数資料:
                raise ValueError('目的重複又は引数資料欠落')
            seen_goals.add(goal.識別子); names.add(goal.識別子)
            span = goal.原文範囲
            if (type(span) is not tuple or len(span) != 2 or any(type(x) is not int for x in span)
                    or not 0 <= span[0] < span[1] <= len(self.原文)):
                raise ValueError('原文範囲不正')
        goals = {g.識別子 for g in self.目的}
        if any(g.対象 not in names for g in self.目的):
            raise ValueError('対象参照欠落')
        if (type(self.出力目的) is not tuple or not self.出力目的
                or any(type(x) is not str or x not in goals for x in self.出力目的)
                or len(set(self.出力目的)) != len(self.出力目的)):
            raise ValueError('出力目的不正')
        if set(self.引数資料) != {g.引数参照 for g in self.目的}:
            raise ValueError('未使用の引数資料')

        # 要求IR自身で依存閉包を確認する。計画器に入る前に、出力へ接続しない
        # 目的や循環を「存在はするが無視された要求」にしない。
        deps = {g.識別子: (g.対象,) if g.対象 in goals else () for g in self.目的}
        try:
            order = tuple(TopologicalSorter(deps).static_order())
        except CycleError as exc:
            raise ValueError('目的依存の循環') from exc
        needed = set(self.出力目的)
        for name in reversed(order):
            if name in needed:
                needed.update(deps[name])
        if needed != goals:
            raise ValueError('出力に接続していない目的')

        raw = {'素材': {k: _結果辞書(v) for k, v in self.素材.items()}, '引数': self.引数資料}
        if len(_符号化(raw)) > 2000000:
            raise ValueError('要求資料のサイズ上限')
        return order

    def 固定複製(self):
        """型・参照・規模・要求閉包を検査してから複製。外部作用は起こさない。"""
        self._検証()
        return deepcopy(self)

    def 被覆台帳(self) -> tuple[要求被覆項, ...]:
        """要求の明示要素と、その消費先を決定論的に列挙する。意味の正しさは確定しない。"""
        self._検証()
        items: list[要求被覆項] = []
        for name in self.素材:
            users = tuple(g.識別子 for g in self.目的 if g.対象 == name)
            items.append(要求被覆項('素材', name, users))
        for goal in self.目的:
            items.append(要求被覆項('目的', goal.識別子, (goal.対象, goal.引数参照), goal.原文範囲))
        for name in self.引数資料:
            users = tuple(g.識別子 for g in self.目的 if g.引数参照 == name)
            items.append(要求被覆項('引数', name, users))
        for name in self.出力目的:
            items.append(要求被覆項('出力', name, (name,)))
        return tuple(items)

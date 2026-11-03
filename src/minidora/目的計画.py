"""成果状態から作用契約を逆引きし、既存合成計画へ降下する。

単一素材の型付き経路探索。多目的の依存は扱うが、任意の複数素材の意味的結合はしない。
計画中に能力を呼ばない。予定の出力型を観測済みの成果と混同しない。
"""
from __future__ import annotations
from dataclasses import dataclass
from graphlib import TopologicalSorter, CycleError
from .能力意味カタログ import 計画作用, 能力意味カタログ
from .汎用要求IR import 汎用要求IR, 要求被覆項
from .能力合成 import 合成計画, 合成工程, 素材参照
from .製品版.型 import 能力結果

@dataclass(frozen=True, slots=True)
class 目的計画結果:
    状態: str
    計画: 合成計画 | None
    資料: dict[str, 能力結果]
    目的出力: tuple[tuple[str, str], ...]
    作用経路: tuple[tuple[str, tuple[str, ...]], ...]
    理由: str = ''
    展開数: int = 0
    カタログ印: str = ''
    要求被覆: tuple[要求被覆項, ...] = ()

    @property
    def 成立(self):
        return self.状態 == '合格' and self.計画 is not None


class 目的計画器:
    def __init__(self, カタログ: 能力意味カタログ, *, 最大深さ=8, 最大展開数=512):
        if type(カタログ) is not 能力意味カタログ:
            raise ValueError('登録済み能力と照合したカタログが必要')
        for x, maximum in ((最大深さ, 16), (最大展開数, 4096)):
            if type(x) is not int or not 1 <= x <= maximum:
                raise ValueError('探索上限不正')
        self.カタログ, self._上限 = カタログ, (最大深さ, 最大展開数)

    def 計画する(self, 要求: 汎用要求IR, *, 禁止作用=()) -> 目的計画結果:
        count = 0
        coverage: tuple[要求被覆項, ...] = ()
        try:
            if type(要求) is not 汎用要求IR:
                raise ValueError('要求IRが必要')
            req = 要求.固定複製()
            coverage = req.被覆台帳()
            if type(禁止作用) is not tuple or any(type(x) is not str for x in 禁止作用):
                raise ValueError('禁止作用型不正')
            rules = self.カタログ.作用
            if len({r.識別子 for r in rules}) != len(rules) or set(禁止作用) - {r.識別子 for r in rules}:
                raise ValueError('作用IDの重複又は未登録の禁止作用')
            rules = tuple(r for r in rules if r.識別子 not in 禁止作用)
            goals = {g.識別子: g for g in req.目的}
            deps = {g.識別子: (g.対象,) if g.対象 in goals else () for g in req.目的}
            order = tuple(TopologicalSorter(deps).static_order())
            # 固定複製でも確認済みだが、計画器自身も未出力・未依存目的を黙って捨てない。
            needed = set(req.出力目的)
            for name in reversed(order):
                if name in needed: needed.update(deps[name])
            if needed != set(goals):
                raise ValueError('出力に接続していない目的')
            資料 = {'素材:' + k: v for k, v in req.素材.items()}
            outputs, states = {}, dict(req.素材種別)
            refs = {k: 素材参照('入力', '素材:' + k) for k in req.素材}
            steps, traces = [], []

            def 経路(情報源, target, args):
                paths = []
                best_length = self._上限[0] + 1
                def visit(want, 連鎖, used):
                    nonlocal count, best_length
                    count += 1
                    if count > self._上限[1]: raise ValueError('計画探索の展開上限')
                    if len(連鎖) >= min(self._上限[0], best_length): return
                    for rule in sorted(rules, key=lambda r: 情報源 not in r.入力状態):
                        if rule.出力状態 != want or rule.識別子 in used: continue
                        required = {k for _, k in rule.引数写像}
                        if not required <= set(args): continue
                        path = (rule, *連鎖)
                        if 情報源 in rule.入力状態:
                            consumed = {k for r in path for _, k in r.引数写像}
                            if consumed == set(args):
                                paths.append(path)
                                best_length = min(best_length, len(path))
                        for before in rule.入力状態:
                            if before != 情報源 and len(path) < best_length:
                                visit(before, path, used | {rule.識別子})
                visit(target, (), frozenset())
                if not paths: raise ValueError('適用経路なし又は引数未解決:' + target)
                shortest = min(map(len, paths))
                best = {tuple(r.識別子 for r in p): p for p in paths if len(p) == shortest}
                if len(best) != 1: raise ValueError('同順位の経路が複数あり未確定:' + target)
                return next(iter(best.values()))

            for name in order:
                goal = goals[name]
                args = req.引数資料[goal.引数参照]
                path = 経路(states[goal.対象], goal.成果種別, args)
                current = refs[goal.対象]
                追跡 = []
                for rule in path:
                    sid = f'目的工程:{len(steps) + 1:04d}'
                    inst, config = '指示:' + sid, '設定:' + sid
                    資料[inst] = 能力結果(True, req.原文[goal.原文範囲[0]:goal.原文範囲[1]])
                    資料[config] = 能力結果(True, '', データ=rule.設定(args))
                    steps.append(合成工程(sid, (rule.能力,), inst, (current,), config))
                    current = 素材参照('工程', sid)
                    追跡.append(rule.識別子)
                refs[name], states[name] = current, goal.成果種別
                outputs[name] = current.識別子
                traces.append((name, tuple(追跡)))
            if len(steps) > 64: raise ValueError('合成器の工程上限')
            plan = 合成計画(tuple(steps), tuple(outputs[g] for g in req.出力目的))
            return 目的計画結果('合格', plan, 資料, tuple(outputs.items()), tuple(traces),
                                  展開数=count, カタログ印=self.カタログ.ハッシュ, 要求被覆=coverage)
        except (ValueError, TypeError, KeyError, AttributeError, RecursionError, CycleError) as exc:
            return 目的計画結果('保留', None, {}, (), (), str(exc), count, self.カタログ.ハッシュ, coverage)

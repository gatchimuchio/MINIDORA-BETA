"""資料内の命題を有限導出し、問いの支持と反証を別々に求める。

前向き含意、明示否定、連言・選言、量化の具体化と仮定導入を実装する。
未導出を否定へ変えず、矛盾から任意命題を生成しない。完全な一階論理決定器ではない。
"""
from __future__ import annotations

from dataclasses import asdict, dataclass
from copy import deepcopy
from itertools import product
from .命題構造 import 命題項, 命題式, 命題記載, 結合, 反対, 置換, 原子群
from .会話意味 import 意味指紋


@dataclass(frozen=True, slots=True)
class 推論上限:
    操作数: int = 50000
    事実数: int = 1024
    項数: int = 48
    仮定深さ: int = 8
    根拠数: int = 4096

    def 検査(self):
        for n, maximum in ((self.操作数, 200000), (self.事実数, 4096),
                           (self.項数, 64), (self.仮定深さ, 12), (self.根拠数, 12000)):
            if type(n) is not int or not 1 <= n <= maximum:
                raise ValueError('命題推論の予算不正')
        return self


def 整形式(e: 命題式) -> 命題式:
    """否定の作用域を保つ。含意の反証には前件と明示反例の両方を要求する。"""
    if e.種別 == '帰属': return e
    if e.種別 != '否定':
        return 結合(e.種別, *(整形式(c) for c in e.子), 変数=e.変数) if e.子 else e
    c = e.子[0]
    if c.種別 in ('原子', '帰属'): return e
    if c.種別 == '否定': return 整形式(c.子[0])
    if c.種別 in ('連言', '選言'):
        return 結合('選言' if c.種別 == '連言' else '連言', *(整形式(反対(x)) for x in c.子))
    if c.種別 == '含意':
        return 結合('連言', 整形式(c.子[0]), 整形式(反対(c.子[1])))
    if c.種別 in ('全称', '存在'):
        return 結合('存在' if c.種別 == '全称' else '全称', 整形式(反対(c.子[0])), 変数=c.変数)
    raise ValueError('否定作用域が未対応')


def _平坦(e):
    return e.種別 in ('原子', '否定', '帰属') or e.種別 in ('連言', '選言') and all(_平坦(c) for c in e.子)


class 命題推論器:
    def __init__(self, 記載: tuple[命題記載, ...], *, 上限: 推論上限 | None = None):
        self.上限 = (上限 or 推論上限()).検査()
        if (type(記載) is not tuple or not 1 <= len(記載) <= 256
                or any(type(r) is not 命題記載 for r in 記載)
                or len({r.識別子 for r in 記載}) != len(記載)):
            raise ValueError('命題記載の型・重複・上限')
        for r in 記載: r.式.検査()
        self.記載 = 記載
        self._操作 = 0
        self._根拠 = {}
        self._種 = []
        self._全称 = []
        self._定数 = set()
        for row in 記載:
            e = 整形式(row.式)
            pid = self._証(e, '資料記載', (), row.識別子)
            self._展開記載(e, pid, row.識別子)
            for a in 原子群(e):
                self._定数.update(t for t in a.項 if t.種別 == '定数')
        self._既定項 = self._定数 | {t for e, _ in self._種 for a in 原子群(e)
                                          for t in a.項 if t.種別 == '存在証人'}
        self._初期根拠 = deepcopy(self._根拠)
        if len(self._既定項) > self.上限.項数:
            raise ValueError('推論項数上限')

    def _刻み(self):
        self._操作 += 1
        if self._操作 > self.上限.操作数:
            raise ValueError('命題推論の操作予算超過。途中結果を採用しない')

    def _証(self, e, kind, parents=(), 情報源='', 範囲=''):
        record = {'式': e.辞書(), '作用': kind, '親': tuple(parents), '出典': 情報源, '仮定範囲': 範囲}
        key = 意味指紋(record)
        if key not in self._根拠:
            if len(self._根拠) >= self.上限.根拠数:
                raise ValueError('導出根拠数上限')
            self._根拠[key] = record
        return key

    def _展開記載(self, e, pid, seed):
        if e.種別 == '連言':
            for i, c in enumerate(e.子):
                self._展開記載(c, self._証(c, '連言分解', (pid,)), seed + ':' + str(i))
        elif e.種別 == '存在':
            if not _平坦(e.子[0]):
                raise ValueError('存在記載は原子・否定・連言・選言に限定する')
            witness = 命題項('証人:' + seed, '存在証人')
            c = 置換(e.子[0], {e.変数: witness})
            self._展開記載(c, self._証(c, '存在の局所証人', (pid,)), seed + ':存在')
        elif e.種別 == '全称':
            variables = []; body = e
            while body.種別 == '全称':
                variables.append(body.変数); body = body.子[0]
            if len(variables) > 2:
                raise ValueError('全称記載の同時変数は2個まで')
            if not (_平坦(body) or body.種別 == '含意' and all(_平坦(x) for x in body.子)):
                raise ValueError('全称記載内の量化・高階条件は未対応')
            self._全称.append((tuple(variables), body, pid))
        elif _平坦(e) or e.種別 == '含意' and all(_平坦(x) for x in e.子):
            self._種.append((e, pid))
        else:
            raise ValueError('記載の条件・量化の構成は未対応')

    def _支持(self, e, facts):
        """閉包中の式を評価する。ここで仮定や未知知識を追加しない。"""
        self._刻み()
        key = e.鍵()
        if key in facts: return facts[key][1]
        if e.種別 == '連言':
            parents = tuple(self._支持(c, facts) for c in e.子)
            if all(parents): return self._証(e, '連言構成', parents)
        elif e.種別 == '選言':
            for c in e.子:
                proof = self._支持(c, facts)
                if proof: return self._証(e, '選言導入', (proof,))
        return None

    def _閉包(self, domain, assumptions):
        if len(domain) > self.上限.項数:
            raise ValueError('具体化する項数上限')
        facts = {}; rules = []; changed = False

        def add(e, pid):
            nonlocal changed
            self._刻み()
            key = e.鍵()
            if key in facts: return
            if len(facts) >= self.上限.事実数:
                raise ValueError('命題事実数上限')
            facts[key] = (e, pid); changed = True
            if e.種別 == '連言':
                for c in e.子: add(c, self._証(c, '連言分解', (pid,)))
            elif e.種別 == '含意': rules.append((e, pid))

        for e, pid in self._種: add(e, pid)
        ordered = sorted(domain, key=lambda t: (t.種別, t.名前))
        for variables, body, pid in self._全称:
            for values in product(ordered, repeat=len(variables)):
                self._刻み()
                ground = 置換(body, dict(zip(variables, values)))
                add(ground, self._証(ground, '全称具体化', (pid,)))
        for e, pid in assumptions: add(e, pid)
        while changed:
            changed = False
            for rule, pid in tuple(rules):
                antecedent, consequent = rule.子
                p = self._支持(antecedent, facts)
                if p: add(consequent, self._証(consequent, '条件適用', (pid, p)))
        return facts

    def _証明(self, e, domain, assumptions=(), depth=0, 分岐済=frozenset()):
        self._刻み()
        if depth > self.上限.仮定深さ:
            raise ValueError('問いの量化・仮定深さ上限')
        facts = self._閉包(domain, assumptions)
        direct = self._支持(e, facts)
        if direct: return direct
        if e.種別 == '連言':
            parents = tuple(self._証明(c, domain, assumptions, depth + 1, 分岐済) for c in e.子)
            if all(parents): return self._証(e, '連言構成', parents)
        elif e.種別 == '選言':
            for c in e.子:
                p = self._証明(c, domain, assumptions, depth + 1, 分岐済)
                if p: return self._証(e, '選言導入', (p,))
        elif e.種別 == '含意':
            left, right = e.子
            if not _平坦(left): raise ValueError('仮定導入の前件は量化のない命題')
            範囲 = 意味指紋({'問い': e.辞書(), '親仮定': [p for _, p in assumptions]})
            pid = self._証(left, '問いの仮定', 範囲=範囲)
            p = self._証明(right, domain, (*assumptions, (left, pid)), depth + 1, 分岐済)
            if p: return self._証(e, '仮定を閉じた条件導出', (pid, p), 範囲=範囲)
        elif e.種別 == '全称':
            fresh = 命題項('任意:' + str(depth) + ':' + e.鍵()[:16], '任意個体')
            body = 置換(e.子[0], {e.変数: fresh})
            p = self._証明(body, domain | {fresh}, assumptions, depth + 1, 分岐済)
            # 全称否定（¬A∨¬B）は「Aならば¬B」等の十分な導出がある場合に限り証明する。
            # 不在による否定ではなく、残した仮定に対する明示反証を要求する。
            if not p and body.種別 == '選言' and all(c.種別 == '否定' for c in body.子):
                for i, c in enumerate(body.子):
                    others = [反対(x) for j, x in enumerate(body.子) if i != j]
                    left = others[0] if len(others) == 1 else 結合('連言', *others)
                    p = self._証明(結合('含意', left, c), domain | {fresh}, assumptions, depth + 1, 分岐済)
                    if p: break
            if p: return self._証(e, '任意個体からの全称導出', (p,))
        elif e.種別 == '存在':
            for t in sorted(domain, key=lambda t: (t.種別, t.名前)):
                if t.種別 == '任意個体': continue
                body = 置換(e.子[0], {e.変数: t})
                p = self._証明(body, domain, assumptions, depth + 1, 分岐済)
                if p: return self._証(e, '存在証拠', (p,))
        # 選言の各場合で同じ問いが導けた場合だけ結論へ接続する。
        # 矛盾する場合も除外せず、仮定を他の分岐や次の問いへ漏らさない。
        for disjunction, 情報源 in tuple(facts.values()):
            if disjunction.種別 != '選言' or disjunction.鍵() in 分岐済: continue
            visited = 分岐済 | {disjunction.鍵()}
            if len(visited) > 8: raise ValueError('選言場合分けの上限')
            parents = [情報源]; complete = True
            for i, child in enumerate(disjunction.子):
                範囲 = 意味指紋({'選言': 情報源, '場合': i, '問い': e.辞書(),
                                  '仮定': [p for _, p in assumptions]})
                hypothesis = self._証(child, '場合の仮定', 範囲=範囲)
                proof = self._証明(e, domain, (*assumptions, (child, hypothesis)), depth + 1, visited)
                if not proof:
                    complete = False; break
                parents.extend((hypothesis, proof))
            if complete:
                return self._証(e, '全場合を閉じた選言除去', tuple(parents))
        return None

    def 判定(self, 問い: 命題式):
        if type(問い) is not 命題式: raise ValueError('命題の問い型不正')
        問い.検査()
        self._操作 = 0
        self._根拠 = deepcopy(self._初期根拠)
        domain = self._既定項 | {t for a in 原子群(問い) for t in a.項 if t.種別 == '定数'}
        if any(t.種別 in ('存在証人', '任意個体') for a in 原子群(問い) for t in a.項):
            raise ValueError('外部の問いに内部個体を指定しない')
        # 結果は問いごとに独立。仮定閉包は後続の問いや持続知識へ残さない。
        positive = self._証明(整形式(問い), domain)
        negative = self._証明(整形式(反対(問い)), domain)
        status = '矛盾' if positive and negative else '支持' if positive else '反証' if negative else '未確定'
        keep = set()
        def visit(pid):
            if not pid or pid in keep: return
            keep.add(pid)
            for parent in self._根拠[pid]['親']: visit(parent)
        visit(positive); visit(negative)
        nodes = {pid: self._根拠[pid] for pid in sorted(keep)}
        return {'判定': status, '問い': 問い.辞書(), '支持': positive, '反証': negative,
                '導出': nodes, '操作数': self._操作,
                '解釈境界': '提供記載からの有限導出。世界の真実性、因果関係、論理的完全性の認定ではない。'}

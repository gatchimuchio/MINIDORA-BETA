from __future__ import annotations

from dataclasses import dataclass
from collections import deque

from .言語構造 import 言語関係構造


能力作用則版 = 'v4-relational-binding-候補'
_問い専用関係 = frozenset({"命題適合", "説明適合", "問い適合", "同定", "数量同定"})

# K3の連続weight値を模写しない。観測できた作用順序だけを符号付き序数へ射影する。
_完全整合 = 2
_極性矛盾 = -2
_逆向関係 = -1
_弱整合 = 1


def _端点意味同一(a: frozenset[str], b: frozenset[str]) -> bool:
    """表層完全一致ではなく、意味語集合の局所対応で端点同一性を判定する。"""
    if not a or not b:
        return False
    if a == b:
        return True

    identity_a = frozenset(x for x in a if x.startswith("識別記号:"))
    identity_b = frozenset(x for x in b if x.startswith("識別記号:"))
    if identity_a != identity_b:
        return False
    # v1のnegative controlを維持する。記号・数式の明示anchorが異なる場合は同一視しない。
    def anchors(values: frozenset[str]) -> tuple[frozenset[str], frozenset[str]]:
        symbol = frozenset(
            item.split(":", 1)[1]
            for item in values
            if item.startswith(("sym:", "atom:"))
        )
        math = frozenset(item.split(":", 1)[1] for item in values if item.startswith("math:"))
        return symbol, math

    symbols_a, math_a = anchors(a)
    symbols_b, math_b = anchors(b)
    if symbols_a and symbols_b and symbols_a != symbols_b:
        return False
    if math_a and math_b and math_a != math_b:
        return False

    # 部分集合は実体同一性の根拠ではない。
    # 例:「sample」と「control sample」を無断で同一対象へ統合しない。
    # 言い換え・別名は、明示された参照対応として別途証明する必要がある。
    return a == b


def _述語対応(a: 言語関係構造, b: 言語関係構造) -> bool:
    """世界関係と問い関係を、保持済み述語identityを失わず接続する。"""
    if a.種別 in _問い専用関係:
        # 問い適合/同定等は世界Dataの関係種別と一致しないため、検索述語由来の意味を使う。
        grammatical = frozenset({"する", "し", "される", "して", "do", "does", "did"})
        left, right = a.述語 - grammatical, b.述語 - grammatical
        return bool(left and right and left.intersection(right))
    if a.種別 == b.種別 and a.種別 != "開放述語":
        return True
    # 既に異なる世界関係へ分類された二つを「する」「される」等の一致で潰さない。
    if a.種別 not in _問い専用関係 | {"開放述語"} and b.種別 not in _問い専用関係 | {"開放述語"}:
        return False
    grammatical = frozenset({"する", "し", "される", "して", "do", "does", "did"})
    left, right = a.述語 - grammatical, b.述語 - grammatical
    return bool(left and right and left.intersection(right))


def _範囲一致(a: 言語関係構造, b: 言語関係構造) -> bool:
    # 条件の記述順を区別しない。異なる条件の証拠を弱い支持へ変換しない。
    return frozenset(a.条件) == frozenset(b.条件)


# 同じ端点に対する既存の比較・等価関係。特定分野の公式・定数は持たない。
_比較成立域 = {
    "比較.大": frozenset({1}), "比較.小": frozenset({-1}),
    "比較.以上": frozenset({0, 1}), "比較.以下": frozenset({-1, 0}),
    "等価": frozenset({0}), "不同": frozenset({-1, 1, 2}),
}
# 2は比較不能。全順序を無断仮定し、not(x>y)をx<=yへ変換しない。
_比較全域 = frozenset({-1, 0, 1, 2})


def 関係寄与(target: 言語関係構造, 証拠: 言語関係構造) -> int:
    """同一条件で、証拠が対象関係を支持・反証・未確定のどれにするか照合する。"""
    if not _範囲一致(target, 証拠):
        return 0
    same = (_端点意味同一(target.始点, 証拠.始点)
            and _端点意味同一(target.終点, 証拠.終点))
    reverse = (_端点意味同一(target.始点, 証拠.終点)
               and _端点意味同一(target.終点, 証拠.始点))
    if not same and not reverse:
        return 0
    if target.種別 in _比較成立域 and 証拠.種別 in _比較成立域:
        wanted = _比較成立域[target.種別]
        observed = _比較成立域[証拠.種別]
        if not target.肯定:
            wanted = _比較全域 - wanted
        if not 証拠.肯定:
            observed = _比較全域 - observed
        if not same:
            observed = frozenset(2 if x == 2 else -x for x in observed)
        if observed.issubset(wanted):
            return _完全整合
        if observed.isdisjoint(wanted):
            return _極性矛盾
        return 0
    if not _述語対応(target, 証拠) or not same:
        # 「逆方向の因果がある」だけでは、順方向の因果を反証したことにならない。
        return 0
    return _完全整合 if target.肯定 == 証拠.肯定 else _極性矛盾


def _推移照合値(target, 証拠, *, 最大段数=8, 最大節点数=128):
    """明示された比較・等価関係だけを有界再結合する。因果へ推移律を一般化しない。"""
    if target.種別 not in _比較成立域 or not target.始点 or not target.終点:
        return ()
    関係図 = {}
    equals = {}
    for item in 証拠:
        if not _範囲一致(target, item) or item.種別 not in _比較成立域:
            continue
        if not item.始点 or not item.終点:
            continue
        domain = _比較成立域[item.種別]
        if not item.肯定:
            domain = _比較全域 - domain
        left, right = item.始点, item.終点
        if domain == frozenset({0}):
            for start, end in ((left, right), (right, left)):
                関係図.setdefault(start, set()).add((end, False))
                equals.setdefault(start, set()).add(end)
        elif domain in (frozenset({1}), frozenset({0, 1})):
            関係図.setdefault(left, set()).add((right, domain == frozenset({1})))
        elif domain in (frozenset({-1}), frozenset({-1, 0})):
            関係図.setdefault(right, set()).add((left, domain == frozenset({-1})))
    nodes = set(関係図)
    nodes.update(end for edges in 関係図.values() for end, _ in edges)
    if len(nodes) > 最大節点数:
        return ()

    def reaches(start, end, *, equality_only=False):
        queue = deque([(start, False, 0)])
        visited = {(start, False)}
        found = set()
        while queue:
            node, strict, depth = queue.popleft()
            if depth >= 最大段数:
                continue
            edges = ((x, False) for x in equals.get(node, ())) if equality_only else 関係図.get(node, ())
            for nxt, edge_strict in edges:
                has_strict = strict or edge_strict
                if nxt == end:
                    found.add(has_strict)
                状態 = (nxt, has_strict)
                if 状態 not in visited:
                    visited.add(状態)
                    queue.append((nxt, has_strict, depth + 1))
        return found

    left, right = target.始点, target.終点
    forward = reaches(left, right)
    backward = reaches(right, left)
    equal = bool(reaches(left, right, equality_only=True))
    strict_forward = True in forward
    strict_backward = True in backward
    kind = target.種別
    if kind == "比較.大":
        support, oppose = strict_forward, bool(backward)
    elif kind == "比較.小":
        support, oppose = strict_backward, bool(forward)
    elif kind == "比較.以上":
        support, oppose = bool(forward), strict_backward
    elif kind == "比較.以下":
        support, oppose = bool(backward), strict_forward
    elif kind == "等価":
        support, oppose = equal, strict_forward or strict_backward
    else:
        support, oppose = strict_forward or strict_backward, equal
    if not target.肯定:
        support, oppose = oppose, support
    return ((_完全整合,) if support else ()) + ((_極性矛盾,) if oppose else ())


def _関係照合値(target, 証拠):
    return tuple(value for item in 証拠 if (value := 関係寄与(target, item))) + _推移照合値(target, 証拠)


def 証拠状態矛盾あり(targets, 証拠) -> bool:
    """同一対象関係について支持と反証が併存することを、最大値選択で消さない。"""
    for target in targets:
        values = _関係照合値(target, 証拠)
        if any(v > 0 for v in values) and any(v < 0 for v in values):
            return True
    return False


def 証拠状態寄与群(targets, 証拠) -> tuple[int, ...]:
    """関係ごとに照合し、矛盾は中立差として残す。反復・重複は追加票にしない。"""
    values = []
    seen = set()
    証拠 = tuple(証拠)
    for target in targets:
        if target.署名 in seen:
            continue
        seen.add(target.署名)
        local = _関係照合値(target, 証拠)
        positive = any(v > 0 for v in local)
        negative = any(v < 0 for v in local)
        if positive and negative:
            values.append(0)
        elif local:
            values.append(max(local, key=abs))
    return tuple(values)


def 証拠状態合計寄与(
    targets: tuple[言語関係構造, ...],
    証拠: tuple[言語関係構造, ...],
) -> int:
    """正式v3模型向け。一参照状態に保持された独立関係差を再結合する。"""
    return sum(証拠状態寄与群(targets, 証拠))


def 証拠状態寄与(targets, 証拠) -> int:
    """旧互換入口も、同一対象への矛盾を肯定へ読み替えない。"""
    証拠 = tuple(証拠)
    if 証拠状態矛盾あり(targets, 証拠):
        return 0
    values = 証拠状態寄与群(targets, 証拠)
    return max(values, key=abs) if values else 0


@dataclass(frozen=True, slots=True)
class 能力保存則:
    """実LLM横断観測から射影した、部品非依存の構成再現作用。"""

    状態分離: bool = True
    意味同一性: bool = True
    寄与関門: bool = True
    状態検査点: bool = True
    再選択: bool = True
    再結合: bool = True
    有界反復: bool = True
    形成分離: bool = True
    終端遅延: bool = True
    未確定差共存: bool = True
    寄与確定分離: bool = True
    再作用閉包: bool = True


標準能力保存則 = 能力保存則()


__all__ = [
    "能力作用則版",
    "能力保存則",
    "標準能力保存則",
    "関係寄与",
    "証拠状態寄与群",
    "証拠状態合計寄与",
    "証拠状態寄与",
]


@dataclass(frozen=True, slots=True)
class 証拠照合状態:
    支持: int = 0
    反証: int = 0
    未観測: int = 0
    矛盾: int = 0

    @property
    def 完全支持(self) -> bool:
        return self.支持 > 0 and not (self.反証 or self.未観測 or self.矛盾)


def 証拠状態照合(targets, 証拠) -> 証拠照合状態:
    """連言の各関係を分別する。一部の支持だけで候補全体を支持済みにしない。"""
    positive = negative = 未知 = conflict = 0
    証拠 = tuple(証拠)
    seen = set()
    for target in targets:
        if target.署名 in seen:
            continue
        seen.add(target.署名)
        values = _関係照合値(target,証拠)
        yes, no = any(x>0 for x in values), any(x<0 for x in values)
        if yes and no: conflict += 1
        elif yes: positive += 1
        elif no: negative += 1
        else: 未知 += 1
    return 証拠照合状態(positive,negative,未知,conflict)

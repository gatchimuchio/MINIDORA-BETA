"""指示の座標と通常循環の入出力・条件・帰還を接続する。採否主体は増やさない。"""
from __future__ import annotations
from dataclasses import dataclass, replace
from .値 import 署名
from ..コア.指示関係 import 座標状態


@dataclass(frozen=True, slots=True)
class HDS条件観測:
    ID: str
    座標ID: str
    ノード: str
    判定: str
    観測署名: str
    理由: str


@dataclass(frozen=True, slots=True)
class HDS指示観測:
    指示署名: str
    状態署名: str
    状態版: int
    条件: tuple[HDS条件観測, ...]
    # 入力の座標状態は変更せず、時点付き観測を別記録として持つ。
    現在状態: tuple[tuple[str, str], ...]


@dataclass(frozen=True, slots=True)
class HDS指示帰還:
    指示: object
    観測: HDS指示観測
    # (利用先, 元ノード, 実際の内容)。採用以外では空。
    内容: tuple[tuple[str, str, object], ...] = ()
    保守待ち: tuple[str, ...] = ()


def _値(状態, ノード):
    from .目的保持 import 目的ノード有効
    種, _, ID = ノード.partition(":")
    if not 目的ノード有効(状態, ノード):
        return False, None
    if 種 not in ("残差", "認識座標", "観測要求"):
        # 新しい到達条件も、既存の依存有効性と非循環条件を迂回しない。
        from .状態更新 import 全依存, ノード有効
        from .依存 import 上流集合
        edges = 全依存(状態)
        related = {ノード} | set(上流集合({ノード}, edges))
        if any(not ノード有効(状態, n) for n in related):
            return False, None
        parents = {n: set() for n in related}
        children = {n: set() for n in related}
        for e in edges:
            if e.前提 in related and e.後続 in related:
                parents[e.後続].add(e.前提)
                children[e.前提].add(e.後続)
        queue = [n for n, ps in parents.items() if not ps]
        visited = set()
        while queue:
            n = queue.pop()
            if n in visited:
                continue
            visited.add(n)
            for child in children[n]:
                parents[child].discard(n)
                if not parents[child]:
                    queue.append(child)
        if visited != related:
            return False, None
    if 種 == "認識":
        return True, 状態.認識辞書()[ID].値
    if 種 == "成果":
        return True, dict(状態.成果)[ID]
    if 種 == "主体":
        return True, dict(状態.主体状態)[ID]
    if 種 in ("状態", "残差"):
        return True, True
    if 種 == "認識座標":
        return True, 状態.認識辞書()[ID]
    if 種 == "観測要求":
        return True, next(x for x in 状態.観測要求 if x.ID == ID)
    from .状態更新 import ノード値
    return True, ノード値(状態, ノード)[1]


def 条件を観測(状態, 条件):
    valid, value = _値(状態, 条件.ノード)
    coords = 状態.指示関係.座標辞書() if 状態.指示関係 else {}
    blocked = any(coords[k].状態 in (座標状態.矛盾, 座標状態.留保)
                  for k in (条件.座標ID, *条件.参照座標) if k in coords)
    if blocked:
        status, reason = "未確定", "評価が依拠する座標に矛盾または留保がある"
    elif not valid:
        status, reason = "未確定", "必要なノードが未成立または依存失効"
    elif 条件.判定 == "有効":
        status, reason = "成立", "実ノードと依存の有効性を確認"
    else:
        equal = 署名(value) == 署名(条件.期待値)
        ok = equal if 条件.判定 == "一致" else not equal
        status, reason = ("成立", "期待内容との型付き照合が成立") if ok else ("不成立", "期待内容と実際の内容が不一致")
    return HDS条件観測(条件.ID, 条件.座標ID, 条件.ノード, status,
        署名((条件.ノード, valid, value)), reason)


def 指示条件有効(状態, ID):
    指示 = 状態.指示関係
    if 指示 is None:
        return False
    x = next((x for x in 指示.条件 if x.ID == ID), None)
    return x is not None and 条件を観測(状態, x).判定 == "成立"


def 指示条件署名(状態, ID):
    指示 = 状態.指示関係
    x = next((x for x in 指示.条件 if x.ID == ID), None) if 指示 else None
    return 署名((指示.署名 if 指示 else None, x, 条件を観測(状態, x) if x else None))


def 指示を観測(状態):
    指示 = 状態.指示関係
    if 指示 is None:
        return None
    from .目的保持 import 目的ノード署名
    nodes = sorted({x.ノード for x in 指示.条件} | {x.ノード for x in 指示.帰還先})
    return HDS指示観測(指示.署名, 状態.状態署名, 状態.版,
        tuple(条件を観測(状態, x) for x in 指示.条件),
        tuple((x, 目的ノード署名(状態, x)) for x in nodes))


def 指示到達ノード(状態):
    指示 = 状態.指示関係
    if 指示 is None:
        return frozenset()
    return frozenset({"指示条件:" + x.ID for x in 指示.条件 if x.段階 in ("達成", "返却")}
                     | {x.ノード for x in 指示.帰還先})


def 指示閉包可能(状態):
    指示 = 状態.指示関係
    if 指示 is None:
        return True
    # 明示目的の機械接続がないものを空条件で採用しない。
    if 指示.未接続座標 or not any(x.段階 == "達成" for x in 指示.条件):
        return False
    from .目的保持 import 目的ノード有効
    return (all(条件を観測(状態, x).判定 == "成立" for x in 指示.条件
                if x.段階 in ("維持", "達成", "返却"))
            and all(_値(状態, x.ノード)[0] for x in 指示.帰還先))


def 指示開始を検査(状態, 主体, *, 開始条件=True):
    指示 = 状態.指示関係
    if 指示 is None:
        return ()
    if not any(x.段階 == "達成" for x in 指示.条件):
        return ("目的の到達状態と評価規則が実行へ未接続",)
    if 指示.未接続座標:
        return tuple("原入力の実行接続が不足:" + k for k in 指示.未接続座標)
    実検証 = {(x.ID, x.版) for x in 主体.最終検証器}
    if not set(指示.検証契約) <= 実検証:
        raise ValueError("指示が要求する検証器のID・版が未接続")
    # 保持した原入力と、実際に実行状態が持つ入力を照合する。
    入力 = dict(状態.成果).get("HDSコア入力")
    if 指示.原入力署名 and 入力 is not None:
        if 署名(入力) != 指示.原入力署名:
            raise ValueError("指示の原入力と実行するCore入力が異なる")
    if not 開始条件:
        return ()
    return tuple(x.ID + ":" + o.判定 for x in 指示.条件 if x.段階 == "開始"
                 if (o := 条件を観測(状態, x)).判定 != "成立")


def 指示維持を検査(状態):
    指示 = 状態.指示関係
    if 指示 is None:
        return ()
    return tuple(x.ID + ":" + o.判定 for x in 指示.条件 if x.段階 == "維持"
                 if (o := 条件を観測(状態, x)).判定 != "成立")


def 作用対応を取る(状態, 作用ID, 仕様=None):
    指示 = 状態.指示関係
    if 指示 is None:
        return None
    固定 = next((x for x in 指示.作用対応 if x.作用ID == 作用ID), None)
    供給 = getattr(仕様, "指示対応", None)
    if 固定 is not None and 供給 is not None and 固定 != 供給:
        raise ValueError("供給された作用対応が原指示の対応と競合")
    対応 = 固定 or 供給
    if 対応 is None:
        return None
    from ..コア.指示関係 import HDS作用対応
    if not isinstance(対応, HDS作用対応) or 対応.作用ID != 作用ID:
        raise ValueError("供給作用の指示対応型・IDが不正")
    coords = 指示.座標辞書()
    conds = {x.ID: x for x in 指示.条件}
    if any(k not in coords for k in (*対応.対象座標, 対応.作用座標)):
        raise ValueError("供給作用の参照座標が不存在")
    if any(k not in conds or conds[k].段階 != "適用" for k in 対応.条件ID):
        raise ValueError("供給作用の条件は既存の適用条件へ接続する")
    return 対応


def 作用指示を検査(状態, 作用ID, 仕様=None):
    if 状態.指示関係 is None:
        return ()
    対応 = 作用対応を取る(状態, 作用ID, 仕様)
    if 対応 is None:
        return ("対象・作用への対応が未構成",)
    coords = 状態.指示関係.座標辞書()
    unknown = tuple(k for k in (*対応.対象座標, 対応.作用座標)
                    if coords[k].内容 is None or coords[k].状態 in (座標状態.矛盾, 座標状態.留保, 座標状態.未観測))
    if unknown:
        return tuple("座標の適用が未確定:" + k for k in unknown)
    by_id = {x.ID: x for x in 状態.指示関係.条件}
    return tuple(k + ":" + o.判定 for k in 対応.条件ID
                 if (o := 条件を観測(状態, by_id[k])).判定 != "成立")


def 仕様を指示へ接続(状態, 仕様):
    """現在の実行契約へ必要条件と評価先を射影する。値の真偽は実行時に再確認。"""
    指示 = 状態.指示関係
    if 指示 is None:
        return 仕様
    対応 = 作用対応を取る(状態, 仕様.作用ID, 仕様)
    conds = {x.ID: x for x in 指示.条件}
    inputs = set(仕様.読取ノード)
    if 対応:
        inputs.update("指示条件:" + k for k in 対応.条件ID)
    outputs = set(仕様.生成ノード)
    for x in 指示.条件:
        if x.ノード in 仕様.出力ノード集合:
            outputs.add("指示条件:" + x.ID)
    return replace(仕様, 読取ノード=tuple(sorted(inputs)), 生成ノード=tuple(sorted(outputs)))


def 条件の不足元(状態, nodes):
    指示 = 状態.指示関係
    if 指示 is None:
        return frozenset()
    ids = {n.partition(":")[2] for n in nodes if n.startswith("指示条件:")}
    return frozenset(x.ノード for x in 指示.条件 if x.ID in ids)


def 指示帰還を構成(状態, *, 採用=False, 保守待ち=()):
    指示 = 状態.指示関係
    if 指示 is None:
        return None
    from copy import deepcopy
    rows = []
    if 採用:
        if not 指示閉包可能(状態):
            raise ValueError("未成立の指示から採用内容を返せない")
        for x in 指示.帰還先:
            valid, value = _値(状態, x.ノード)
            if not valid:
                raise ValueError("帰還内容が未成立")
            rows.append((x.利用先, x.ノード, deepcopy(value)))
    return HDS指示帰還(指示, 指示を観測(状態), tuple(rows), tuple(保守待ち))


def 帰還を読む(結果):
    """送達の再試行で推論を再実行しない。内容の検査と複製だけを行う。"""
    from copy import deepcopy
    from ..HDS実行主体 import HDS終端
    帰還 = 結果.指示帰還
    if 結果.終端 != HDS終端.採用 or 帰還 is None:
        raise ValueError("採用済みの指示帰還が必要")
    if not 結果.状態.閉包済み:
        raise ValueError("帰還に対応する全体状態が未閉包")
    expected = 指示帰還を構成(結果.状態, 採用=True, 保守待ち=帰還.保守待ち)
    if expected != 帰還:
        raise ValueError("帰還内容または対応する指示・状態が変更された")
    return deepcopy(帰還)


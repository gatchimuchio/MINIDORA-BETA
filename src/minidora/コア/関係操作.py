"""条件束縛と由来を壊さない有限な共通関係導出。専門法則は外から受け取る。"""
from __future__ import annotations
from .値 import 署名, 整数
from .意味操作 import 束縛を試す, 具体化する


def 前方閉包(種: tuple[object, ...], 規則群, *, 最大事実数: int = 128, 最大照合数: int | None = None):
    """規則の前提が全て明示成立した場合だけ結論を追加する。

    規則は `前提` と `結論` を持つ。後件肯定、未記載の否定化、専門規則の生成は行わない。
    """
    整数(最大事実数, "最大事実数", 1, 4096)
    if 最大照合数 is None:
        最大照合数 = 最大事実数 * 32
    整数(最大照合数, "最大照合数", 1, 200000)
    if not isinstance(種, tuple):
        raise TypeError("種はtuple")
    事実群 = {署名(x): x for x in 種}
    照合回数 = 0
    変化 = True
    while 変化:
        変化 = False
        for 規則 in tuple(規則群):
            前提群 = getattr(規則, "前提", None)
            結論 = getattr(規則, "結論", None)
            if not isinstance(前提群, tuple) or not 前提群 or 結論 is None:
                raise TypeError("関係規則は空でない前提tupleと結論が必要")
            束縛群 = [{}]
            for 前提 in 前提群:
                次束縛群 = {}
                for 束縛 in 束縛群:
                    for 事実 in tuple(事実群.values()):
                        照合回数 += 1
                        if 照合回数 > 最大照合数:
                            raise ValueError("関係導出の照合予算超過。未探索部分を切り捨てない")
                        新束縛 = 束縛を試す(前提, 事実, 束縛)
                        if 新束縛 is not None:
                            次束縛群[署名(新束縛)] = 新束縛
                束縛群 = list(次束縛群.values())
                if not 束縛群:
                    break
            for 束縛 in 束縛群:
                導出 = 具体化する(結論, 束縛)
                if 導出 is None:
                    continue
                導出署名 = 署名(導出)
                if 導出署名 not in 事実群:
                    事実群[導出署名] = 導出
                    変化 = True
                    if len(事実群) > 最大事実数:
                        raise ValueError("関係導出の容量超過。候補切り捨ては禁止")
    return tuple(事実群[k] for k in sorted(事実群))


def 条件付き前方閉包(種: tuple[object, ...], 規則群, 条件群=(), *, 真集合=(), 偽集合=(), 最大事実数: int = 128, 最大照合数: int | None = None):
    """明示条件が全て成立した場合だけ共通関係導出を実行する。未確定は保留する。"""
    from .条件操作 import 条件集合を判定, 条件状態
    判定 = 条件集合を判定(tuple(条件群), 真集合, 偽集合)
    if 判定.状態 != 条件状態.成立:
        return tuple(種), 判定
    return 前方閉包(種, 規則群, 最大事実数=最大事実数, 最大照合数=最大照合数), 判定

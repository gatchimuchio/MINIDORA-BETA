"""原文の不足表現と明示的な関係規則から観測・仮説を構成する。

有限な日本語構文と変数束縛による実装射影。未解釈文、未束縛変数、
未検証規則を事実で埋めない。規則は外部から与えられるデータである。
"""
from __future__ import annotations
from dataclasses import dataclass, replace
import re
from .値 import 文字, 署名, 不変値, 整数
from .認識 import HDS認識項目, HDS出典, 認識区分
from .仮説 import HDS仮説, HDS予測, HDS作業枝
from .観測 import HDS観測要求
from ..コア.意味操作 import 束縛を試す as _共通束縛, 具体化する as _共通具体化
from ..コア.関係操作 import 前方閉包 as _共通前方閉包
from ..コア.条件操作 import 条件項 as _共通条件項, 条件集合を判定 as _共通条件判定, 条件状態 as _共通条件状態


@dataclass(frozen=True, slots=True)
class HDS命題:
    対象: str
    関係: str
    値: object
    範囲: str = "未指定"
    時点: str = "未指定"

    def __post_init__(self):
        for n in ("対象", "関係", "範囲", "時点"):
            文字(getattr(self, n), n)
        不変値(self.値)


@dataclass(frozen=True, slots=True)
class HDS関係規則:
    ID: str
    前提: tuple[HDS命題, ...]
    結論: HDS命題
    出典: tuple[HDS出典, ...]
    版: str = "v1"
    排他群: str = ""

    def __post_init__(self):
        文字(self.ID); 文字(self.版)
        if not isinstance(self.排他群, str):
            raise TypeError("排他群は明示文字列。空は排他未指定")
        if not isinstance(self.前提, tuple) or not self.前提 or any(not isinstance(x, HDS命題) for x in self.前提):
            raise TypeError("空でない命題前提tupleが必要")
        if not isinstance(self.結論, HDS命題):
            raise TypeError("結論は命題型が必要")
        if not isinstance(self.出典, tuple) or not self.出典 or any(not isinstance(x, HDS出典) for x in self.出典):
            raise ValueError("関係規則の由来を明示すること")


@dataclass(frozen=True, slots=True)
class HDS不足抽出:
    原文: str
    対象: str
    関係: str
    開始: int
    終了: int


_不足 = re.compile(r"(?P<対象>[^\n。！？!?]{1,120}?)の(?P<関係>[^\n。！？!?]{1,120}?)(?:が|は)(?:不明|未観測|未確認|不足|分からない|わからない|判明していない)(?:です|である)?[。！？!?]?")
_確認 = re.compile(r"(?P<対象>[^\n。！？!?]{1,120}?)の(?P<関係>[^\n。！？!?]{1,120}?)(?:を)(?:確認|調査|調べて|教えて)(?:する|して|してください|ほしい)?[。！？!?]?")


def 不足を抽出(原文: str) -> HDS不足抽出 | None:
    if not isinstance(原文, str):
        raise TypeError("不足原文は文字列")
    if len(原文) > 256 or any(x in 原文 for x in ("ただし", "ではない", "ではなく", "かもしれ", "とされる", "という文")):
        return None
    stripped = 原文.strip()
    m = _不足.fullmatch(stripped) or _確認.fullmatch(stripped)
    if not m:
        return None
    a, b = m.group("対象").strip(), m.group("関係").strip()
    if not a or not b or "の" in a + b or any(x in a + b for x in ("、", ";", "；")):
        return None
    start = 原文.index(stripped)
    return HDS不足抽出(原文, a, b, start, start + len(stripped))


def _命題(x):
    return HDS命題(x.対象, x.関係, x.値, x.範囲, x.時点)


def _束縛(型: HDS命題, 値: HDS命題, 初期=None):
    return _共通束縛(型, 値, 初期)


def _具体化(x: HDS命題, 束):
    return _共通具体化(x, 束, 生成=HDS命題)


def _ID(x: HDS命題, 認識群):
    matches = [r.ID for r in 認識群 if (r.対象, r.関係, r.範囲, r.時点) == (x.対象, x.関係, x.範囲, x.時点)]
    if len(matches) > 1:
        matches = [k for k in matches if "/候補/" not in k and not k.startswith("枝/")]
    if len(matches) > 1:
        raise ValueError("同一意味座標に複数ID。無言で同一視しない")
    return matches[0] if matches else "命題:" + 署名((x.対象, x.関係, x.範囲, x.時点))[:24]


def _規則有効(r, 記憶):
    raw = 記憶.正本辞書()
    return all(e.資料ID in raw and raw[e.資料ID].版 == e.版 and raw[e.資料ID].内容署名 == e.内容署名 for e in r.出典)


def _前方予測(種: tuple[HDS命題, ...], 規則群, 上限: int):
    return _共通前方閉包(種, 規則群, 最大事実数=上限, 最大照合数=上限 * 32)


def 関係から仮説を構成(状態, 規則群, *, 最大件数=128):
    from .状態更新 import 有効認識
    整数(最大件数, "仮説構成上限", 1)
    規則 = tuple(r for r in 規則群 if _規則有効(r, 状態.記憶))
    確定 = tuple(x for x in 状態.認識 if 有効認識(状態, x.ID) and not x.ID.startswith("枝/"))
    候補, 未定義, 要求 = {}, {}, {}
    for observed in 確定:
        for rule in 規則:
            env = _束縛(rule.結論, _命題(observed))
            if env is None:
                continue
            antecedents = tuple(_具体化(p, env) for p in rule.前提)
            if any(x is None for x in antecedents):
                continue
            確定命題 = tuple(_命題(x) for x in 確定)
            missing = tuple(p for p in antecedents if not any(署名(p) == 署名(x) for x in 確定命題))
            if not missing:
                continue
            # 未観測の前提を「偽」と扱わない。反対値が同一意味座標で確定している場合だけ不成立。
            条件群 = tuple(_共通条件項("仮定:" + _ID(p, 状態.認識), p) for p in missing)
            偽条件命題 = tuple(p for p in missing if any(
                (x.対象, x.関係, x.範囲, x.時点) == (p.対象, p.関係, p.範囲, p.時点)
                and 署名(x.値) != 署名(p.値) for x in 確定命題))
            条件判定 = _共通条件判定(条件群, 確定命題, 偽条件命題)
            if 条件判定.状態 == _共通条件状態.不成立:
                continue
            if 条件判定.状態 != _共通条件状態.未確定:
                raise ValueError("不足前提の条件状態が未確定以外になった")
            world = _前方予測(確定命題 + missing, 規則, 最大件数)
            preds = {}
            conflict = set()
            for p in world:
                k = _ID(p, 状態.認識)
                if k in preds and 署名(preds[k].値) != 署名(p.値):
                    conflict.add(k)
                else:
                    preds[k] = HDS予測(k, p.値)
                if k not in 状態.認識辞書():
                    未定義[k] = HDS認識項目(k, p.対象, p.関係, 範囲=p.範囲, 時点=p.時点)
                    要求[k] = HDS観測要求(k, p.対象, p.関係, ("仮説を分別する未観測の前提・予測",), p.範囲, p.時点, (p.対象, p.関係))
            hid = "構成仮説:" + 署名((rule.ID, rule.版, env, observed.ID))[:24]
            前提ID = tuple(_ID(p, 状態.認識) for p in missing)
            conditions = tuple("仮定:" + k + "=" + str(p.値) for k, p in zip(前提ID, missing))
            h = HDS仮説(hid, f"規則{rule.ID}の前提候補（観測{observed.ID}の説明仮説）",
                       tuple(preds[k] for k in sorted(preds) if k not in conflict), conditions,
                       tuple(x.ID for x in 確定), 認識区分.競合 if conflict else 認識区分.条件付き,
                       排他群=(rule.排他群 + ":" + observed.ID) if rule.排他群 else "")
            h = h.再照合(状態.認識)
            if conflict:
                h = replace(h, 区分=認識区分.競合)
            候補[hid] = h
            if len(候補) > 最大件数 or len(未定義) > 最大件数:
                raise ValueError("仮説/認識構成上限超過。全候補を保留する")
    return tuple(候補[k] for k in sorted(候補)), tuple(未定義[k] for k in sorted(未定義)), tuple(要求[k] for k in sorted(要求))


def 仮説から枝を構成(状態):
    結果群 = []
    認識 = 状態.認識辞書()
    for h in sorted(状態.仮説, key=lambda x: x.ID):
        rows = []
        for p in h.予測:
            base = 認識.get(p.観測ID)
            if base is None:
                continue
            条件 = tuple(sorted(set(h.条件) | {"仮説:" + h.ID}))
            rows.append(replace(base, 値=p.値, 区分=認識区分.失効 if h.区分 in (認識区分.失効, 認識区分.棄却) else 認識区分.条件付き,
                                条件=条件, 検証契約="", 依存=tuple(sorted(set(base.依存) - {base.ID}))))
        結果群.append(HDS作業枝("自動:" + h.ID, ("仮説:" + h.ID,), tuple(rows)))
    return tuple(結果群)


class 不足意味構成作用:
    作用ID = "内的/不足意味構成"

    def _生成(self, 状態):
        認識, 要求 = {}, {}
        for 残差名 in sorted(状態.残差):
            extracted = 不足を抽出(残差名)
            if extracted is None:
                continue
            p = HDS命題(extracted.対象, extracted.関係, None)
            ID = _ID(p, 状態.認識)
            if ID not in 状態.認識辞書():
                認識[ID] = HDS認識項目(ID, p.対象, p.関係)
            existing = next((x for x in 状態.観測要求 if x.ID == ID), None)
            needed = tuple(sorted(set(existing.解消残差 if existing else ()) | {残差名}))
            要求[ID] = HDS観測要求(ID, p.対象, p.関係, ("原文範囲:" + str(extracted.開始) + ":" + str(extracted.終了),),
                                  語群=(p.対象, p.関係), 解消残差=needed)
        return tuple(認識.values()), tuple(要求.values())

    def 機会(self, 状態):
        from ..HDS実行主体 import HDS作用機会
        new, requests = self._生成(状態)
        existing = {x.ID: x for x in 状態.観測要求}
        if not new and all(existing.get(x.ID) == x for x in requests):
            return None
        目的依存 = tuple(sorted({"残差:" + r for q in requests for r in q.解消残差}))
        return HDS作用機会(self.作用ID, 署名((new, requests)), 優先度=3, 種別="意味構成", 目的依存=目的依存)

    def 実行(self, 状態):
        from ..HDS実行主体 import HDS作用結果, HDS作用状態
        new, requests = self._生成(状態)
        return HDS作用結果(HDS作用状態.成立, 認識更新=new, 観測要求追加=requests,
                          理由=("原文中の対象・関係を観測要求へ射影。値と真偽は補完しない",))


class 関係仮説構成作用:
    作用ID = "内的/関係仮説構成"
    def __init__(self, 規則群, 最大件数):
        self.規則群, self.最大件数 = tuple(規則群), 最大件数
        self._生成署名 = None
        self._生成結果 = None
        self.生成実行数 = 0
        self.生成再利用数 = 0

    def _生成(self, 状態):
        # 機会取得と実行は同じ作用インスタンスを使う。状態や規則が変われば
        # 必ず再構成し、同じ入力での二重生成だけを省く。例外は保存しない。
        入力 = 署名((self.規則群, self.最大件数, 状態.状態署名))
        if self._生成署名 == 入力:
            self.生成再利用数 += 1
            return self._生成結果
        結果 = 関係から仮説を構成(状態, self.規則群, 最大件数=self.最大件数)
        self._生成署名, self._生成結果 = 入力, 結果
        self.生成実行数 += 1
        return 結果

    def 機会(self, 状態):
        from ..HDS実行主体 import HDS作用機会
        hs, rs, qs = self._生成(状態)
        old = {x.ID: x for x in 状態.仮説}
        if not rs and all(old.get(x.ID) == x for x in hs):
            return None
        # 座標が登録済みでも、後続の仮説・予測が使う意味座標との接続は残る。
        # 新規差分0件を目的との関係0件へ読み替えない。
        目的依存 = tuple(sorted(
            {"認識:" + x.ID for x in rs} | {"認識:" + q.ID for q in qs}
            | {"認識:" + p.観測ID for h in hs for p in h.予測}
        ))
        return HDS作用機会(self.作用ID, 署名((self.規則群, hs, rs, qs)), 優先度=2, 種別="仮説形成", 目的依存=目的依存)

    def 実行(self, 状態):
        from ..HDS実行主体 import HDS作用結果, HDS作用状態
        hs, rs, qs = self._生成(状態)
        if rs:
            return HDS作用結果(HDS作用状態.成立, 認識更新=rs, 観測要求追加=qs, 理由=("前提・下流予測の観測座標を構成",))
        nodes = sorted({"認識:" + k for h in hs for k in (*h.依存, *(p.観測ID for p in h.予測))})
        from .依存 import HDS依存辺
        edges = tuple(sorted({HDS依存辺("資料:" + e.資料ID, "仮説:" + h.ID) for r in self.規則群 for e in r.出典 for h in hs}))
        nodes = sorted(set(nodes) | {e.前提 for e in edges})
        return HDS作用結果(HDS作用状態.成立, 仮説更新=hs, 依存追加=edges,
                          検証依存=tuple((n, 状態.ノード署名(n)) for n in nodes), 理由=("関係規則を逆照合し仮定世界の下流予測を生成。事実採用ではない",))


class 自動分岐作用:
    作用ID = "内的/仮説分岐"
    def 機会(self, 状態):
        from ..HDS実行主体 import HDS作用機会
        rows = 仮説から枝を構成(状態)
        existing = {x.ID: x for x in 状態.枝}
        if all(existing.get(x.ID) == x for x in rows):
            return None
        目的依存 = tuple(sorted({"認識:" + x.ID for row in rows for x in row.認識}))
        return HDS作用機会(self.作用ID, 署名(rows), 種別="枝生成", 目的依存=目的依存)

    def 実行(self, 状態):
        from ..HDS実行主体 import HDS作用結果, HDS作用状態
        from .依存 import HDS依存辺
        rows = 仮説から枝を構成(状態)
        edges = tuple(HDS依存辺("仮説:" + h.ID, "枝:自動:" + h.ID) for h in 状態.仮説)
        return HDS作用結果(HDS作用状態.成立, 枝更新=rows, 依存追加=edges,
                          検証依存=tuple((e.前提, 状態.ノード署名(e.前提)) for e in edges),
                          理由=("仮説ごとの条件付き作業枝を自動生成・更新",))

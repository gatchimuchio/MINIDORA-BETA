from __future__ import annotations
from copy import deepcopy
from dataclasses import replace
from .値 import 署名
from .認識 import HDS認識項目, 認識区分
from .依存 import HDS依存辺
from .観測 import HDS観測要求, HDS観測値, HDS観測器, 観測範囲が一致
from .仮説 import HDS仮説雛型, 枝を合流
from .検証 import HDS検証器, HDS検証票
from .政策 import HDS阻害, 停止理由
from .状態更新 import 有効認識, ノード署名


class 観測作用:
    def __init__(self, 要求: HDS観測要求, 器: HDS観測器, 再利用回数: int = 3):
        self.要求, self.器 = 要求, 器
        self.再利用回数 = 再利用回数
        self.作用ID = f"内的/観測/{器.ID}/{要求.ID}"

    def 機会(self, 状態):
        from ..HDS実行主体 import HDS作用機会
        if self.器.手段 not in self.要求.手段 or 有効認識(状態, self.要求.ID):
            return None
        入力 = (self.要求, self.器, tuple((k, 状態.ノード署名("認識:" + k)) for k in self.要求.依存), 状態.記憶.正本署名)
        return HDS作用機会(self.作用ID, 署名(入力), 解消対象=frozenset(self.要求.解消残差),
                            資源負荷=self.器.資源負荷, 根拠=self.要求.必要性,
                            読取認識=self.要求.依存, 必要権限=self.器.必要権限,
                            識別対象=(self.要求.ID,), 種別="観測", 契約版=self.器.版 + "/" + self.器.検証版)

    def 実行(self, 状態):
        from ..HDS実行主体 import HDS作用結果, HDS作用状態
        証拠署名 = 署名(tuple((k, 状態.ノード署名("認識:" + k)) for k in self.要求.依存))
        計画 = 状態.記憶.計画
        if 計画 is None or not 計画.再利用可能(self.要求.問合せ, 状態.記憶, 証拠署名):
            計画 = 状態.記憶.計画する(self.要求.問合せ, self.要求.語群, 証拠署名, self.再利用回数)
        _, 消費後 = 計画.消費(self.要求.問合せ, 状態.記憶, 証拠署名)
        作業記憶 = replace(状態.記憶, 計画=消費後)
        読取 = deepcopy(replace(状態, 記憶=作業記憶))
        前署名 = 読取.状態署名
        応答 = self.器.取得(self.要求, 読取)
        if 読取.状態署名 != 前署名:
            raise ValueError("観測器が入力状態を変更した")
        群 = (応答,) if isinstance(応答, HDS観測値) else 応答
        if not isinstance(群, tuple) or any(not isinstance(v, HDS観測値) for v in 群):
            raise TypeError("観測器はHDS観測値またはそのtupleを返す必要がある")
        if len(群) > 256:
            raise ValueError("一観測の候補数上限を超えた")
        許容 = []
        原本 = {}
        for v in 群:
            if not 観測範囲が一致(self.要求, v):
                continue
            合格 = self.器.検証(self.要求, v)
            if type(合格) is not bool:
                raise TypeError("観測検証はboolを返す必要がある")
            if not 合格:
                continue
            for d in v.資料群:
                if d.ID in 原本 and 原本[d.ID] != d:
                    raise ValueError("一つの観測に同一資料IDの競合する版がある")
                原本[d.ID] = d
            許容.append(v)
        新記憶 = 作業記憶.更新(tuple(原本[k] for k in sorted(原本)))
        正本 = 新記憶.正本辞書()
        許容 = [v for v in 許容 if all(e.資料ID in 正本 and 正本[e.資料ID].版 == e.版 and 正本[e.資料ID].内容署名 == e.内容署名 for e in v.出典群)]
        if not 許容:
            return HDS作用結果(HDS作用状態.保留, 記憶更新=新記憶, 理由=("観測資料が不足または検証不成立",),
                                  阻害=HDS阻害(停止理由.観測不足, self.作用ID, "対象・範囲・時点・出典・検証を満たす観測がない", True))
        値別 = {}
        for v in 許容:
            値別.setdefault(署名(v.値), []).append(v)
        根拠 = tuple(sorted({e for v in 許容 for e in v.出典群}, key=署名))
        一意 = len(値別) == 1
        値 = 許容[0].値 if 一意 else tuple(値別[k][0].値 for k in sorted(値別))
        前 = 状態.認識辞書().get(self.要求.ID)
        主項目 = HDS認識項目(self.要求.ID, self.要求.対象, self.要求.関係, 値,
                           認識区分.確定 if 一意 else 認識区分.競合, 根拠=根拠,
                           依存=self.要求.依存, 範囲=self.要求.範囲, 時点=self.要求.時点,
                           検証契約=self.器.ID + "/" + self.器.検証版, 改訂=前.改訂 + 1 if 前 else 0)
        項目 = [主項目]
        if not 一意:
            for k, vs in sorted(値別.items()):
                項目.append(HDS認識項目(self.要求.ID + "/候補/" + k[:16], self.要求.対象, self.要求.関係,
                                      vs[0].値, 認識区分.暫定, 根拠=tuple(sorted({e for v in vs for e in v.出典群}, key=署名)),
                                      範囲=self.要求.範囲, 時点=self.要求.時点))
        return HDS作用結果(HDS作用状態.成立, 認識更新=tuple(項目), 記憶更新=新記憶,
                            解消残差=frozenset(self.要求.解消残差) if 一意 else frozenset(),
                            理由=("検証された同範囲の観測を受理" if 一意 else "競合値と出典を分離保持",),
                            阻害=None if 一意 else HDS阻害(停止理由.証拠競合, self.要求.ID, "検証を通った観測が一致しない", True))


class 仮説形成作用:
    def __init__(self, 雛型: HDS仮説雛型):
        self.雛型 = 雛型
        self.作用ID = "内的/仮説形成/" + 雛型.ID

    def _候補(self, 状態):
        if any(not 有効認識(状態, k) for k in self.雛型.必要認識):
            return ()
        return self.雛型.生成(状態.認識)

    def 機会(self, 状態):
        from ..HDS実行主体 import HDS作用機会
        候補 = self._候補(状態)
        現在 = {x.ID: x for x in 状態.仮説}
        if not 候補 or all(現在.get(x.ID) == x for x in 候補):
            return None
        任意 = tuple(sorted({p.観測ID for x in 候補 for p in x.予測} - set(self.雛型.必要認識)))
        目的依存 = tuple(sorted("認識:" + k for k in 任意))
        return HDS作用機会(self.作用ID, 署名((self.雛型, tuple((k, 状態.ノード署名("認識:" + k)) for k in (*self.雛型.必要認識, *任意)))),
                            読取認識=self.雛型.必要認識, 未確定読取=任意, 種別="仮説形成", 目的依存=目的依存)

    def 実行(self, 状態):
        from ..HDS実行主体 import HDS作用結果, HDS作用状態
        return HDS作用結果(HDS作用状態.成立, 仮説更新=self._候補(状態), 理由=("条件付き仮説を構成。事実採用ではない",))


class 仮説再照合作用:
    作用ID = "内的/仮説再照合"

    @staticmethod
    def _更新(状態):
        return tuple(y for x in 状態.仮説 if (y := x.再照合(状態.認識)) != x)

    def 機会(self, 状態):
        from ..HDS実行主体 import HDS作用機会
        更新 = self._更新(状態)
        if not 更新:
            return None
        ID群 = tuple(sorted({k for x in 更新 for k in (*x.依存, *(p.観測ID for p in x.予測))}))
        目的依存 = tuple(sorted("認識:" + k for k in ID群))
        return HDS作用機会(self.作用ID, 署名((tuple((k, 状態.ノード署名("認識:" + k)) for k in ID群), 更新)),
                            未確定読取=ID群, 種別="大域再照合", 優先度=1.0, 目的依存=目的依存)

    def 実行(self, 状態):
        from ..HDS実行主体 import HDS作用結果, HDS作用状態
        return HDS作用結果(HDS作用状態.成立, 仮説更新=self._更新(状態), 理由=("実観測で支持・反証・依存を再照合",))


class 枝合流作用:
    作用ID = "内的/作業枝合流"

    def 機会(self, 状態):
        from ..HDS実行主体 import HDS作用機会
        候補 = 枝を合流(状態.枝)
        現在 = 状態.認識辞書()
        if not 候補 or all(x.ID in 現在 and 現在[x.ID].意味署名 == x.意味署名 for x in 候補):
            return None
        目的依存 = tuple(sorted("認識:" + x.ID for x in 候補))
        return HDS作用機会(self.作用ID, 署名(状態.枝), 種別="枝合流", 目的依存=目的依存)

    def 実行(self, 状態):
        from ..HDS実行主体 import HDS作用結果, HDS作用状態
        組 = tuple(HDS依存辺("枝:" + b.ID, "認識:枝/" + b.ID + "/" + x.ID) for b in 状態.枝 for x in b.認識)
        return HDS作用結果(HDS作用状態.成立, 認識更新=枝を合流(状態.枝), 依存追加=組,
                            検証依存=tuple(("枝:" + b.ID, 状態.ノード署名("枝:" + b.ID)) for b in 状態.枝),
                            理由=("枝ごとの仮定・根拠を保存して合流",))


class 草案検証作用:
    def __init__(self, ID: str, 検証器群: tuple[HDS検証器, ...]):
        self.ID = ID
        self.検証器群 = {x.ID: x for x in 検証器群}
        self.作用ID = "内的/草案検証/" + ID

    def _草案(self, 状態):
        return next((x for x in 状態.草案 if x.ID == self.ID), None)

    def 機会(self, 状態):
        from ..HDS実行主体 import HDS作用機会
        草案 = self._草案(状態)
        if 草案 is None or 草案.区分 not in ("未検証", "失効"):
            return None
        # 旧前提の草案は作り直しを待つ。前提不一致が既知なのに検証を空回ししない。
        if any(状態.ノード署名(k) != sig for k, sig in 草案.依存署名):
            return None
        契約 = tuple((k, self.検証器群[k].版 if k in self.検証器群 else "未接続") for k in 草案.検証器ID)
        入力 = (草案.内容署名, 契約, tuple((k, 状態.ノード署名(k)) for k, _ in 草案.依存署名))
        return HDS作用機会(self.作用ID, 署名(入力), 出力状態=草案.追加状態, 解消対象=草案.解消残差,
                            種別="草案検証", 優先度=1.0)

    def 実行(self, 状態):
        from ..HDS実行主体 import HDS作用結果, HDS作用状態
        草案 = self._草案(状態)
        理由 = []
        契約 = []
        for k, s in 草案.依存署名:
            if 状態.ノード署名(k) != s:
                理由.append("草案の前提が変更された:" + k)
        for k in 草案.検証器ID:
            v = self.検証器群.get(k)
            if v is None:
                理由.append("検証器未接続:" + k)
                continue
            契約.append((v.ID, v.版))
            if 理由:
                continue
            状態写し, 草案写し = deepcopy(状態), deepcopy(草案)
            前署名, 草案前署名 = 状態写し.状態署名, 署名(草案写し)
            合格 = v.検証(状態写し, 草案写し)
            if 状態写し.状態署名 != 前署名 or 署名(草案写し) != 草案前署名:
                raise ValueError("検証器が対象状態または草案を変更した")
            if type(合格) is not bool:
                raise TypeError("草案検証器はboolを返す必要がある")
            if not 合格:
                理由.append("検証不成立:" + k)
        合格 = not 理由
        票 = HDS検証票(草案.ID, 草案.内容署名, tuple(契約), 合格, tuple(理由))
        if not 合格:
            return HDS作用結果(HDS作用状態.成立, 草案更新=(replace(草案, 区分="棄却"),), 検証票追加=(票,),
                                理由=tuple(理由), 阻害=HDS阻害(停止理由.検証不成立, 草案.ID, ";".join(理由), True))
        元ノード = "草案:" + 草案.ID
        辺 = tuple(HDS依存辺(元ノード, "成果:" + k) for k, _ in 草案.成果) + tuple(HDS依存辺(元ノード, "状態:" + k) for k in sorted(草案.追加状態))
        return HDS作用結果(HDS作用状態.成立, 成果=草案.成果, 追加状態=草案.追加状態,
                            解消残差=草案.解消残差, 草案更新=(replace(草案, 区分="検証済み"),), 検証票追加=(票,),
                            依存追加=辺, 検証依存=tuple(dict((*草案.依存署名, (元ノード, 状態.ノード署名(元ノード)))).items()),
                            理由=("指定検証契約に合格。目的全体の採否は通常循環で再評価",))

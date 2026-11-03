"""作用の入出力関係から目的経路と有限な作用列を構成する。

ここにあるのは操作契約の射影であり、生成予定を観測済みの事実にしない。
計画にない接続は未構成として残す。実行時の入力確認・結果検証は別に行う。
"""
from __future__ import annotations
from dataclasses import dataclass
import heapq
from .値 import 文字, 文字列組, 整数


_ノード種 = frozenset(("状態", "残差", "認識", "成果", "仮説", "枝", "草案",
                     "形成", "資料", "主体", "認識座標", "観測要求", "指示条件"))


def ノードを検査(ノード: str) -> str:
    文字(ノード, "目的接続ノード")
    種, 区切り, 名前 = ノード.partition(":")
    if not 区切り or 種 not in _ノード種 or not 名前.strip():
        raise ValueError("目的接続ノードは既知の種別と空でない名前で修飾する")
    return ノード


@dataclass(frozen=True, slots=True)
class HDS探索契約:
    """未構成の接続を調べる有限契約。取得予定は確定効果ではない。"""
    ID: str
    不明点: str
    取得ノード: tuple[str, ...]
    利用先: tuple[str, ...]
    最大試行: int = 2
    最大資源: int = 2
    版: str = "v1"
    再観測: bool = False

    def __post_init__(self):
        if type(self.再観測) is not bool:
            raise TypeError("再観測はbool")
        for 名 in ("ID", "不明点", "版"):
            文字(getattr(self, 名), 名)
        for 名 in ("取得ノード", "利用先"):
            群 = getattr(self, 名)
            文字列組(群, 名)
            if not 群:
                raise ValueError("探索の取得ノードと利用先は空にできない")
            for x in 群:
                ノードを検査(x)
        整数(self.最大試行, "探索最大試行", 1, 4096)
        整数(self.最大資源, "探索最大資源", 1, 1_000_000)
        if any(x.startswith(("状態:", "残差:")) for x in self.取得ノード):
            raise ValueError("探索は関係・成果・観測を取得する。達成状態を取得物と呼ばない")


@dataclass(frozen=True, slots=True)
class HDS作用仕様:
    作用ID: str
    入力状態: frozenset[str] = frozenset()
    追加状態: frozenset[str] = frozenset()
    削除状態: frozenset[str] = frozenset()
    解消残差: frozenset[str] = frozenset()
    追加残差: frozenset[str] = frozenset()
    読取認識: tuple[str, ...] = ()
    必要権限: tuple[str, ...] = ()
    資源負荷: int = 1
    版: str = "v1"
    純粋: bool = False
    # 成果は名前ではなく、実行状態で検証・失効管理される値の受渡し契約。
    読取成果: tuple[str, ...] = ()
    生成成果: tuple[str, ...] = ()
    # 仮説・関係束・変換等は既存状態の型付きノードで同じ契約へ接続する。
    読取ノード: tuple[str, ...] = ()
    生成ノード: tuple[str, ...] = ()
    目的依存: tuple[str, ...] = ()
    # Falseは未記述の可能性を残す。空の契約を無関係の証明にしない。
    契約完全: bool = False
    探索: HDS探索契約 | None = None
    # 供給時に具体化した対象・作用の対応。元の指示正本は変更しない。
    指示対応: object | None = None
    # 起動条件と成果の因果親を分離する。参照の更新を自己失効にしない。
    観測入力: tuple[str, ...] = ()
    # 観測は外界を読むが純粋計算ではない。自動再現では再実行しない。
    観測専用: bool = False

    def __post_init__(self):
        if self.指示対応 is not None:
            from ..コア.指示関係 import HDS作用対応
            if not isinstance(self.指示対応, HDS作用対応):
                raise TypeError("作用仕様の指示対応型が不正")
            if self.指示対応.作用ID != self.作用ID:
                raise ValueError("作用仕様と指示対応のIDが異なる")
        if type(self.純粋) is not bool or type(self.契約完全) is not bool:
            raise TypeError("純粋作用・契約完全宣言はbool")
        if self.探索 is not None and not isinstance(self.探索, HDS探索契約):
            raise TypeError("探索契約型が必要")
        if type(self.観測専用) is not bool:
            raise TypeError("観測専用はbool")
        if self.探索 is not None and not (self.純粋 or self.観測専用):
            raise ValueError("有限探索には純粋作用または観測専用契約が必要")
        文字列組(self.観測入力, "観測入力")
        for 項 in self.観測入力:
            ノードを検査(項)
        文字(self.作用ID); 文字(self.版)
        for 名 in ("入力状態", "追加状態", "削除状態", "解消残差", "追加残差"):
            群 = getattr(self, 名)
            if not isinstance(群, frozenset):
                raise TypeError(f"{名}はfrozenset")
            for 項 in 群:
                文字(項)
        if self.追加状態 & self.削除状態 or self.解消残差 & self.追加残差:
            raise ValueError("相反する作用効果")
        for 名 in ("読取認識", "必要権限", "読取成果", "生成成果", "読取ノード", "生成ノード", "目的依存"):
            文字列組(getattr(self, 名), 名)
        for 項 in (*self.読取ノード, *self.生成ノード, *self.目的依存):
            ノードを検査(項)
        if any(x.startswith(("状態:", "残差:")) for x in self.生成ノード):
            raise ValueError("状態・残差の生成は専用の効果欄へ記述する")
        整数(self.資源負荷, "計画資源負荷")

    @property
    def 入力ノード集合(self) -> frozenset[str]:
        return frozenset({"状態:" + x for x in self.入力状態}
                         | {"認識:" + x for x in self.読取認識}
                         | {"成果:" + x for x in self.読取成果}
                         | set(self.読取ノード) | set(self.観測入力))

    @property
    def 出力ノード集合(self) -> frozenset[str]:
        return frozenset({"状態:" + x for x in self.追加状態}
                         | {"残差:" + x for x in self.解消残差}
                         | {"成果:" + x for x in self.生成成果}
                         | set(self.生成ノード))


@dataclass(frozen=True, slots=True)
class HDS目的経路:
    根: frozenset[str]
    必要ノード: frozenset[str]
    仕様群: tuple[HDS作用仕様, ...]
    # (作用ID, 必要ノード, 関係種)。根へ至る取得済みの接続を保持する。
    接続: tuple[tuple[str, str, str], ...] = ()
    未構成作用: tuple[str, ...] = ()
    無関係作用: tuple[str, ...] = ()

    @property
    def 関連作用ID(self) -> frozenset[str]:
        return frozenset(x.作用ID for x in self.仕様群)

    def 利用先(self, ノード: str) -> tuple[str, ...]:
        return tuple(x.作用ID for x in self.仕様群 if ノード in x.入力ノード集合)


def 目的経路を構成(根, 仕様群, *, 明示依存=()) -> HDS目的経路:
    """生産・消費と前処理依存を同じ固定点へ反映する。

    中間ノードが増えるたびに明示依存も照合する。三項の個数を固定せず、
    有限に取得できた契約を再帰的に辿る。必要性と実行可否は混同しない。
    """
    根 = frozenset(根)
    for ノード in 根:
        ノードを検査(ノード)
    群 = tuple(sorted(仕様群, key=lambda x: x.作用ID))
    if any(not isinstance(x, HDS作用仕様) for x in 群):
        raise TypeError("HDS作用仕様が必要")
    文字列組(tuple(x.作用ID for x in 群), "目的経路作用ID")
    依存 = {k: frozenset(v) for k, v in 明示依存}
    for 値群 in 依存.values():
        for ノード in 値群:
            ノードを検査(ノード)
    必要 = set(根)
    関連 = {}
    変化 = True
    while 変化:
        変化 = False
        for 仕様 in 群:
            宣言 = set(仕様.目的依存) | set(依存.get(仕様.作用ID, ()))
            if 仕様.探索 is not None:
                宣言.update(仕様.探索.利用先)
            if 仕様.作用ID not in 関連 and (仕様.出力ノード集合 & 必要 or 宣言 & 必要):
                関連[仕様.作用ID] = 仕様
                必要.update(仕様.入力ノード集合)
                if 仕様.探索 is not None:
                    必要.update(仕様.探索.取得ノード)
                必要.update("残差:" + x for x in 仕様.追加残差)
                変化 = True
    接続 = []
    for 仕様 in 関連.values():
        接続.extend((仕様.作用ID, x, "生成") for x in 仕様.出力ノード集合 & 必要)
        接続.extend((仕様.作用ID, x, "消費") for x in 仕様.入力ノード集合)
        宣言 = set(仕様.目的依存) | set(依存.get(仕様.作用ID, ()))
        接続.extend((仕様.作用ID, x, "前処理") for x in 宣言 & 必要)
        if 仕様.探索 is not None:
            接続.extend((仕様.作用ID, x, "探索先") for x in 仕様.探索.利用先 if x in 必要)
            接続.extend((仕様.作用ID, x, "取得予定") for x in 仕様.探索.取得ノード)
    return HDS目的経路(
        根, frozenset(必要), tuple(関連[k] for k in sorted(関連)), tuple(sorted(set(接続))),
        tuple(x.作用ID for x in 群 if x.作用ID not in 関連 and not x.契約完全),
        tuple(x.作用ID for x in 群 if x.作用ID not in 関連 and x.契約完全),
    )


@dataclass(frozen=True, slots=True)
class HDS構成計画:
    作用列: tuple[str, ...]
    探索状態数: int
    打切り: bool = False
    予定資源: int = 0

    @property
    def 成立(self):
        return bool(self.作用列)


def 作用列を構成(成立状態, 残差, 要求状態, 仕様群, *, 最大深さ=12,
               最大状態数=2048, 最大資源=4096, 制約群=(),
               可用ノード=frozenset(), 要求ノード=frozenset()):
    """状態・残差だけでなく、成果や型付き中間結果の受渡しを計画する。

    生成宣言は計画上の候補であり、実行時に再検証する。未知の意味や手段を
    捏造せず、有限な契約の範囲で構成できなければ未構成を返す。
    """
    整数(最大深さ, "探索深さ", 1); 整数(最大状態数, "最大探索状態", 1)
    整数(最大資源, "最大探索資源")
    成立状態, 残差, 要求状態 = map(frozenset, (成立状態, 残差, 要求状態))
    可用ノード, 要求ノード = map(frozenset, (可用ノード, 要求ノード))
    for 項 in 可用ノード | 要求ノード:
        ノードを検査(項)
    根 = ({"状態:" + x for x in 要求状態} | {"残差:" + x for x in 残差} | set(要求ノード))
    経路 = 目的経路を構成(根, tuple(仕様群))
    仕様群 = 経路.仕様群
    # 状態・残差は状態集合を正本とし、可用値欄の古いラベルで上書きしない。
    可用ノード = frozenset(x for x in 可用ノード if not x.startswith(("状態:", "残差:")))
    初期 = (成立状態, 残差, 可用ノード)
    待ち = [(0, 0, (), tuple(sorted(成立状態)), tuple(sorted(残差)), tuple(sorted(可用ノード)))]
    最小費用 = {初期: (0, 0)}
    数 = 0
    打切り = False
    while 待ち and 数 < 最大状態数:
        費用, 深さ, 列, s, r, n = heapq.heappop(待ち)
        s, r, n = frozenset(s), frozenset(r), frozenset(n)
        if (費用, 深さ) > 最小費用.get((s, r, n), (float("inf"), float("inf"))):
            continue
        数 += 1
        有効 = n | {"状態:" + x for x in s}
        if 要求状態 <= s and not r and all(
                (x[3:] not in r) if x.startswith("残差:") else (x in 有効) for x in 要求ノード):
            return HDS構成計画(列, 数, False, 費用)
        if 深さ >= 最大深さ:
            打切り = True
            continue
        for 仕様 in 仕様群:
            if 仕様.探索 is not None:
                # 未構成経路の取得成功を確定的な計画効果として扱わない。
                continue
            # 残差ノードは「残差の解消」を表す。存在する残差を成立扱いしない。
            必要 = 仕様.入力ノード集合
            if any((x[3:] in r) if x.startswith("残差:") else (x not in 有効) for x in 必要):
                continue
            ns = (s - 仕様.削除状態) | 仕様.追加状態
            nr = (r - 仕様.解消残差) | 仕様.追加残差
            nn = frozenset(x for x in n | 仕様.出力ノード集合 if not x.startswith(("状態:", "残差:")))
            if any(c.違反(ns) for c in 制約群):
                continue
            if (ns, nr, nn) == (s, r, n):
                continue
            次費用 = 費用 + 仕様.資源負荷
            if 次費用 > 最大資源:
                打切り = True
                continue
            鍵 = (ns, nr, nn)
            評価 = (次費用, 深さ + 1)
            if 評価 >= 最小費用.get(鍵, (float("inf"), float("inf"))):
                continue
            if 鍵 not in 最小費用 and len(最小費用) >= 最大状態数:
                打切り = True
                continue
            最小費用[鍵] = 評価
            heapq.heappush(待ち, (次費用, 深さ + 1, 列 + (仕様.作用ID,),
                                 tuple(sorted(ns)), tuple(sorted(nr)), tuple(sorted(nn))))
    return HDS構成計画((), 数, bool(待ち) or 打切り)

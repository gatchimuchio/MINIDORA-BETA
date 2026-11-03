"""対象・目的・手段を原文、意味座標、実行への対応とともに保持する。

九座標は提供する一つの射影ひな型であり、世界の固定スキーマではない。
任意の追加座標と層間関係を保存する。分類自体を真偽・採否には使わない。
"""
from __future__ import annotations
from dataclasses import dataclass, replace
from enum import StrEnum
import json
from .値 import 文字, 文字列組, 不変値, 署名, 正規化


class 座標状態(StrEnum):
    確定値 = "確定値"
    推定値 = "推定値"
    未確定 = "未確定"
    未観測 = "未観測"
    矛盾 = "矛盾"
    留保 = "留保"


三組九座標 = (
    ("対象", "実体"), ("対象", "現在状態"), ("対象", "文脈・関係"),
    ("目的", "必要性"), ("目的", "到達状態"), ("目的", "評価規則"),
    ("手段", "作用"), ("手段", "境界"), ("手段", "検証・帰還"),
)


@dataclass(frozen=True, slots=True)
class HDS指示座標:
    ID: str
    組: str
    名称: str
    内容: object = None
    状態: 座標状態 = 座標状態.未観測
    由来: tuple[str, ...] = ()
    原文範囲: tuple[tuple[int, int], ...] = ()
    親: str = ""

    def __post_init__(self):
        for 名 in ("ID", "組", "名称"):
            文字(getattr(self, 名), 名)
        if not isinstance(self.状態, 座標状態):
            raise TypeError("座標状態型が必要")
        不変値(self.内容)
        文字列組(self.由来, "座標由来", 一意=False)
        if self.状態 in (座標状態.確定値, 座標状態.推定値) and not self.由来:
            raise ValueError("確定値・推定値には由来が必要")
        if not isinstance(self.親, str):
            raise TypeError("親座標IDは文字列")
        if not isinstance(self.原文範囲, tuple):
            raise TypeError("原文範囲はtuple")
        for 範囲 in self.原文範囲:
            if (not isinstance(範囲, tuple) or len(範囲) != 2
                    or any(type(x) is not int for x in 範囲)
                    or not 0 <= 範囲[0] <= 範囲[1]):
                raise ValueError("座標の原文範囲不正")


@dataclass(frozen=True, slots=True)
class HDS指示条件:
    """原文上の評価内容と機械照合を明示的に接続する。暗黙の評価関数は作らない。"""
    ID: str
    座標ID: str
    ノード: str
    判定: str = "有効"
    期待値: object = None
    段階: str = "達成"
    版: str = "v1"
    参照座標: tuple[str, ...] = ()

    def __post_init__(self):
        from ..統合駆動_v2.計画 import ノードを検査
        for 名 in ("ID", "座標ID", "版"):
            文字(getattr(self, 名), 名)
        ノードを検査(self.ノード)
        if self.ノード.startswith("指示条件:"):
            raise ValueError("条件は他条件の真偽ラベルでなく実際の状態へ接続する")
        if self.判定 not in ("有効", "一致", "不一致"):
            raise ValueError("未実装の評価規則")
        if self.段階 not in ("開始", "適用", "維持", "達成", "返却"):
            raise ValueError("条件段階不正")
        文字列組(self.参照座標, "条件参照座標")
        不変値(self.期待値)
        if self.判定 == "有効" and self.期待値 is not None:
            raise ValueError("有効判定に期待値を混在させない")


@dataclass(frozen=True, slots=True)
class HDS作用対応:
    """呼出側の信頼された作用定義と、作用対象・意味上の手段の対応。"""
    作用ID: str
    対象座標: tuple[str, ...]
    作用座標: str
    条件ID: tuple[str, ...] = ()

    def __post_init__(self):
        文字(self.作用ID, "作用ID")
        文字(self.作用座標, "作用座標")
        文字列組(self.対象座標, "対象座標")
        文字列組(self.条件ID, "作用条件")
        if not self.対象座標:
            raise ValueError("操作対象を明示すること")
        if self.作用ID.startswith("内的/"):
            raise ValueError("内部作用の偽装対応を入力から登録できない")


@dataclass(frozen=True, slots=True)
class HDS帰還先:
    座標ID: str
    ノード: str
    利用先: str

    def __post_init__(self):
        from ..統合駆動_v2.計画 import ノードを検査
        文字(self.座標ID, "帰還座標")
        文字(self.利用先, "帰還先")
        ノードを検査(self.ノード)
        if self.ノード.startswith(("指示条件:", "残差:", "認識座標:", "観測要求:")):
            raise ValueError("帰還する内容は実値のノードで指定する")


@dataclass(frozen=True, slots=True)
class HDS指示関係:
    原文: str
    認知世界ID: str
    座標: tuple[HDS指示座標, ...]
    # (始点座標, 終点座標, 関係の内容)。階層以外の循環は表現可能。
    関係: tuple[tuple[str, str, str], ...] = ()
    条件: tuple[HDS指示条件, ...] = ()
    作用対応: tuple[HDS作用対応, ...] = ()
    帰還先: tuple[HDS帰還先, ...] = ()
    検証契約: tuple[tuple[str, str], ...] = ()
    原入力署名: str = ""
    原入力正本: str = ""
    返却優先: bool = True
    版: str = "指示関係-v2"
    要接続座標: tuple[str, ...] = ()

    def __post_init__(self):
        文字(self.原文, "指示原文")
        文字(self.認知世界ID, "認知世界ID")
        文字(self.版, "指示射影版")
        for 名, 型, key in (("座標", HDS指示座標, "ID"), ("条件", HDS指示条件, "ID"),
                            ("作用対応", HDS作用対応, "作用ID")):
            群 = getattr(self, 名)
            if not isinstance(群, tuple) or any(not isinstance(x, 型) for x in 群):
                raise TypeError(名 + "の型不正")
            文字列組(tuple(getattr(x, key) for x in 群), 名)
        if not isinstance(self.帰還先, tuple) or any(not isinstance(x, HDS帰還先) for x in self.帰還先):
            raise TypeError("帰還先型不正")
        文字列組(tuple(x.利用先 for x in self.帰還先), "帰還利用先")
        座標 = self.座標辞書()
        条件 = {x.ID: x for x in self.条件}
        for x in self.座標:
            if x.親 and x.親 not in 座標:
                raise ValueError("親座標が不存在: " + x.親)
            if any(b > len(self.原文) for _, b in x.原文範囲):
                raise ValueError("原文範囲が原文を超える")
        # 固定深度ではなく有限入力の全親関係を検査する。
        完了 = set()
        for x in self.座標:
            現在, 訪問 = x.ID, set()
            while 現在 and 現在 not in 完了:
                if 現在 in 訪問:
                    raise ValueError("座標の親関係が循環している")
                訪問.add(現在)
                現在 = 座標[現在].親
            完了.update(訪問)
        if not isinstance(self.関係, tuple):
            raise TypeError("座標関係はtuple")
        for 行 in self.関係:
            if not isinstance(行, tuple) or len(行) != 3:
                raise TypeError("座標関係の型不正")
            a, b, 内容 = 行
            if a not in 座標 or b not in 座標:
                raise ValueError("関係の参照座標が不存在")
            文字(内容, "座標間関係")
        for x in (*self.条件, *self.帰還先):
            if x.座標ID not in 座標:
                raise ValueError("条件・帰還の参照座標が不存在")
        文字列組(self.要接続座標, "要接続座標")
        if any(k not in 座標 for k in self.要接続座標):
            raise ValueError("要接続座標が不存在")
        for x in self.条件:
            if any(k not in 座標 for k in x.参照座標):
                raise ValueError("条件の参照座標が不存在")
        for x in self.作用対応:
            if any(k not in 座標 for k in (*x.対象座標, x.作用座標)):
                raise ValueError("作用の参照座標が不存在")
            if any(k not in 条件 or 条件[k].段階 != "適用" for k in x.条件ID):
                raise ValueError("作用条件は適用段階の既存条件IDで指定する")
        if not isinstance(self.検証契約, tuple):
            raise TypeError("検証契約はtuple")
        for 行 in self.検証契約:
            if not isinstance(行, tuple) or len(行) != 2:
                raise TypeError("検証契約はID・版の組")
            文字列組(行, "検証契約", 一意=False)
        文字列組(tuple(x[0] for x in self.検証契約), "検証器ID")
        if type(self.返却優先) is not bool:
            raise TypeError("返却優先はbool")
        if not isinstance(self.原入力正本, str) or not isinstance(self.原入力署名, str):
            raise TypeError("入力正本・署名は文字列")
        if bool(self.原入力署名) != bool(self.原入力正本):
            raise ValueError("入力の正本と署名を同時に保持すること")
        if self.原入力正本:
            from hashlib import sha256
            if sha256(self.原入力正本.encode("utf-8")).hexdigest() != self.原入力署名:
                raise ValueError("入力正本と署名が不一致")

    @property
    def 未接続座標(self):
        接続 = {x.座標ID for x in (*self.条件, *self.帰還先)}
        接続.update(k for x in self.条件 for k in x.参照座標)
        接続.update(k for x in self.作用対応 for k in (*x.対象座標, x.作用座標))
        return tuple(k for k in self.要接続座標 if k not in 接続)

    def 座標辞書(self):
        return {x.ID: x for x in self.座標}

    @property
    def 署名(self):
        return 署名(self)


def 九座標を用意(値群=()):
    """未提供値は未観測として生存させる。空欄を確定値で埋めない。"""
    値群 = tuple(値群)
    文字列組(tuple(x.ID for x in 値群), "座標ID")
    by_id = {x.ID: x for x in 値群}
    結果 = []
    for 組, 名称 in 三組九座標:
        ID = 組 + "/" + 名称
        if ID in by_id:
            x = by_id.pop(ID)
            if (x.組, x.名称) != (組, 名称):
                raise ValueError("標準座標IDの意味を変更できない")
        else:
            x = HDS指示座標(ID, 組, 名称)
        結果.append(x)
    結果.extend(by_id.values())
    return tuple(結果)


def コア入力を指示へ射影(入力, *, 条件=(), 作用対応=(), 帰還先=(), 検証契約=()):
    """既存Compilerの入力束を保持する。新しい自然言語解析器は置かない。

    同じ束に含まれる未対応項・制約・関係・選択肢は正本と個別座標へ残す。
    分類と機械照合の対応は別であり、条件等は信頼された呼出側が指定する。
    """
    from ..HDSコア入力 import HDSコア入力束
    if not isinstance(入力, HDSコア入力束):
        raise TypeError("既存HDSコア入力束が必要")
    必須 = ("原文", "認知世界ID", "意味項目", "関係", "条件", "目的", "作用要求",
          "要求成果", "残差", "検証要求", "実行制約", "表現制約", "文脈引用")
    if any(not hasattr(入力, n) for n in 必須):
        raise TypeError("既存Core入力契約が不足")
    def text(x):
        return json.dumps(正規化(x), ensure_ascii=False, sort_keys=True, separators=(",", ":"), allow_nan=False)
    roots = list(九座標を用意())
    children = []
    links = []
    states = {x.value: x for x in 座標状態}
    states["確定"] = 座標状態.確定値
    def add(path, value, parent, state="未確定", span=None):
        coord = HDS指示座標(path, parent.split("/")[0], path.split("/")[-1], text(value),
            states.get(str(state), 座標状態.未確定), ("Core入力:" + path,),
            () if span is None else (tuple(span),), parent)
        children.append(coord)
        links.append((path, parent, "原入力の構成成分"))
    for x in 入力.意味項目:
        kind = str(x.種別)
        parent = ("対象/実体" if kind.startswith(("対象", "実体")) else
                  "対象/現在状態" if kind.startswith(("状態", "属性", "値")) else "対象/文脈・関係")
        add("意味/" + x.ID, x, parent, x.状態, x.原文範囲)
    for field, parent in (("関係", "対象/文脈・関係"), ("条件", "手段/境界"),
                          ("目的", "目的/到達状態"), ("作用要求", "手段/作用"),
                          ("残差", "対象/現在状態"), ("検証要求", "手段/検証・帰還"),
                          ("実行制約", "手段/境界")):
        for x in getattr(入力, field):
            # 必要性と到達状態を同じ目的文字列で自動充足させない。
            p = "目的/必要性" if field == "目的" and x.種別 == "必要性" else parent
            add(field + "/" + x.ID, x, p, getattr(x, "状態", "未確定"), getattr(x, "原文範囲", None))
    for field, parent in (("要求成果", "目的/到達状態"), ("表現制約", "手段/境界"),
                          ("文脈引用", "対象/文脈・関係")):
        add(field, getattr(入力, field), parent)
    for i, x in enumerate(roots):
        ids = tuple(c.ID for c in children if c.親 == x.ID)
        if ids:
            roots[i] = replace(x, 内容=ids, 状態=座標状態.未確定, 由来=("Core入力の参照集合",))
    # 原文の命令・制約を、保存しただけで実行へ接続済みと扱わない。
    required = [field + "/" + x.ID for field in ("目的", "条件", "作用要求", "実行制約", "検証要求")
                for x in getattr(入力, field)]
    if 入力.要求成果:
        required.append("要求成果")
    if 入力.表現制約.出力言語 is not None or 入力.表現制約.要求:
        required.append("表現制約")
    正本 = text(入力)
    from hashlib import sha256
    return HDS指示関係(入力.原文, 入力.認知世界ID, tuple(roots + children), tuple(links),
        tuple(条件), tuple(作用対応), tuple(帰還先), tuple(検証契約),
        sha256(正本.encode("utf-8")).hexdigest(), 正本, 要接続座標=tuple(required))


def 指示を接続(状態, 指示):
    """実行開始前の明示的な接続。途中の目的改定APIではない。"""
    from ..HDS実行主体 import HDS実行状態
    if not isinstance(状態, HDS実行状態) or not isinstance(指示, HDS指示関係):
        raise TypeError("実行状態と指示関係が必要")
    if 状態.版 != 0 or 状態.指示関係 is not None:
        raise ValueError("指示接続は未実行の状態に一度だけ行う")
    return replace(状態, 指示関係=指示)


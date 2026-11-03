"""目的正本、必要関係の充足、実際の進展を分けて保持する。"""
from __future__ import annotations
from dataclasses import dataclass
from .値 import 署名
from .計画 import ノードを検査


@dataclass(frozen=True, slots=True)
class HDS目的観測:
    契約署名: str
    未達状態: frozenset[str]
    未達認識: frozenset[str]
    残差: frozenset[str]
    閉包済み: bool
    必要ノード: frozenset[str] = frozenset()
    充足ノード: frozenset[str] = frozenset()

    @property
    def 直接尺度(self) -> tuple[int, int, int]:
        # 従来ログの互換表示。件数の大小だけで進展認定には用いない。
        return (0 if self.閉包済み else 1,
                len(self.未達状態) + len(self.未達認識), len(self.残差))

    @property
    def 未充足署名(self) -> str:
        return 署名((self.未達状態, self.未達認識, self.残差,
                  self.必要ノード - self.充足ノード))


def 目的契約署名(状態) -> str:
    # 他の作業状態や記憶を深複製する必要はない。
    主体 = dict(状態.主体状態)
    原契約 = (tuple(状態.目的), frozenset(状態.要求状態),
              frozenset(状態.要求認識), tuple(主体.get("HDS目的正本", ())))
    if 状態.指示関係 is None:
        # 旧構文化作用は実行途中でCore入力を形成する。既存入口の正当な
        # 入力形成を目的改変と誤認しない。九座標の新契約は明示接続後だけ有効。
        return 署名(原契約)
    return 署名((*原契約, 状態.指示関係, dict(状態.成果).get("HDSコア入力"),
               主体.get("HDSコア入力署名"), 主体.get("HDS入力案件")))


def 目的ノード有効(状態, ノード: str) -> bool:
    """座標の存在、仮説の存在、事実の確定を区別して照合する。"""
    from .状態更新 import ノード有効
    ノードを検査(ノード)
    種, _, ID = ノード.partition(":")
    if 種 == "指示条件":
        from .指示接続 import 指示条件有効
        return 指示条件有効(状態, ID)
    if 種 == "残差":
        return ID not in 状態.残差
    if 種 == "認識座標":
        return ID in 状態.認識辞書()
    if 種 == "観測要求":
        return any(x.ID == ID for x in 状態.観測要求)
    return ノード有効(状態, ノード)


def 目的ノード署名(状態, ノード: str) -> str:
    ノードを検査(ノード)
    種, _, ID = ノード.partition(":")
    if 種 == "指示条件":
        from .指示接続 import 指示条件署名
        return 指示条件署名(状態, ID)
    if 種 == "残差":
        return 署名((ノード, ID in 状態.残差))
    if 種 == "認識座標":
        return 署名((ノード, 状態.認識辞書().get(ID)))
    if 種 == "観測要求":
        return 署名((ノード, next((x for x in 状態.観測要求 if x.ID == ID), None)))
    return 状態.ノード署名(ノード)


def 目的を観測(状態, 認識有効判定, *, 契約署名=None, 必要ノード=()) -> HDS目的観測:
    現契約 = 目的契約署名(状態)
    if 契約署名 is not None and 現契約 != 契約署名:
        raise ValueError("実行中に目的契約を変更できない")
    from .指示接続 import 指示到達ノード
    必要 = frozenset(必要ノード) | 指示到達ノード(状態)
    return HDS目的観測(
        現契約, frozenset(状態.要求状態 - 状態.成立状態),
        frozenset(k for k in 状態.要求認識 if not 認識有効判定(状態, k)),
        frozenset(状態.残差), bool(状態.閉包済み), 必要,
        frozenset(x for x in 必要 if 目的ノード有効(状態, x)),
    )


def 目的未充足ノード(状態, 認識有効判定, *, 必要認識=(), 修復状態=(), 修復認識=(), 必要ノード=()):
    nodes = {"状態:" + str(x) for x in 状態.要求状態 if x not in 状態.成立状態}
    # 残差の所有契約が未導入の入力を、無関係と推測して消さない。
    nodes.update("残差:" + str(x) for x in 状態.残差)
    required = set(状態.要求認識) | set(必要認識) | set(修復認識)
    nodes.update("認識:" + str(x) for x in required if not 認識有効判定(状態, x))
    nodes.update("状態:" + str(x) for x in 修復状態 if x not in 状態.成立状態)
    nodes.update(x for x in 必要ノード if not 目的ノード有効(状態, x))
    return frozenset(nodes)


def 作用が目的経路に属する(状態, 機会, *, 認識有効判定, 関連作用ID=(),
                      必要認識=(), 修復状態=(), 修復認識=(), 計画仕様=None, 必要ノード=()):
    nodes = 目的未充足ノード(状態, 認識有効判定, 必要認識=必要認識,
        修復状態=修復状態, 修復認識=修復認識, 必要ノード=必要ノード)
    if not nodes:
        return False
    if 機会.作用ID in set(関連作用ID):
        return True
    if 計画仕様 is not None:
        if 計画仕様.出力ノード集合 & nodes:
            return True
    elif ({"状態:" + x for x in 機会.出力状態}
          | {"残差:" + x for x in 機会.解消対象}
          | set(getattr(機会, "生成ノード", ()))) & nodes:
        return True
    if {"認識:" + x for x in 機会.識別対象} & nodes:
        return True
    return bool(set(getattr(機会, "目的依存", ())) & nodes)


def 計画効果を実測(状態差, 計画仕様) -> bool:
    if 計画仕様 is None:
        return False
    return bool(set(状態差.追加状態) & set(計画仕様.追加状態)
                or set(状態差.解消残差) & set(計画仕様.解消残差)
                or set(getattr(状態差, "変更成果", ())) & set(計画仕様.生成成果))


def 進展ノードを取得(前観測, 後観測):
    if 前観測.契約署名 != 後観測.契約署名:
        raise ValueError("目的契約が途中で変化した")
    nodes = ({"状態:" + x for x in 前観測.未達状態 - 後観測.未達状態}
             | {"認識:" + x for x in 前観測.未達認識 - 後観測.未達認識}
             | {"残差:" + x for x in 前観測.残差 - 後観測.残差}
             | set((後観測.充足ノード - 前観測.充足ノード) & 前観測.必要ノード))
    if 後観測.閉包済み and not 前観測.閉包済み:
        nodes.add("目的:閉包")
    return frozenset(nodes)


def 目的進展を判定(*, 前観測, 後観測, 最良直接尺度, 計画長=0,
              計画仕様=None, 状態差=None, 計画最良残数=None, 進展台帳=None):
    """必要な関係の実際の成立を認定し、同じ充足の往復を二重計上しない。

    必要ノードは作用前に構成された経路のものを使う。作用が出力した後から
    好都合なノードを必要だったことにしない。予定効果やラベルだけでは進展しない。
    """
    新規 = set(進展ノードを取得(前観測, 後観測))
    if 進展台帳 is not None:
        新規.difference_update(進展台帳)
        進展台帳.update(新規)
    return bool(新規), min(最良直接尺度, 後観測.直接尺度)


__all__ = ["HDS目的観測", "目的契約署名", "目的を観測", "目的未充足ノード",
           "作用が目的経路に属する", "計画効果を実測", "目的進展を判定",
           "目的ノード有効", "目的ノード署名", "進展ノードを取得"]

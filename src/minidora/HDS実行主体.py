from __future__ import annotations

from dataclasses import dataclass, fields, is_dataclass, replace
from enum import StrEnum
from hashlib import sha256
import json
import math
from typing import Callable, Protocol, Sequence


from .統合駆動_v2.値 import 正規化 as _正規化v2, 署名 as _署名v2, 文字列組, 整数, 文字
from .統合駆動_v2.認識 import HDS認識項目, HDS認識差, 認識区分
from .統合駆動_v2.依存 import HDS依存辺
from .統合駆動_v2.記憶 import HDS記憶
from .統合駆動_v2.仮説 import HDS仮説, HDS作業枝, HDS仮説雛型
from .統合駆動_v2.観測 import HDS観測要求, HDS観測器
from .統合駆動_v2.検証 import HDS草案, HDS検証器, HDS検証票
from .統合駆動_v2.形成 import HDS形成関係
from .統合駆動_v2.政策 import HDS運用政策, HDS阻害, HDS計装, 停止理由
from .統合駆動_v2.計画 import HDS作用仕様, HDS目的経路, HDS探索契約, ノードを検査
from .統合駆動_v2.意味構成 import HDS関係規則
from .統合駆動_v2.未来 import HDS未来制約, HDS未来状態
from .統合駆動_v2.診断 import HDS失敗診断
from .統合駆動_v2.適応記憶 import HDS適応記憶
from .コア.効果 import 期待効果
from .コア.指示関係 import HDS指示関係

HDS実行主体版 = "HDS実行主体-v4"


# 旧Adapterの非公開importとの互換名。意味署名の方式はv2へ一括移行する。
_正規化 = _正規化v2
_署名 = _署名v2


def _一意型群(値, 型, 名称):
    if not isinstance(値, tuple) or any(not isinstance(x, 型) for x in 値):
        raise TypeError(f"{名称}の要素型が不正")
    文字列組(tuple(x.ID for x in 値), 名称)


def _文字集合を検査(名前: str, 値: frozenset[str]) -> None:
    if not isinstance(値, frozenset):
        raise TypeError(f"{名前}はfrozensetである必要がある")
    if any(not isinstance(要素, str) or not 要素.strip() for 要素 in 値):
        raise ValueError(f"{名前}は空でない文字列だけを持つ必要がある")


class HDS終端(StrEnum):
    実行中 = "RUNNING"
    採用 = "COMMIT"
    保留 = "SUSPEND"
    失敗 = "FAIL"


class HDS作用状態(StrEnum):
    成立 = "成立"
    保留 = "保留"
    失敗 = "失敗"
    非適用 = "非適用"


@dataclass(frozen=True, slots=True)
class HDS実行状態:
    """HDS駆動コアが所有する最小作業状態。

    K3、候補得点、GPQA、製品能力型には依存しない。既存部品は作用適合器を介して
    この状態へ成果・残差・成立状態を帰還させる。
    """

    目的: tuple[str, ...] = ()
    要求状態: frozenset[str] = frozenset()
    成立状態: frozenset[str] = frozenset()
    残差: frozenset[str] = frozenset()
    成果: tuple[tuple[str, object], ...] = ()
    主体状態: tuple[tuple[str, object], ...] = ()
    版: int = 0
    認識: tuple[HDS認識項目, ...] = ()
    要求認識: frozenset[str] = frozenset()
    依存: tuple[HDS依存辺, ...] = ()
    仮説: tuple[HDS仮説, ...] = ()
    観測要求: tuple[HDS観測要求, ...] = ()
    記憶: HDS記憶 = HDS記憶()
    枝: tuple[HDS作業枝, ...] = ()
    草案: tuple[HDS草案, ...] = ()
    形成関係: tuple[HDS形成関係, ...] = ()
    再評価待ち: frozenset[str] = frozenset()
    認識履歴: tuple[HDS認識項目, ...] = ()
    検証票: tuple[HDS検証票, ...] = ()
    指示関係: HDS指示関係 | None = None

    def __post_init__(self) -> None:
        入力指示 = dict(self.主体状態).get("HDS指示関係")
        if 入力指示 is not None:
            if not isinstance(入力指示, HDS指示関係):
                raise TypeError("入力準備の指示関係型が不正")
            if self.指示関係 is not None and self.指示関係 != 入力指示:
                raise ValueError("入力準備と実行状態の指示関係が競合")
            object.__setattr__(self, "指示関係", 入力指示)
        if self.指示関係 is not None and not isinstance(self.指示関係, HDS指示関係):
            raise TypeError("指示関係型が必要")
        if type(self.版) is not int or self.版 < 0:
            raise ValueError("HDS実行状態の版は0以上の整数である必要がある")
        if not isinstance(self.目的, tuple) or any(not isinstance(項目, str) or not 項目.strip() for 項目 in self.目的):
            raise ValueError("HDS実行状態の目的は空でない文字列tupleである必要がある")
        _文字集合を検査("要求状態", self.要求状態)
        _文字集合を検査("成立状態", self.成立状態)
        _文字集合を検査("残差", self.残差)
        if not isinstance(self.成果, tuple):
            raise TypeError("HDS実行状態の成果はtupleである必要がある")
        成果名 = [名前 for 名前, _ in self.成果]
        if any(not isinstance(名前, str) or not 名前.strip() for 名前 in 成果名):
            raise ValueError("HDS実行状態の成果名は空でない文字列である必要がある")
        if len(成果名) != len(set(成果名)):
            raise ValueError("HDS実行状態の成果名は一意である必要がある")
        if not isinstance(self.主体状態, tuple):
            raise TypeError("HDS実行状態の主体状態はtupleである必要がある")
        主体名群 = [名前 for 名前, _ in self.主体状態]
        if any(not isinstance(名前, str) or not 名前.strip() for 名前 in 主体名群):
            raise ValueError("HDS実行状態の主体状態名は空でない文字列である必要がある")
        if len(主体名群) != len(set(主体名群)):
            raise ValueError("HDS実行状態の主体状態名は一意である必要がある")
        _文字集合を検査("要求認識", self.要求認識)
        _文字集合を検査("再評価待ち", self.再評価待ち)
        for 名称, 型 in (("認識", HDS認識項目), ("仮説", HDS仮説), ("観測要求", HDS観測要求), ("枝", HDS作業枝), ("草案", HDS草案), ("形成関係", HDS形成関係)):
            _一意型群(getattr(self, 名称), 型, 名称)
        if not isinstance(self.依存, tuple) or any(not isinstance(x, HDS依存辺) for x in self.依存) or len(set(self.依存)) != len(self.依存):
            raise ValueError("依存辺の型または一意性が不正")
        if not isinstance(self.記憶, HDS記憶):
            raise TypeError("HDS記憶型が必要")
        if not isinstance(self.認識履歴, tuple) or any(not isinstance(x, HDS認識項目) for x in self.認識履歴):
            raise TypeError("認識履歴の型が不正")
        if not isinstance(self.検証票, tuple) or any(not isinstance(x, HDS検証票) for x in self.検証票):
            raise TypeError("検証票の型が不正")

    @property
    def 未達状態(self) -> frozenset[str]:
        return frozenset(self.要求状態.difference(self.成立状態))

    @property
    def 閉包済み(self) -> bool:
        from .コア.状態操作 import 閉包を確認
        return 閉包を確認(self)

    @property
    def 状態署名(self) -> str:
        return _署名((
            self.目的,
            tuple(sorted(self.要求状態)),
            tuple(sorted(self.成立状態)),
            tuple(sorted(self.残差)),
            self.成果,
            self.主体状態,
            tuple(sorted(((x.ID, x.意味署名) for x in self.認識))),
            self.要求認識,
            tuple(sorted(self.依存)),
            tuple(sorted(self.仮説, key=lambda x: x.ID)),
            tuple(sorted(self.観測要求, key=lambda x: x.ID)),
            self.記憶.正本署名,
            self.記憶.圧縮,
            self.枝,
            self.草案,
            self.形成関係,
            self.再評価待ち,
            self.指示関係,
        ))

    def 成果辞書(self) -> dict[str, object]:
        from copy import deepcopy
        return deepcopy(dict(self.成果))

    def 主体辞書(self) -> dict[str, object]:
        from copy import deepcopy
        return deepcopy(dict(self.主体状態))

    def 認識辞書(self) -> dict[str, HDS認識項目]:
        return {x.ID: x for x in self.認識}

    def ノード署名(self, ノード: str) -> str:
        from .コア.状態操作 import ノード意味署名
        return ノード意味署名(self, ノード)


@dataclass(frozen=True, slots=True)
class HDS状態差:
    前状態署名: str
    後状態署名: str
    追加状態: tuple[str, ...] = ()
    削除状態: tuple[str, ...] = ()
    解消残差: tuple[str, ...] = ()
    追加残差: tuple[str, ...] = ()
    変更成果: tuple[str, ...] = ()
    変更主体状態: tuple[str, ...] = ()
    認識差: tuple[HDS認識差, ...] = ()
    影響対象: tuple[str, ...] = ()
    失効対象: tuple[str, ...] = ()
    変更依存: tuple[HDS依存辺, ...] = ()
    変更資料: tuple[str, ...] = ()

    @property
    def 変化有無(self) -> bool:
        return self.前状態署名 != self.後状態署名


@dataclass(frozen=True, slots=True)
class HDS作用機会:
    作用ID: str
    作用入力署名: str
    入力状態: frozenset[str] = frozenset()
    出力状態: frozenset[str] = frozenset()
    解消対象: frozenset[str] = frozenset()
    資源負荷: int = 1
    優先度: float = 0.0
    状態変更可能: bool = True
    根拠: tuple[str, ...] = ()
    読取認識: tuple[str, ...] = ()
    読取成果: tuple[str, ...] = ()
    必要権限: tuple[str, ...] = ()
    識別対象: tuple[str, ...] = ()
    種別: str = "通常"
    契約版: str = "v1"
    未確定読取: tuple[str, ...] = ()
    作用定義ID: str = ""
    意味入力署名: str = ""
    期待: 期待効果 = 期待効果()
    # 現在目的の未充足ノードへの前処理依存。直接効果の偽装には使わない。
    目的依存: tuple[str, ...] = ()
    読取ノード: tuple[str, ...] = ()
    生成ノード: tuple[str, ...] = ()

    def __post_init__(self) -> None:
        if not isinstance(self.作用ID, str) or not self.作用ID.strip():
            raise ValueError("HDS作用IDは空にできない")
        if not isinstance(self.作用入力署名, str) or not self.作用入力署名.strip():
            raise ValueError("HDS作用入力署名は空にできない")
        _文字集合を検査("作用入力状態", self.入力状態)
        _文字集合を検査("作用出力状態", self.出力状態)
        _文字集合を検査("作用解消対象", self.解消対象)
        if type(self.資源負荷) is not int or self.資源負荷 < 0:
            raise ValueError("HDS作用の資源負荷は0以上の整数である必要がある")
        if type(self.優先度) not in (int, float) or not math.isfinite(float(self.優先度)):
            raise ValueError("HDS作用の優先度は有限数である必要がある")
        if type(self.状態変更可能) is not bool:
            raise TypeError("HDS作用の状態変更可能はboolである必要がある")
        if not isinstance(self.根拠, tuple) or any(not isinstance(項目, str) for 項目 in self.根拠):
            raise TypeError("HDS作用の根拠は文字列tupleである必要がある")
        for 名称 in ("読取認識", "読取成果", "必要権限", "識別対象", "未確定読取"):
            文字列組(getattr(self, 名称), 名称)
        if not self.契約版 or not self.種別:
            raise ValueError("作用契約版と種別は空にできない")
        if not self.作用定義ID:
            object.__setattr__(self, "作用定義ID", self.作用ID)
        if not self.意味入力署名:
            object.__setattr__(self, "意味入力署名", self.作用入力署名)
        文字(self.作用定義ID, "作用定義ID")
        文字(self.意味入力署名, "意味入力署名")
        if not isinstance(self.期待, 期待効果):
            raise TypeError("期待効果型が不正")
        文字列組(self.目的依存, "作用目的依存")
        for 名 in ("読取ノード", "生成ノード"):
            文字列組(getattr(self, 名), 名)
        for x in self.目的依存:
            try:
                ノードを検査(x)
            except ValueError as exc:
                raise ValueError("作用目的依存: " + str(exc)) from exc
        for x in (*self.読取ノード, *self.生成ノード):
            ノードを検査(x)

    @property
    def 計画出力状態(self) -> frozenset[str]:
        return self.出力状態 | self.期待.追加状態

    @property
    def 計画解消対象(self) -> frozenset[str]:
        return self.解消対象 | self.期待.解消残差


@dataclass(frozen=True, slots=True)
class HDS作用結果:
    状態: HDS作用状態
    追加状態: frozenset[str] = frozenset()
    削除状態: frozenset[str] = frozenset()
    解消残差: frozenset[str] = frozenset()
    追加残差: frozenset[str] = frozenset()
    成果: tuple[tuple[str, object], ...] = ()
    主体状態差分: tuple[tuple[str, object], ...] = ()
    理由: tuple[str, ...] = ()
    停止要求: bool = False
    認識更新: tuple[HDS認識項目, ...] = ()
    認識削除: frozenset[str] = frozenset()
    依存追加: tuple[HDS依存辺, ...] = ()
    依存削除: tuple[HDS依存辺, ...] = ()
    仮説更新: tuple[HDS仮説, ...] = ()
    観測要求追加: tuple[HDS観測要求, ...] = ()
    記憶更新: HDS記憶 | None = None
    枝更新: tuple[HDS作業枝, ...] = ()
    草案更新: tuple[HDS草案, ...] = ()
    形成更新: tuple[HDS形成関係, ...] = ()
    成果削除: frozenset[str] = frozenset()
    検証依存: tuple[tuple[str, str], ...] = ()
    検証票追加: tuple[HDS検証票, ...] = ()
    阻害: HDS阻害 | None = None
    診断: HDS失敗診断 | None = None

    def __post_init__(self) -> None:
        if not isinstance(self.状態, HDS作用状態):
            raise TypeError("HDS作用結果の状態型が不正")
        _文字集合を検査("追加状態", self.追加状態)
        _文字集合を検査("削除状態", self.削除状態)
        _文字集合を検査("解消残差", self.解消残差)
        _文字集合を検査("追加残差", self.追加残差)
        if self.追加状態.intersection(self.削除状態):
            raise ValueError("同じ成立状態を一作用で追加・削除できない")
        if self.解消残差.intersection(self.追加残差):
            raise ValueError("同じ残差を一作用で解消・再追加できない")
        if not isinstance(self.成果, tuple):
            raise TypeError("HDS作用結果の成果はtupleである必要がある")
        成果名 = [名前 for 名前, _ in self.成果]
        if any(not isinstance(名前, str) or not 名前.strip() for 名前 in 成果名):
            raise ValueError("HDS作用結果の成果名は空でない文字列である必要がある")
        if len(成果名) != len(set(成果名)):
            raise ValueError("HDS作用結果の成果名は一意である必要がある")
        if not isinstance(self.主体状態差分, tuple):
            raise TypeError("HDS作用結果の主体状態差分はtupleである必要がある")
        主体名群 = [名前 for 名前, _ in self.主体状態差分]
        if any(not isinstance(名前, str) or not 名前.strip() for 名前 in 主体名群):
            raise ValueError("HDS作用結果の主体状態名は空でない文字列である必要がある")
        if len(主体名群) != len(set(主体名群)):
            raise ValueError("HDS作用結果の主体状態名は一意である必要がある")
        if not isinstance(self.理由, tuple) or any(not isinstance(項目, str) for 項目 in self.理由):
            raise TypeError("HDS作用結果の理由は文字列tupleである必要がある")
        if type(self.停止要求) is not bool:
            raise TypeError("HDS作用結果の停止要求はboolである必要がある")
        for 名称, 型 in (("認識更新", HDS認識項目), ("仮説更新", HDS仮説), ("観測要求追加", HDS観測要求), ("枝更新", HDS作業枝), ("草案更新", HDS草案), ("形成更新", HDS形成関係)):
            _一意型群(getattr(self, 名称), 型, 名称)
        for 名称 in ("認識削除", "成果削除"):
            _文字集合を検査(名称, getattr(self, 名称))
        if self.認識削除 & {x.ID for x in self.認識更新} or self.成果削除 & {x[0] for x in self.成果}:
            raise ValueError("同一項目の更新・削除は同時にできない")
        for 群 in (self.依存追加, self.依存削除):
            if not isinstance(群, tuple) or any(not isinstance(x, HDS依存辺) for x in 群) or len(set(群)) != len(群):
                raise ValueError("依存変更の型・一意性が不正")
        if set(self.依存追加) & set(self.依存削除):
            raise ValueError("同じ依存辺を追加・削除できない")
        if self.記憶更新 is not None and not isinstance(self.記憶更新, HDS記憶):
            raise TypeError("記憶更新型が不正")
        if self.阻害 is not None and not isinstance(self.阻害, HDS阻害):
            raise TypeError("阻害型が不正")
        文字列組(tuple(k for k, _ in self.検証依存), "検証依存ノード")
        if any(not isinstance(x, HDS検証票) for x in self.検証票追加):
            raise TypeError("検証票型が不正")


@dataclass(frozen=True, slots=True)
class HDS作用記録:
    番号: int
    作用ID: str
    作用入力署名: str
    作用状態: HDS作用状態
    前状態署名: str
    後状態署名: str
    状態差: HDS状態差
    対象残差: tuple[str, ...]
    対象未達状態: tuple[str, ...]
    理由: tuple[str, ...] = ()
    消費認識: tuple[str, ...] = ()
    消費資源: int = 0
    阻害: HDS阻害 | None = None
    計画由来: tuple[str, ...] = ()

    未来状態: tuple[HDS未来状態, ...] = ()
    診断: HDS失敗診断 | None = None
    目的進展: bool = False
    目的契約: str = ""
    目的依存: tuple[str, ...] = ()
    必要ノード: tuple[str, ...] = ()
    進展根拠: tuple[str, ...] = ()
    探索契約ID: str = ""
    探索契約署名: str = ""
    指示前観測: object | None = None
    指示後観測: object | None = None
    対応座標: tuple[str, ...] = ()
    作用対応: object | None = None
    対応契約版: str = ""
    作用定義ID: str = ""


@dataclass(frozen=True, slots=True)
class HDS入力更新記録:
    番号: int
    作用履歴位置: int
    入力署名: str
    前状態署名: str
    後状態署名: str
    状態差: HDS状態差


@dataclass(frozen=True, slots=True)
class HDS実行結果:
    終端: HDS終端
    状態: HDS実行状態
    履歴: tuple[HDS作用記録, ...]
    理由: tuple[str, ...] = ()
    停止種別: 停止理由 | None = None
    計装: HDS計装 = HDS計装()
    観測待ち: tuple[HDS観測要求, ...] = ()
    阻害履歴: tuple[HDS阻害, ...] = ()
    入力履歴: tuple[HDS入力更新記録, ...] = ()
    目的経路: HDS目的経路 | None = None
    指示帰還: object | None = None
    指示除外: tuple[tuple[str, tuple[str, ...]], ...] = ()
    指示開始署名: str = ""


class HDS作用器(Protocol):
    作用ID: str

    def 機会(self, 状態: HDS実行状態) -> HDS作用機会 | None: ...

    def 実行(self, 状態: HDS実行状態) -> HDS作用結果: ...


class 標準HDS作用選択器:
    """残差と未達状態を主語に、次に実行する既存作用を選ぶ。

    能力得点や回答ラベルを勝者選択へ流用しない。直接解消できる残差/未達状態を
    最優先し、同率では作用側が宣言した優先度、資源負荷、作用IDだけで決定する。
    中間状態を作る作用も、他に直接進展する作用が無い場合は候補に残す。
    """

    def __init__(self, 適応記憶=None) -> None:
        self.適応記憶 = 適応記憶

    def 選択(
        self,
        状態: HDS実行状態,
        機会群: Sequence[HDS作用機会],
        履歴: Sequence[HDS作用記録] = (),
    ) -> HDS作用機会 | None:
        使用済み = {(項目.作用ID, 項目.作用入力署名) for 項目 in 履歴}
        from .統合駆動_v2.仮説 import 識別対数
        候補列 = []
        for 機会 in 機会群:
            if not 機会.状態変更可能:
                continue
            if not 機会.入力状態.issubset(状態.成立状態):
                continue
            if (機会.作用ID, 機会.作用入力署名) in 使用済み:
                continue
            残差被覆 = len(状態.残差.intersection(機会.計画解消対象))
            状態被覆 = len(状態.未達状態.intersection(機会.計画出力状態)) + len(set(機会.識別対象).intersection(状態.要求認識))
            直接被覆 = 残差被覆 + 状態被覆
            特異度 = (
                直接被覆 / max(1, len(機会.計画解消対象) + len(機会.計画出力状態))
                if 直接被覆 else 0.0
            )
            学習得点, 学習経路長 = (
                self.適応記憶.作用経路評価(状態, 機会)
                if self.適応記憶 is not None and hasattr(self.適応記憶, "作用経路評価")
                else (0, 0)
            )
            候補列.append((
                直接被覆,
                sum(識別対数(状態.仮説, ID) for ID in 機会.識別対象),
                特異度,
                int(学習得点),
                int(学習経路長),
                float(機会.優先度),
                max(0, int(機会.資源負荷)),
                機会.作用ID,
                機会,
            ))
        if not 候補列:
            return None
        候補列.sort(key=lambda 行: (
            -行[0], -行[1], -行[2], -行[3],
            行[4] if 行[3] > 0 else 0,
            -行[5], 行[6], 行[7],
        ))
        return 候補列[0][-1]


class HDS関数作用:
    """既存の決定論的部品をHDS作用へ接続する薄い適合器。"""

    def __init__(
        self,
        作用ID: str,
        実行関数: Callable[[HDS実行状態], HDS作用結果],
        *,
        入力状態: Sequence[str] = (),
        出力状態: Sequence[str] = (),
        解消対象: Sequence[str] = (),
        資源負荷: int = 1,
        優先度: float = 0.0,
        根拠: Sequence[str] = (),
        機会判定: Callable[[HDS実行状態], bool] | None = None,
        入力署名: Callable[[HDS実行状態], str] | None = None,
        読取認識: Sequence[str] = (),
        読取成果: Sequence[str] = (),
        必要権限: Sequence[str] = (),
        契約版: str = "v1",
        計画仕様: HDS作用仕様 | None = None,
        純粋作用: bool = False,
        作用定義ID: str | None = None,
        意味入力署名: Callable[[HDS実行状態], str] | None = None,
        目的依存: Sequence[str] = (),
        生成成果: Sequence[str] = (),
        読取ノード: Sequence[str] = (),
        生成ノード: Sequence[str] = (),
        契約完全: bool = False,
        探索: HDS探索契約 | None = None,
    ) -> None:
        文字(作用ID, "作用ID")
        整数(資源負荷, "作用資源負荷")
        if type(優先度) not in (int, float) or not math.isfinite(優先度):
            raise ValueError("作用優先度は有限数が必要")
        if not callable(実行関数):
            raise TypeError("作用実行関数が必要")
        self.作用ID = 作用ID
        self._実行関数 = 実行関数
        self._入力状態 = frozenset(str(x) for x in 入力状態)
        self._出力状態 = frozenset(str(x) for x in 出力状態)
        self._解消対象 = frozenset(str(x) for x in 解消対象)
        self._資源負荷 = int(資源負荷)
        self._優先度 = float(優先度)
        self._根拠 = tuple(str(x) for x in 根拠)
        self._機会判定 = 機会判定
        self._入力署名 = 入力署名
        self._読取認識 = tuple(読取認識)
        self._読取成果 = tuple(読取成果)
        self._必要権限 = tuple(必要権限)
        self._契約版 = 契約版
        self._目的依存 = tuple(str(x) for x in 目的依存)
        文字列組(self._目的依存, "作用目的依存")
        for x in self._目的依存:
            try:
                ノードを検査(x)
            except ValueError as exc:
                raise ValueError("作用目的依存: " + str(exc)) from exc
        if 作用定義ID is None:
            # 現行運用の能力作用は計画/工程が呼出住所で、末尾能力名が定義名。
            # それ以外は従来どおり作用IDを定義IDとして扱う。
            parts = 作用ID.split("/")
            作用定義ID = "運用能力/" + parts[-1] if 作用ID.startswith("運用能力/") and len(parts) >= 4 else 作用ID
        文字(作用定義ID, "作用定義ID")
        if 意味入力署名 is not None and not callable(意味入力署名):
            raise TypeError("意味入力署名は呼出可能である必要がある")
        self.作用定義ID = 作用定義ID
        self._意味入力署名 = 意味入力署名
        self.計画仕様 = 計画仕様 or HDS作用仕様(
            作用ID, self._入力状態, self._出力状態, 解消残差=self._解消対象,
            読取認識=self._読取認識, 必要権限=self._必要権限,
            資源負荷=self._資源負荷, 版=契約版, 純粋=純粋作用,
            読取成果=self._読取成果, 生成成果=tuple(生成成果),
            読取ノード=tuple(読取ノード), 生成ノード=tuple(生成ノード),
            目的依存=self._目的依存, 契約完全=契約完全, 探索=探索,
        )
        if self.計画仕様.作用ID != self.作用ID:
            raise ValueError("計画仕様と作用IDが異なる")
        # 計画側だけに存在する読取条件を、実行側から落とさない。
        spec = self.計画仕様
        if 探索 is not None and spec.探索 is not None and 探索 != spec.探索:
            raise ValueError("計画仕様と作用の探索契約が異なる")
        self.計画仕様 = replace(spec,
            探索=探索 or spec.探索,
            入力状態=spec.入力状態 | self._入力状態,
            読取認識=tuple(sorted(set(spec.読取認識) | set(self._読取認識))),
            読取成果=tuple(sorted(set(spec.読取成果) | set(self._読取成果))),
            生成成果=tuple(sorted(set(spec.生成成果) | set(生成成果))),
            読取ノード=tuple(sorted(set(spec.読取ノード) | set(読取ノード))),
            生成ノード=tuple(sorted(set(spec.生成ノード) | set(生成ノード))),
            目的依存=tuple(sorted(set(spec.目的依存) | set(self._目的依存))),
            必要権限=tuple(sorted(set(spec.必要権限) | set(self._必要権限))),
            資源負荷=max(spec.資源負荷, self._資源負荷))
        self._入力状態 = self.計画仕様.入力状態
        self._読取認識 = self.計画仕様.読取認識
        self._読取成果 = self.計画仕様.読取成果
        self._読取ノード = self.計画仕様.読取ノード
        self._生成ノード = tuple(sorted(set(self.計画仕様.生成ノード)
                                  | {"成果:" + x for x in self.計画仕様.生成成果}))
        self._目的依存 = self.計画仕様.目的依存
        self._必要権限 = self.計画仕様.必要権限
        self._資源負荷 = self.計画仕様.資源負荷

    @property
    def 目的依存(self) -> tuple[str, ...]:
        return self._目的依存

    def 意味入力を署名(self, 状態: HDS実行状態) -> str:
        """作用が宣言して読む意味入力だけから、作用定義単位の署名を構成する。"""
        if self.計画仕様.観測入力:
            from .統合駆動_v2.目的保持 import 目的ノード有効, 目的ノード署名
            if not all(目的ノード有効(状態, k) for k in self.計画仕様.観測入力):
                return _署名((self.作用定義ID, self._契約版, "観測入力準備待ち",
                    tuple((k, 目的ノード署名(状態, k)) for k in self.計画仕様.観測入力)))
        if self._意味入力署名 is not None:
            return str(self._意味入力署名(状態))
        if self._入力署名 is not None:
            return str(self._入力署名(状態))
        if self._読取認識 or self._読取成果 or self._入力状態 or self._読取ノード or self.計画仕様.観測入力:
            from .コア.状態操作 import ノード意味値
            from .統合駆動_v2.目的保持 import 目的ノード署名
            return _署名((
                self.作用定義ID, self._契約版,
                tuple((k, _署名(ノード意味値(状態, "認識:" + k))) for k in self._読取認識),
                tuple((k, _署名(ノード意味値(状態, "成果:" + k))) for k in self._読取成果),
                tuple((k, _署名(ノード意味値(状態, "状態:" + k))) for k in sorted(self._入力状態)),
                tuple((k, 目的ノード署名(状態, k)) for k in (*self._読取ノード, *self.計画仕様.観測入力)),
            ))
        # 明示入力がない作用を全体状態で過剰分断しない。
        # 状態依存が意味上必要なら読取契約または専用署名関数として宣言する。
        return _署名((self.作用定義ID, self._契約版, "明示意味入力なし"))

    def 機会(self, 状態: HDS実行状態) -> HDS作用機会 | None:
        from .統合駆動_v2.目的保持 import 目的ノード有効, 目的ノード署名
        準備済 = all(目的ノード有効(状態, x) for x in self.計画仕様.入力ノード集合)
        # 不足入力を読む任意の判定・署名関数を起動しない。契約上の候補だけを
        # 返して生産側の逆算を可能にし、実入力の成立後に判定を実行する。
        if 準備済 and self._機会判定 is not None and not bool(self._機会判定(状態)):
            return None
        if not 準備済:
            署名 = _署名((self.作用ID, self._契約版, "入力準備待ち",
                        tuple((x, 目的ノード署名(状態, x)) for x in sorted(self.計画仕様.入力ノード集合))))
        elif self._入力署名 is not None:
            署名 = self._入力署名(状態)
        elif self._読取認識 or self._読取成果:
            署名 = _署名((self._契約版, tuple((k, 状態.ノード署名("認識:" + k)) for k in self._読取認識), tuple((k, 状態.ノード署名("成果:" + k)) for k in self._読取成果), tuple((k, 状態.ノード署名("状態:" + k)) for k in sorted(self._入力状態))))
        else:
            署名 = 状態.状態署名
        # 未成立の上流入力を読む意味署名関数は、候補観測時にはまだ実行しない。
        # 実入力が成立した後、通常循環が再署名してから実行内期待を適用する。
        意味署名 = self.意味入力を署名(状態) if 準備済 else str(署名)
        return HDS作用機会(
            self.作用ID,
            str(署名),
            self._入力状態,
            self._出力状態,
            self._解消対象,
            self._資源負荷,
            self._優先度,
            True,
            self._根拠,
            self._読取認識,
            self._読取成果,
            self._必要権限,
            契約版=self._契約版,
            作用定義ID=self.作用定義ID,
            意味入力署名=str(意味署名),
            目的依存=self._目的依存,
            読取ノード=self._読取ノード,
            生成ノード=self._生成ノード,
        )

    def 実行(self, 状態: HDS実行状態) -> HDS作用結果:
        結果 = self._実行関数(状態)
        if not isinstance(結果, HDS作用結果):
            raise TypeError("HDS作用の実行関数はHDS作用結果を返す必要がある")
        return 結果


@dataclass(frozen=True, slots=True)
class HDS作用供給器:
    """現在状態から作用候補を構成する読取専用の供給境界。

    選択・実行・採否は行わない。資料内の命令から実行コードを登録しない。
    供給器自身と返却する作用器は、呼出側が信頼する実装に限定する。
    """
    ID: str
    構成: Callable
    版: str = "v1"

    def __post_init__(self):
        文字(self.ID)
        文字(self.版)
        if not callable(self.構成):
            raise TypeError("作用供給器には構成関数が必要")


class HDS実行主体:
    """観測・再評価・修復・採否を同じ通常循環で所有する。外付け監督は呼ばない。"""

    def __init__(self, 作用群: Sequence[HDS作用器], *,
                 作用選択器: 標準HDS作用選択器 | None = None,
                 最大作用回数: int = 32,
                 政策: HDS運用政策 | None = None,
                 観測器: Sequence[HDS観測器] = (),
                 仮説雛型: Sequence[HDS仮説雛型] = (),
                 検証器: Sequence[HDS検証器] = (),
                 最終検証器: Sequence[HDS検証器] = (),
                 関係規則: Sequence[HDS関係規則] = (),
                 未来制約: Sequence[HDS未来制約] = (),
                 作用供給器: Sequence[HDS作用供給器] = (),
                 停止要求: Callable[[], bool] | None = None,
                 適応記憶: HDS適応記憶 | None = None) -> None:
        if type(最大作用回数) is not int or not 1 <= 最大作用回数 <= 4096:
            raise ValueError("HDS最大作用回数は1..4096の整数である必要がある")
        self.作用群 = tuple(作用群)
        self.作用供給器 = tuple(作用供給器)
        _一意型群(self.作用供給器, HDS作用供給器, "作用供給器")
        if 停止要求 is not None and not callable(停止要求):
            raise TypeError("停止要求は呼出可能である必要がある")
        self.停止要求 = 停止要求
        self.適応記憶 = 適応記憶 if 適応記憶 is not None else HDS適応記憶()
        if not isinstance(self.適応記憶, HDS適応記憶):
            raise TypeError("適応記憶はHDS適応記憶である必要がある")
        文字列組(tuple(x.作用ID for x in self.作用群), "作用ID")
        if any(x.作用ID.startswith("内的/") for x in self.作用群):
            raise ValueError("内的/はコア内部作用の予約名前空間")
        self.作用選択器 = 作用選択器 or 標準HDS作用選択器(self.適応記憶)
        self.最大作用回数 = 最大作用回数
        self.政策 = 政策 if 政策 is not None else HDS運用政策()
        if not isinstance(self.政策, HDS運用政策):
            raise TypeError("HDS運用政策型が必要")
        self.観測器 = tuple(観測器)
        self.仮説雛型 = tuple(仮説雛型)
        self.検証器 = tuple(検証器)
        self.最終検証器 = tuple(最終検証器)
        self.関係規則 = tuple(関係規則)
        self.未来制約 = tuple(未来制約)
        _一意型群(self.関係規則, HDS関係規則, "関係規則")
        _一意型群(self.未来制約, HDS未来制約, "未来制約")
        for 名称, 型 in (("観測器", HDS観測器), ("仮説雛型", HDS仮説雛型), ("検証器", HDS検証器), ("最終検証器", HDS検証器)):
            _一意型群(getattr(self, 名称), 型, 名称)

    @staticmethod
    def _状態更新(前: HDS実行状態, 作用結果: HDS作用結果):
        from .コア.状態操作 import 状態差を受理
        return 状態差を受理(前, 作用結果)

    def 実行(self, 初期状態: HDS実行状態) -> HDS実行結果:
        from .統合駆動_v2.循環 import 通常循環
        結果 = 通常循環(self, 初期状態)
        self.適応記憶.実行結果を受け取る(結果)
        return 結果

    def 再開(self, 前回: HDS実行結果, 追加入力: HDS作用結果 | None = None) -> HDS実行結果:
        from .統合駆動_v2.循環 import 通常循環
        if not isinstance(前回, HDS実行結果):
            raise TypeError("再開には実行結果が必要")
        if 前回.停止種別 in (停止理由.権限制約, 停止理由.方針制約):
            raise ValueError("権限・方針停止を再開APIで迂回できない")
        状態 = 前回.状態
        if 追加入力 is not None:
            状態, 差 = self._状態更新(状態, 追加入力)
            入力記録 = HDS入力更新記録(len(前回.入力履歴) + 1, len(前回.履歴), _署名(追加入力),
                                       前回.状態.状態署名, 状態.状態署名, 差)
            前回 = replace(前回, 入力履歴=前回.入力履歴 + (入力記録,),
                           計装=replace(前回.計装, 外部入力数=前回.計装.外部入力数 + 1,
                                        依存失効数=前回.計装.依存失効数 + len(差.失効対象)))
        結果 = 通常循環(self, 状態, 前回)
        self.適応記憶.実行結果を受け取る(結果)
        return 結果


__all__ = [
    "HDS実行主体版",
    "HDS終端",
    "HDS作用状態",
    "HDS実行状態",
    "HDS状態差",
    "HDS作用機会",
    "HDS作用結果",
    "HDS作用記録",
    "HDS実行結果",
    "HDS入力更新記録",
    "HDS作用器",
    "標準HDS作用選択器",
    "HDS関数作用",
    "HDS実行主体",
    "HDS作用供給器",
]

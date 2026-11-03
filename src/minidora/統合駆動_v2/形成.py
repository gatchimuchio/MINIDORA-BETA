"""実行経験から条件付きの再利用手順を形成する。成果を世界知識へ無言転用しない。"""
from __future__ import annotations
from dataclasses import dataclass, replace
from enum import StrEnum
from .値 import 文字, 文字列組, 署名


class HDS形成採用状態(StrEnum):
    試行 = "試行"
    有効 = "有効"
    隔離 = "隔離"
    棄却 = "棄却"


@dataclass(frozen=True, slots=True)
class HDS経験:
    ID: str
    前提状態: frozenset[str]
    要求状態: frozenset[str]
    作用列: tuple[str, ...]
    成功: bool
    根拠署名: str
    文脈署名: str
    失敗署名: str = ""
    作用契約: tuple[tuple[str, str], ...] = ()

    def __post_init__(self):
        文字(self.ID)
        文字(self.根拠署名)
        文字(self.文脈署名)
        文字列組(self.作用列, 一意=False)
        if not self.作用列:
            raise ValueError("作用のない経験から手順を形成しない")
        if type(self.成功) is not bool:
            raise TypeError("経験成否はbool")


@dataclass(frozen=True, slots=True)
class HDS形成関係:
    ID: str
    前提状態: frozenset[str]
    要求状態: frozenset[str]
    作用列: tuple[str, ...]
    文脈署名: str
    由来: tuple[str, ...]
    反例: tuple[str, ...] = ()
    検証契約: str = ""
    版: int = 1
    作用契約: tuple[tuple[str, str], ...] = ()
    未解決反例: tuple[str, ...] = ()
    除外反例: tuple[str, ...] = ()
    採用状態: HDS形成採用状態 = HDS形成採用状態.有効
    復帰検証契約: str = ""
    審査履歴: tuple[tuple[str, str, str, tuple[str, ...]], ...] = ()

    def __post_init__(self):
        文字(self.ID)
        文字(self.文脈署名)
        文字列組(self.作用列, 一意=False)
        文字列組(self.由来)
        文字列組(self.反例)
        文字列組(self.未解決反例)
        文字列組(self.除外反例)
        if not isinstance(self.採用状態, HDS形成採用状態):
            raise TypeError("形成採用状態型が必要")
        if not isinstance(self.検証契約, str) or not isinstance(self.復帰検証契約, str):
            raise TypeError("検証契約は文字列")
        if not self.由来 or not self.作用列:
            raise ValueError("由来と作用列のない形成関係")
        if not set(self.未解決反例) <= set(self.反例):
            raise ValueError("未解決反例は反例履歴に存在する必要がある")
        if not set(self.除外反例) <= set(self.反例):
            raise ValueError("除外反例は反例履歴に存在する必要がある")
        if self.採用状態 == HDS形成採用状態.有効 and self.未解決反例:
            raise ValueError("未解決反例を持つ形成関係を有効採用できない")
        if self.復帰検証契約 and self.採用状態 != HDS形成採用状態.隔離:
            raise ValueError("復帰検証契約は隔離中だけ保持できる")
        if not isinstance(self.審査履歴, tuple):
            raise TypeError("審査履歴はtuple")
        for 行 in self.審査履歴:
            if not isinstance(行, tuple) or len(行) != 4:
                raise TypeError("審査履歴行の型が不正")
            処置, 理由, 主体, 参照 = 行
            文字(処置, "審査処置")
            文字(理由, "審査理由")
            文字(主体, "承認主体")
            文字列組(参照, "審査反例")

    @property
    def 使用可能(self) -> bool:
        return self.採用状態 == HDS形成採用状態.有効 and bool(self.検証契約) and not self.未解決反例

    def 反例追加(self, 失敗署名: str) -> HDS形成関係:
        文字(失敗署名)
        return replace(
            self,
            反例=tuple(sorted(set(self.反例) | {失敗署名})),
            未解決反例=tuple(sorted(set(self.未解決反例) | {失敗署名})),
            検証契約="",
            復帰検証契約="",
            採用状態=HDS形成採用状態.隔離,
            版=self.版 + 1,
        )

    def 再評価へ戻す(self) -> HDS形成関係:
        if self.採用状態 == HDS形成採用状態.棄却:
            return self
        return replace(
            self,
            検証契約="",
            復帰検証契約="",
            採用状態=HDS形成採用状態.隔離,
            版=self.版 + 1,
        )


def 経験から形成(経験: HDS経験) -> HDS形成関係:
    if not 経験.成功:
        raise ValueError("失敗経験を成功則として形成できない")
    return HDS形成関係(
        "形成:" + 署名((経験.前提状態, 経験.要求状態, 経験.作用列, 経験.文脈署名))[:24],
        経験.前提状態, 経験.要求状態, 経験.作用列, 経験.文脈署名, (経験.根拠署名,),
        作用契約=経験.作用契約, 採用状態=HDS形成採用状態.試行,
    )


def 再実行で検証(関係: HDS形成関係, 実測: HDS経験, 検証契約: str) -> HDS形成関係:
    文字(検証契約)
    if 実測.文脈署名 != 関係.文脈署名 or 実測.要求状態 != 関係.要求状態 or not 関係.前提状態 <= 実測.前提状態:
        raise ValueError("検証経験の適用範囲が異なる")
    if not 実測.成功:
        return 関係.反例追加(実測.失敗署名 or 実測.根拠署名)
    if 関係.作用契約 and 実測.作用契約 != 関係.作用契約:
        raise ValueError("検証時の作用契約版が異なる")
    if 実測.作用列 != 関係.作用列:
        raise ValueError("別手順の成功を対象関係の検証に転用できない")
    由来 = tuple(sorted(set(関係.由来) | {実測.根拠署名}))
    if 関係.採用状態 == HDS形成採用状態.棄却:
        return replace(関係, 由来=由来)
    if 関係.採用状態 == HDS形成採用状態.隔離:
        # 隔離後の成功は復帰根拠にはなるが、採用判断そのものは自動化しない。
        return replace(関係, 由来=由来, 復帰検証契約=検証契約, 版=関係.版 + 1)
    return replace(
        関係, 由来=由来, 検証契約=検証契約,
        採用状態=HDS形成採用状態.有効, 復帰検証契約="", 版=関係.版 + 1,
    )


def 形成隔離を審査(
    関係: HDS形成関係,
    処置: str,
    理由: str,
    承認主体: str,
    反例参照: tuple[str, ...] = (),
) -> HDS形成関係:
    """隔離形成の出口。現在の未解決反例を明示した審査だけで復帰または棄却する。"""
    文字(処置, "審査処置")
    文字(理由, "審査理由")
    文字(承認主体, "承認主体")
    文字列組(反例参照, "審査反例")
    if 関係.採用状態 != HDS形成採用状態.隔離:
        raise ValueError("隔離中の形成関係だけ審査できる")
    現行 = tuple(sorted(関係.未解決反例))
    指定 = tuple(sorted(反例参照))
    if 指定 != 現行:
        raise ValueError("現在の未解決反例を全て明示して審査する必要がある")
    履歴 = 関係.審査履歴 + ((処置, 理由, 承認主体, 指定),)
    if 処置 == "復帰":
        if not 関係.復帰検証契約:
            raise ValueError("隔離後の再実行検証が成立していない")
        return replace(
            関係,
            未解決反例=(),
            除外反例=tuple(sorted(set(関係.除外反例) | set(指定))),
            検証契約=関係.復帰検証契約,
            復帰検証契約="",
            採用状態=HDS形成採用状態.有効,
            審査履歴=履歴,
            版=関係.版 + 1,
        )
    if 処置 == "棄却":
        return replace(
            関係,
            検証契約="",
            復帰検証契約="",
            採用状態=HDS形成採用状態.棄却,
            審査履歴=履歴,
            版=関係.版 + 1,
        )
    raise ValueError("処置は『復帰』または『棄却』")


def 実行結果から経験(ID, 初期状態, 結果, 文脈署名, 仕様群):
    """実行トレースからのみ経験を形成する。推測で成功フラグを補わない。"""
    from ..HDS実行主体 import HDS実行結果, HDS終端
    if not isinstance(結果, HDS実行結果) or not 結果.履歴:
        raise ValueError("実行履歴が必要")
    契約 = {x.作用ID: x.版 for x in 仕様群}
    列 = tuple(x.作用ID for x in 結果.履歴)
    if any(k not in 契約 for k in 列):
        raise ValueError("全実行作用の契約版が必要")
    成功 = 結果.終端 == HDS終端.採用 and 結果.状態.閉包済み
    return HDS経験(ID, 初期状態.成立状態, 初期状態.要求状態, 列, 成功,
                  署名((初期状態.状態署名, 結果.状態.状態署名, 結果.履歴)), 文脈署名,
                  "" if 成功 else 署名(結果.阻害履歴), tuple(sorted((k, 契約[k]) for k in set(列))))


def 形成手順を再利用(関係, 成立状態, 残差, 要求状態, 仕様群, 文脈署名, 最大資源, *, 可用ノード=frozenset()):
    """保存手順を現在の契約で再展開する。成果や成立状態を直接返さない。"""
    from .計画 import HDS構成計画
    if not 関係.使用可能 or not 関係.作用契約 or 関係.文脈署名 != 文脈署名 or 関係.要求状態 != 要求状態 or not 関係.前提状態 <= 成立状態:
        return None
    束 = {s.作用ID: s for s in 仕様群}
    契約 = dict(関係.作用契約)
    if any(k not in 束 or k not in 契約 or 束[k].版 != 契約[k] for k in 関係.作用列):
        return None
    s, r, 列, 費用 = 成立状態, 残差, [], 0
    nodes = frozenset(x for x in 可用ノード if not x.startswith(("状態:", "残差:")))
    for k in 関係.作用列:
        a = 束[k]
        available = nodes | {"状態:" + x for x in s}
        if any((x[3:] in r) if x.startswith("残差:") else (x not in available) for x in a.入力ノード集合):
            return None
        nn = frozenset(x for x in nodes | a.出力ノード集合 if not x.startswith(("状態:", "残差:")))
        ns, nr = (s - a.削除状態) | a.追加状態, (r - a.解消残差) | a.追加残差
        if (ns, nr, nn) != (s, r, nodes):
            列.append(k)
            費用 += a.資源負荷
        s, r, nodes = ns, nr, nn
    if 列 and 要求状態 <= s and not r and 費用 <= 最大資源:
        return HDS構成計画(tuple(列), 0, False, 費用)
    return None

from __future__ import annotations

from collections import deque
from dataclasses import dataclass, replace

from ..コア.効果 import 期待効果


@dataclass(frozen=True, slots=True)
class _作用文脈:
    作用定義ID: str
    意味入力署名: str
    種別: str
    契約版: str


@dataclass(frozen=True, slots=True)
class _経験:
    文脈: _作用文脈
    成立: bool
    変化有無: bool
    追加状態: frozenset[str]
    削除状態: frozenset[str]
    解消残差: frozenset[str]
    追加残差: frozenset[str]
    反証: bool = False


@dataclass(frozen=True, slots=True)
class _経路経験:
    作用定義ID: str
    対象残差: tuple[str, ...]
    対象未達状態: tuple[str, ...]
    経験署名: str
    支持: bool
    経路長: int


def _文脈(機会) -> _作用文脈:
    # 旧作用機会は作用定義ID/意味入力署名を持たないため、宣言済みの旧契約へ縮退する。
    作用ID = str(getattr(機会, "作用ID"))
    作用定義ID = str(getattr(機会, "作用定義ID", "") or 作用ID)
    意味入力署名 = getattr(機会, "意味入力署名", "")
    if not 意味入力署名:
        意味入力署名 = getattr(機会, "作用入力署名", "")
    if not 意味入力署名:
        from ..コア.値 import 署名
        意味入力署名 = 署名((
            作用定義ID,
            tuple(sorted(getattr(機会, "入力状態", ()))),
            tuple(getattr(機会, "読取認識", ())),
            tuple(getattr(機会, "読取成果", ())),
        ))
    return _作用文脈(
        作用定義ID,
        str(意味入力署名),
        str(getattr(機会, "種別")),
        str(getattr(機会, "契約版")),
    )


def _成立か(結果) -> bool:
    状態 = getattr(結果, "状態", None)
    return getattr(状態, "value", 状態) == "成立"


def _既に満たす(機会, 前状態) -> bool:
    if 前状態 is None:
        return False
    出力済み = 機会.出力状態 <= 前状態.成立状態
    解消済み = not bool(機会.解消対象 & 前状態.残差)
    return 出力済み and 解消済み


class HDS適応記憶:
    """同一定義・同一意味入力で観測した効果を、Core寿命内の後続処理へ継続利用する。

    最終回答や採点結果ではなく、各作用の実測状態差だけを保持する。
    最新の反証以後に再確認された安定効果だけを後続計画へ反映する。
    """

    __slots__ = ("_経験列", "_経路経験列")

    def __init__(self, 最大経験数: int = 256) -> None:
        if type(最大経験数) is not int or not 1 <= 最大経験数 <= 4096:
            raise ValueError("最大経験数は1..4096の整数が必要")
        self._経験列 = deque(maxlen=最大経験数)
        self._経路経験列 = deque(maxlen=最大経験数)

    @property
    def 経験数(self) -> int:
        return len(self._経験列)

    @property
    def 経路経験数(self) -> int:
        return len(self._経路経験列)

    @property
    def 経路状態集計(self) -> tuple[int, int, int]:
        """(有効経路, 隔離経路, 未確定経路)。診断専用で選択状態を変更しない。"""
        群 = {}
        for x in self._経路経験列:
            鍵 = (x.作用定義ID, x.対象残差, x.対象未達状態)
            行 = 群.setdefault(鍵, [set(), set()])
            行[0 if x.支持 else 1].add(x.経験署名)
        有効 = 隔離 = 未確定 = 0
        for 支持, 反証 in 群.values():
            if len(支持) >= 2 and len(支持) > len(反証):
                有効 += 1
            elif len(反証) >= 2 and len(反証) > len(支持):
                隔離 += 1
            else:
                未確定 += 1
        return 有効, 隔離, 未確定

    @property
    def 状態署名(self) -> str:
        from ..コア.値 import 署名
        return 署名((tuple(self._経験列), tuple(self._経路経験列)))

    def 初期化(self) -> None:
        self._経験列.clear()
        self._経路経験列.clear()

    def 結果を受け取る(self, 機会, 結果, 状態差, 前状態=None) -> None:
        if str(機会.作用ID).startswith("内的/"):
            return
        成立 = _成立か(結果)
        変化 = bool(状態差.変化有無)
        # 同じ意味入力で実行失敗した場合、以前の成功期待をそのまま残さない。
        # 成功かつ無変化は、既に効果が成立していた場合は反証にしない。
        反証 = (not 成立) or (成立 and not 変化 and not _既に満たす(機会, 前状態))
        self._経験列.append(
            _経験(
                _文脈(機会),
                成立,
                変化,
                frozenset(getattr(状態差, "追加状態", ())),
                frozenset(getattr(状態差, "削除状態", ())),
                frozenset(getattr(状態差, "解消残差", ())),
                frozenset(getattr(状態差, "追加残差", ())),
                反証,
            )
        )

    def _安定効果(self, 機会) -> 期待効果:
        文脈 = _文脈(機会)
        対象 = [x for x in self._経験列 if x.文脈 == 文脈]
        if not 対象:
            return 期待効果()
        # 最新の反証より前の成功は再利用しない。
        最後の反証 = max((i for i, x in enumerate(対象) if x.反証), default=-1)
        有効 = [x for x in 対象[最後の反証 + 1:] if x.成立 and x.変化有無]
        if not 有効:
            return 期待効果()

        追加 = set(有効[0].追加状態)
        削除 = set(有効[0].削除状態)
        解消 = set(有効[0].解消残差)
        追加残差 = set(有効[0].追加残差)
        for 経験 in 有効[1:]:
            追加.intersection_update(経験.追加状態)
            削除.intersection_update(経験.削除状態)
            解消.intersection_update(経験.解消残差)
            追加残差.intersection_update(経験.追加残差)
        return 期待効果(frozenset(追加), frozenset(削除), frozenset(解消),
                      frozenset(追加残差), len(有効))

    def 実行結果を受け取る(self, 実行結果) -> None:
        """終端成否から、残差/未達状態ごとの作用経路を支持・反証として保持する。"""
        終端 = getattr(getattr(実行結果, "終端", None), "value", getattr(実行結果, "終端", None))
        状態 = getattr(実行結果, "状態", None)
        閉包 = bool(getattr(状態, "閉包済み", False))
        全体成功 = 終端 == "COMMIT" and 閉包
        履歴 = tuple(getattr(実行結果, "履歴", ()))
        経験署名 = str(getattr(履歴[0], "前状態署名", "")) if 履歴 else ""
        if not 経験署名:
            return
        既存 = set(self._経路経験列)
        経路長 = sum(1 for x in 履歴 if not str(getattr(x, "作用ID", "")).startswith("内的/"))
        for 記録 in 履歴:
            作用ID = str(getattr(記録, "作用ID", ""))
            作用定義ID = str(getattr(記録, "作用定義ID", "") or 作用ID)
            if not 作用ID or 作用ID.startswith("内的/"):
                continue
            残差 = tuple(sorted(str(x) for x in getattr(記録, "対象残差", ()) if str(x)))
            未達 = tuple(sorted(str(x) for x in getattr(記録, "対象未達状態", ()) if str(x)))
            if not 残差 and not 未達:
                continue
            作用状態 = getattr(getattr(記録, "作用状態", None), "value", getattr(記録, "作用状態", None))
            差 = getattr(記録, "状態差", None)
            進展 = bool(
                getattr(記録, "目的進展", False)
                or getattr(記録, "進展根拠", ())
                or getattr(差, "変化有無", False)
            )
            if 全体成功 and 作用状態 == "成立" and 進展:
                支持 = True
            elif not 全体成功 or 作用状態 != "成立":
                # 中間で進展してもrun全体が閉じなければ、経路成功の支持にはしない。
                # ARC2の反例監査と同じく、最終的に目的未達の経験を経路原理の反例へ戻す。
                支持 = False
            else:
                continue
            行 = _経路経験(作用定義ID, 残差, 未達, 経験署名, 支持, max(1, 経路長))
            if 行 not in 既存:
                self._経路経験列.append(行)
                既存.add(行)

    def 作用経路評価(self, 状態, 機会) -> tuple[int, int]:
        """支持差と、支持された最短COMMIT経路長を返す。未成立は(0, 0)。"""
        残差 = tuple(sorted(str(x) for x in (状態.残差 & 機会.計画解消対象)))
        未達 = tuple(sorted(str(x) for x in (状態.未達状態 & 機会.計画出力状態)))
        if not 残差 and not 未達:
            return 0, 0
        作用定義ID = str(getattr(機会, "作用定義ID", "") or 機会.作用ID)
        対象 = tuple(
            x for x in self._経路経験列
            if x.作用定義ID == 作用定義ID and x.対象残差 == 残差 and x.対象未達状態 == 未達
        )
        支持経験 = {x.経験署名 for x in 対象 if x.支持 and x.経験署名}
        反証経験 = {x.経験署名 for x in 対象 if not x.支持 and x.経験署名}
        if len(支持経験) >= 2 and len(支持経験) > len(反証経験):
            最短 = min(
                x.経路長 for x in 対象
                if x.支持 and x.経験署名 in 支持経験
            )
            return len(支持経験) - len(反証経験), 最短
        if len(反証経験) >= 2 and len(反証経験) > len(支持経験):
            # ARC2の反例隔離に対応する。経験は消さず、同型経路の次回優先度だけを下げる。
            return len(支持経験) - len(反証経験), 0
        return 0, 0

    def 作用経路得点(self, 状態, 機会) -> int:
        return self.作用経路評価(状態, 機会)[0]

    def 機会を補正(self, 機会):
        if str(機会.作用ID).startswith("内的/"):
            return 機会
        効果 = self._安定効果(機会)
        if 効果.空:
            return 機会
        # 契約効果は変更せず、経験由来の期待だけ別欄へ保持する。
        return replace(機会, 期待=効果)


__all__ = ["HDS適応記憶"]

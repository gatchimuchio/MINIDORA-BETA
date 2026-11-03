from __future__ import annotations

from dataclasses import replace
from typing import Mapping, TYPE_CHECKING

from .HDS選択実行系 import HDS選択実行結果, HDS選択推論実行
from .hds既存能力resolver import (
    既存MINIDORA提案解決,
    既存能力提案,
    既存提案源,
    既存提案状態,
)
from .hds適応候補調停 import HDS適応候補提案実行
from .模型 import MINIDORA模型核
from .参照 import 参照記録

if TYPE_CHECKING:
    from .HDS中間表現 import HDSIR
    from .K3機能 import K3相当能力核


def _承認済み(結果: HDS選択実行結果) -> bool:
    return bool(
        結果.状態 == "APPROVE"
        and 結果.回答ラベル is not None
        and 結果.回答内容 is not None
    )


def _提案済み(結果: HDS選択実行結果) -> bool:
    return bool(
        結果.状態 in {"APPROVE", "PROPOSE"}
        and 結果.回答ラベル is not None
        and 結果.回答内容 is not None
    )


def _模型根拠成立(結果: HDS選択実行結果) -> bool:
    if not _提案済み(結果):
        return False
    模型結果 = 結果.MINIDORA模型結果
    if 模型結果 is None:
        return False
    最有力 = getattr(模型結果, "参照最有力候補ID", None)
    if 最有力 != 結果.回答ラベル:
        return False
    辞書化 = getattr(模型結果, "参照候補辞書", None)
    if not callable(辞書化):
        return False
    得点群 = dict(辞書化())
    得点 = float(得点群.get(結果.回答ラベル, 0))
    最大 = max((float(x) for x in 得点群.values()), default=0.0)
    return bool(
        得点 > 0
        and 最大 > 0
        and sum(1 for x in 得点群.values() if float(x) == 最大) == 1
    )


def _旧補助根拠成立(結果: HDS選択実行結果) -> bool:
    if not _承認済み(結果):
        return False
    if "DIRECTED_関係_VERIFIED" in 結果.理由:
        return True
    K3結果 = 結果.K3結果
    if K3結果 is not None and int(getattr(K3結果, "根拠事実数", 0)) > 0:
        return True
    return int(結果.K証拠事実数) > 0


def HDS既存能力結果証明済み(結果: HDS選択実行結果) -> bool:
    """既存MINIDORAの模型または旧補助器が、自身の根拠で一意閉包したかを返す。"""
    return bool(_模型根拠成立(結果) or _旧補助根拠成立(結果))


def _旧補助提案(結果: HDS選択実行結果) -> 既存能力提案 | None:
    if not _旧補助根拠成立(結果):
        return None
    直接 = "DIRECTED_関係_VERIFIED" in 結果.理由
    return 既存能力提案(
        既存提案源.直接関係 if 直接 else 既存提案源.K3,
        既存提案状態.承認候補,
        結果.回答ラベル,
        根拠成立=True,
        一意=True,
        直接検証済み=直接,
        理由=tuple(結果.理由),
    )


def _能力模型提案(結果: HDS選択実行結果) -> 既存能力提案 | None:
    if not _模型根拠成立(結果):
        return None
    return 既存能力提案(
        既存提案源.能力模型,
        既存提案状態.承認候補,
        結果.回答ラベル,
        根拠成立=True,
        一意=True,
        理由=tuple(結果.理由),
    )


def HDS既存能力選択評価(
    質問IR: HDSIR,
    参照群: tuple[参照記録, ...],
    *,
    コンパイル,
    模型核: MINIDORA模型核,
    基礎能力核: K3相当能力核 | None = None,
    候補意味IR: Mapping[str, HDSIR] | None = None,
) -> HDS選択実行結果:
    """MINIDORA30正本を下限に、既存K3/graph/direct/能力v3を非退行で継承する。

    最初に正本formal模型を評価し、閉包済みなら完全透過する。
    formal未閉包時だけ旧補助器と能力v3をworkerとして起動し、
    既存能力resolverで競合を監査する。HDS自身は候補の勝者を選ばない。
    """

    正本結果 = HDS選択推論実行(
        質問IR,
        参照群,
        コンパイル=コンパイル,
        基礎能力核=None,
        候補意味IR=候補意味IR,
        模型核=模型核,
        正式模型評価=True,
    )
    if _承認済み(正本結果) or 基礎能力核 is None:
        return 正本結果

    旧補助結果 = HDS選択推論実行(
        質問IR,
        参照群,
        コンパイル=コンパイル,
        基礎能力核=基礎能力核,
        候補意味IR=候補意味IR,
        模型核=None,
        正式模型評価=False,
        作業再作用=True,
        局所再照合=True,
    )
    能力結果 = HDS適応候補提案実行(
        質問IR,
        参照群,
        コンパイル=コンパイル,
        基礎能力核=基礎能力核,
        候補意味IR=候補意味IR,
        模型核=模型核,
    )

    提案群: list[既存能力提案] = []
    旧提案 = _旧補助提案(旧補助結果)
    if 旧提案 is not None:
        提案群.append(旧提案)
    模型提案 = _能力模型提案(能力結果)
    if 模型提案 is not None:
        提案群.append(模型提案)

    if not 提案群:
        return 正本結果

    解決 = 既存MINIDORA提案解決(tuple(提案群))
    if 解決.状態 != 既存提案状態.承認候補 or 解決.回答 is None:
        return replace(
            正本結果,
            理由=tuple(dict.fromkeys((
                *tuple(正本結果.理由),
                *tuple(解決.理由),
                *tuple(解決.残差),
                "HDS_EXISTING_CAPABILITY_CONFLICT_OR_UNCLOSED",
            ))),
        )

    採用元 = set(解決.採用源)
    if 既存提案源.直接関係 in 採用元 or (
        既存提案源.K3 in 採用元
        and 既存提案源.能力模型 not in 採用元
    ):
        採用結果 = 旧補助結果
    elif (
        既存提案源.能力模型 in 採用元
        and 能力結果.回答ラベル == 解決.回答
        and _模型根拠成立(能力結果)
    ):
        採用結果 = 能力結果
    else:
        採用結果 = 旧補助結果

    if 採用結果.回答ラベル != 解決.回答:
        return replace(
            正本結果,
            理由=tuple(dict.fromkeys((
                *tuple(正本結果.理由),
                "HDS_EXISTING_CAPABILITY_RESOLUTION_MISMATCH",
            ))),
        )

    return replace(
        採用結果,
        状態="APPROVE",
        理由=tuple(dict.fromkeys((
            *tuple(採用結果.理由),
            *tuple(解決.理由),
            "HDS_EXISTING_CAPABILITY_INHERITED",
            *tuple(f"HDS_EXISTING_SOURCE:{x.value}" for x in 解決.採用源),
        ))),
    )


def HDS既存能力直接反証評価(
    質問IR: HDSIR,
    参照群: tuple[参照記録, ...],
    *,
    コンパイル,
    基礎能力核: K3相当能力核 | None,
    候補意味IR: Mapping[str, HDSIR] | None = None,
    基準ラベル: str,
    最小独立証拠数: int = 2,
) -> HDS選択実行結果 | None:
    """承認済み基準に対する強い直接反証だけを返す。

    goldや評価問題固有情報は使わない。旧補助器のDIRECTED関係検証が基準と異なる候補を
    APPROVEし、かつ直接検証proofが独立2件以上ある場合だけ反証候補として返す。
    """
    if 基礎能力核 is None:
        return None
    if type(最小独立証拠数) is not int or 最小独立証拠数 < 2:
        raise ValueError("直接反証の最小独立証拠数は2以上の整数である必要がある")
    結果 = HDS選択推論実行(
        質問IR,
        参照群,
        コンパイル=コンパイル,
        基礎能力核=基礎能力核,
        候補意味IR=候補意味IR,
        模型核=None,
        正式模型評価=False,
        作業再作用=True,
        局所再照合=True,
    )
    if not _承認済み(結果) or 結果.回答ラベル == str(基準ラベル):
        return None
    if "DIRECTED_関係_VERIFIED" not in 結果.理由:
        return None
    K3結果 = 結果.K3結果
    if K3結果 is None or int(getattr(K3結果, "根拠事実数", 0)) < 最小独立証拠数:
        return None
    return 結果


__all__ = [
    "HDS既存能力結果証明済み",
    "HDS既存能力選択評価",
    "HDS既存能力直接反証評価",
]

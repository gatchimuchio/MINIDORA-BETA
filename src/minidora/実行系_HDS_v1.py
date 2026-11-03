from __future__ import annotations

from dataclasses import replace

from .HDS参照 import HDS参照予算選択, HDS参照検索
from .参照 import 参照取得診断
from .HDS観測計画 import HDS参照観測要求群
from .HDS実行主体 import HDS終端
from .HDS選択実行系 import HDS選択実行結果, HDS選択問題
from .HDS選択継承循環 import 回答成果名, 参照成果名, 現行結果成果名
from .HDS駆動コア import HDS駆動コア, HDS駆動コア版, HDS継承基準版
from .実行系 import ミニドラ as _MINIDORAV05
from .実行系_v03 import 結果, 要求


def _選択肢(中間表現) -> tuple[str, ...]:
    行群 = [
        (座標.座標ID.split(":", 1)[1], str(座標.内容))
        for 座標 in 中間表現.座標
        if 座標.座標ID.startswith("選択肢:")
    ]
    return tuple(値 for _, 値 in sorted(行群, key=lambda 行: 行[0]))


def _作用履歴(実行結果) -> tuple[str, ...]:
    return tuple(記録.作用ID for 記録 in 実行結果.履歴)


class HDS駆動ミニドラ(_MINIDORAV05):
    """HDS-first Core v5を選択問題の実運用入口へ接続するMINIDORA実行系。

    MINIDORA30系のformal模型核と、更新前HDS-MINIDORAの追加参照・計算・再評価・
    非退行下限を `HDS駆動コア.選択実行` の通常循環として継承する。
    """

    版 = "v2-hds-first-core-inheritance"

    def _初期参照(self, 中間表現) -> tuple[tuple[object, ...], tuple[参照取得診断, ...]]:
        if self.参照供給器 is None:
            return (), ()
        予算 = HDS参照予算選択(中間表現)
        診断群: list[参照取得診断] = []
        記録群 = HDS参照検索(
            self.参照供給器,
            中間表現,
            上限=予算.取得上限,
            一問合せ上限=予算.一問合せ上限,
            最大問合せ並列=予算.最大問合せ並列,
            観測要求=HDS参照観測要求群(中間表現),
            診断収集=診断群,
        )
        return tuple(記録群), tuple(診断群)

    def 実行(self, 要求_: 要求) -> 結果:
        if 要求_.手順 is not None or self.HDSコンパイラ is None:
            return super().実行(要求_)
        try:
            中間表現 = self.コンパイル(要求_.問合せ)
        except (ValueError, TypeError):
            return super().実行(要求_)
        if not HDS選択問題(中間表現):
            return super().実行(要求_)

        選択肢 = _選択肢(中間表現)
        if len(選択肢) < 2:
            return super().実行(要求_)

        初期参照, 初期参照診断 = self._初期参照(中間表現)
        コア = HDS駆動コア(
            HDSコンパイラ=self.HDSコンパイラ,
            最大作用回数=32,
        )
        駆動結果 = コア.選択実行(
            要求_.問合せ,
            選択肢,
            初期参照=初期参照,
            初期参照診断=初期参照診断,
            参照供給器=self.参照供給器,
            計算実行器_=self.計算実行器,
            模型核=self.能力模型核,
            最大回復回数=6,
        )

        成果 = 駆動結果.状態.成果辞書()
        選択結果 = 成果.get(現行結果成果名)
        if not isinstance(選択結果, HDS選択実行結果):
            raise RuntimeError("HDS-first Coreが選択評価結果を帰還しなかった")

        最終参照 = 成果.get(参照成果名, 初期参照)
        if not isinstance(最終参照, tuple):
            raise TypeError("HDS-first Coreの参照成果はtupleである必要がある")

        コア理由 = tuple(dict.fromkeys((*tuple(選択結果.理由), *tuple(駆動結果.理由))))
        if 駆動結果.終端 == HDS終端.採用:
            回答ラベル = 成果.get(回答成果名)
            if 回答ラベル is None or 選択結果.回答ラベル != 回答ラベル:
                raise RuntimeError("HDS-first CoreのCOMMIT回答と選択評価結果が一致しない")
            選択結果 = replace(
                選択結果,
                状態="APPROVE",
                理由=tuple(dict.fromkeys((*コア理由, "HDS_FIRST_CORE_COMMIT"))),
            )
        else:
            終端理由 = (
                "HDS_FIRST_CORE_FAIL" if 駆動結果.終端 == HDS終端.失敗
                else "HDS_FIRST_CORE_SUSPEND"
            )
            選択結果 = replace(
                選択結果,
                状態="SUSPEND",
                回答ラベル=None,
                回答内容=None,
                理由=tuple(dict.fromkeys((*コア理由, 終端理由))),
            )

        出力結果 = self._HDS選択結果(要求_, 中間表現, 最終参照, 選択結果)
        状態 = dict(出力結果.状態)
        状態["HDS駆動コアRun"] = {
            "終端": 駆動結果.終端.value,
            "コア版": HDS駆動コア版,
            "継承基準": HDS継承基準版,
            "状態版": 駆動結果.状態.版,
            "参照数": len(最終参照),
            "初期参照診断": tuple((x.供給器, x.状態, x.取得件数, x.試行回数) for x in 初期参照診断),
            "成立状態": tuple(sorted(駆動結果.状態.成立状態)),
            "残差": tuple(sorted(駆動結果.状態.残差)),
            "作用履歴": _作用履歴(駆動結果),
            "理由": tuple(駆動結果.理由),
        }
        履歴 = 出力結果.履歴 + ({
            "op": "HDS_FIRST_CORE_INHERITANCE_RUN",
            "終端": 駆動結果.終端.value,
            "コア版": HDS駆動コア版,
            "継承基準": HDS継承基準版,
            "作用履歴": _作用履歴(駆動結果),
        },)
        return replace(出力結果, 状態=状態, 履歴=履歴)


__all__ = ["HDS駆動ミニドラ"]

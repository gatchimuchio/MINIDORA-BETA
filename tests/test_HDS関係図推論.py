from __future__ import annotations

import unittest

from minidora.HDS資料K import HDSIR知識適合器
from minidora.HDS関係図推論 import HDS意味関係図索引構築, HDS意味経路探索
from minidora.HDS中間表現 import HDSIR, HDS実行核, HDS座標, HDS関係, HDS残差
from minidora.K3機能 import K3相当能力核
from minidora.意味字句 import 意味語


def _関係中間表現(target: str, residuals: tuple[HDS残差, ...] = ()) -> HDSIR:
    return HDSIR(
        原文=f"Alpha relates to {target}.",
        正規化文=f"Alpha relates to {target}.",
        認知世界ID='関係図-cycle-test',
        座標=(
            HDS座標("alpha", "対象.実体", "Alpha", 原文範囲=(0, 5)),
            HDS座標("target", "対象.実体", target, 原文範囲=(18, 18 + len(target))),
        ),
        関係=(HDS関係("r", ("alpha",), ("target",), "作用"),),
        残差=residuals,
        意味作用履歴=(),
        実行核=HDS実行核("意味構造転送"),
        種別="意味構造",
        閉包状態='CLOSED_FOR_意味_TRANSFER',
        入力言語="en",
    )


class HDS関係図単純路試験(unittest.TestCase):
    def test_直接到達を往復循環で水増ししない(self) -> None:
        模型核 = K3相当能力核()
        適合器 = HDSIR知識適合器(模型核)
        適合器.投入(_関係中間表現("engine"), provenance=("fixture", "doc:engine"))
        適合器.投入(_関係中間表現("stone"), provenance=("fixture", "doc:stone"))

        結果 = HDS意味経路探索(模型核, 意味語("Alpha"), 意味語("stone"), 最大深さ=4)
        self.assertGreater(結果.得点, 0.0)
        self.assertEqual(結果.深さ, 1)
        self.assertEqual(len(結果.事実ID), 1)

    def test_正当な多段単純路は維持する(self) -> None:
        模型核 = K3相当能力核()
        適合器 = HDSIR知識適合器(模型核)
        適合器.投入(_関係中間表現("Beta"), provenance=("fixture", "doc:1"))
        second = HDSIR(
            原文="Beta relates to Engine.",
            正規化文="Beta relates to Engine.",
            認知世界ID='関係図-cycle-test',
            座標=(
                HDS座標("beta", "対象.実体", "Beta", 原文範囲=(0, 4)),
                HDS座標("engine", "対象.実体", "Engine", 原文範囲=(16, 22)),
            ),
            関係=(HDS関係("r2", ("beta",), ("engine",), "因果"),),
            残差=(),
            意味作用履歴=(),
            実行核=HDS実行核("意味構造転送"),
            種別="意味構造",
            閉包状態='CLOSED_FOR_意味_TRANSFER',
            入力言語="en",
        )
        適合器.投入(second, provenance=("fixture", "doc:2"))

        結果 = HDS意味経路探索(模型核, 意味語("Alpha"), 意味語("Engine"), 最大深さ=4)
        self.assertGreater(結果.得点, 0.0)
        self.assertEqual(結果.深さ, 2)
        self.assertGreaterEqual(len(結果.事実ID), 2)

    def test_関係図索引はK更新まで再利用し投入後に無効化する(self) -> None:
        模型核 = K3相当能力核()
        適合器 = HDSIR知識適合器(模型核)
        適合器.投入(_関係中間表現("engine"), provenance=("fixture", "doc:1"))

        first = HDS意味関係図索引構築(模型核)
        second = HDS意味関係図索引構築(模型核)
        self.assertIs(first, second)

        適合器.投入(_関係中間表現("stone"), provenance=("fixture", "doc:2"))
        third = HDS意味関係図索引構築(模型核)
        self.assertIsNot(first, third)
        self.assertGreater(third.revision, first.revision)
        self.assertGreaterEqual(third.関係Fact数, first.関係Fact数)

    def test_残差情報源と健全情報源の投入順で関係図可用性が変わらない(self) -> None:
        blocked = _関係中間表現(
            "engine",
            (HDS残差("loss", '意味_loss', "Alpha relates to engine", "意味損失"),),
        )
        good = _関係中間表現("engine")

        for order in ((blocked, good), (good, blocked)):
            模型核 = K3相当能力核()
            適合器 = HDSIR知識適合器(模型核)
            適合器.投入(order[0], provenance=("fixture", "doc:first"))
            適合器.投入(order[1], provenance=("fixture", "doc:second"))

            関係図 = HDS意味関係図索引構築(模型核)
            結果 = HDS意味経路探索(
                模型核,
                意味語("Alpha"),
                意味語("engine"),
                最大深さ=2,
                索引=関係図,
            )
            self.assertGreater(結果.得点, 0.0)
            self.assertEqual(結果.深さ, 1)
            self.assertEqual(関係図.関係Fact数, 1)


if __name__ == "__main__":
    unittest.main()

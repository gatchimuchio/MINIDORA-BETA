from __future__ import annotations

import unittest

from minidora.HDS資料K import HDSIR知識適合器, HDS証拠事実, HDS証拠状態複製
from minidora.HDS関係図推論 import HDS意味関係図索引構築
from minidora.HDS中間表現 import HDSIR, HDS実行核, HDS座標, HDS関係
from minidora.K3機能 import K3相当能力核


def _ir() -> HDSIR:
    return HDSIR(
        原文="Alpha uses engine.",
        正規化文="Alpha uses engine.",
        認知世界ID="clone-test",
        座標=(
            HDS座標("alpha", "対象.実体", "Alpha"),
            HDS座標("engine", "対象.実体", "engine"),
        ),
        関係=(HDS関係("r", ("alpha",), ("engine",), "作用"),),
        残差=(),
        意味作用履歴=(),
        実行核=HDS実行核("意味構造転送"),
    )


class HDS証拠Clone試験(unittest.TestCase):
    def test_K3clone後も独立情報源台帳を明示複製できる(self) -> None:
        情報源 = K3相当能力核()
        適合器 = HDSIR知識適合器(情報源)
        適合器.投入(_ir(), provenance=("fixture", "doc:1"))
        適合器.投入(_ir(), provenance=("fixture", "doc:2"))
        情報源_証拠 = HDS証拠事実(情報源)
        self.assertGreater(len(情報源_証拠), 0)

        cloned = 情報源.clone()
        self.assertEqual(HDS証拠事実(cloned), ())
        HDS証拠状態複製(情報源, cloned)

        cloned_証拠 = HDS証拠事実(cloned)
        self.assertEqual(len(cloned_証拠), len(情報源_証拠))
        self.assertIsNot(cloned.K, 情報源.K)
        self.assertEqual(
            {fact.fact_id for fact in cloned_証拠},
            {fact.fact_id for fact in 情報源_証拠},
        )

    def test_複製先関係図索引は共有せず再構築する(self) -> None:
        情報源 = K3相当能力核()
        HDSIR知識適合器(情報源).投入(_ir(), provenance=("fixture", "doc:1"))
        情報源_index = HDS意味関係図索引構築(情報源)

        cloned = 情報源.clone()
        HDS証拠状態複製(情報源, cloned)
        cloned_index = HDS意味関係図索引構築(cloned)

        self.assertIsNot(情報源_index, cloned_index)
        self.assertEqual(情報源_index.revision, cloned_index.revision)
        self.assertEqual(情報源_index.関係Fact数, cloned_index.関係Fact数)


if __name__ == "__main__":
    unittest.main()

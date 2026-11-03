from __future__ import annotations

import unittest

from minidora.HDS資料K import HDSIR知識適合器, HDS証拠事実
from minidora.HDS関係図推論 import HDS意味関係図索引構築
from minidora.HDS中間表現 import HDSIR, HDS実行核, HDS座標, HDS関係
from minidora.K3機能 import K3相当能力核


class HDS関係ラベル往復試験(unittest.TestCase):
    def test_関係記号をsanitizeで潰さずKへ保持する(self) -> None:
        ir = HDSIR(
            原文="A describes B.",
            正規化文="A describes B.",
            認知世界ID='関係-往復',
            座標=(
                HDS座標("a", "対象.実体", "A"),
                HDS座標("b", "対象.実体", "B"),
            ),
            関係=(HDS関係("r", ("a",), ("b",), "記述→問い"),),
            残差=(),
            意味作用履歴=(),
            実行核=HDS実行核("意味構造転送"),
            種別="意味構造",
            閉包状態='CLOSED_FOR_意味_TRANSFER',
        )
        模型核 = K3相当能力核()
        HDSIR知識適合器(模型核).投入(ir, provenance=("fixture", "doc:1"))

        関係_facts = [fact for fact in HDS証拠事実(模型核) if fact.predicate.startswith('hds_関係_')]
        self.assertEqual(len(関係_facts), 1)
        self.assertEqual(関係_facts[0].predicate, 'hds_関係_記述→問い')

        関係図 = HDS意味関係図索引構築(模型核)
        関係_names = {edge.関係 for edges in 関係図.隣接.values() for edge in edges}
        self.assertIn("記述→問い", 関係_names)
        self.assertNotIn("記述 問い", 関係_names)


if __name__ == "__main__":
    unittest.main()

from __future__ import annotations

import unittest

from minidora.HDS資料K import HDSIR知識適合器, HDS証拠事実
from minidora.HDS中間表現 import HDSIR, HDS実行核, HDS座標, HDS関係, 値状態
from minidora.K3機能 import K3相当能力核


def _ir(状態: 値状態 = 値状態.確定) -> HDSIR:
    return HDSIR(
        原文="Alpha uses engine.",
        正規化文="Alpha uses engine.",
        認知世界ID='情報源-信頼度-test',
        座標=(
            HDS座標("alpha", "対象.実体", "Alpha", 値状態=状態),
            HDS座標("engine", "対象.実体", "engine", 値状態=状態),
        ),
        関係=(HDS関係("r", ("alpha",), ("engine",), "作用", 値状態=状態),),
        残差=(),
        意味作用履歴=(),
        実行核=HDS実行核("意味構造転送"),
    )


class HDS情報源信頼度試験(unittest.TestCase):
    def test_R信頼係数をHDS値状態信頼度と乗算する(self) -> None:
        模型核 = K3相当能力核()
        結果 = HDSIR知識適合器(模型核).投入(
            _ir(値状態.推定),
            provenance=("fixture", "doc:1"),
            信頼係数=0.5,
        )
        facts = [fact for fact in HDS証拠事実(模型核) if fact.predicate != 'hds_残差']
        self.assertTrue(facts)
        self.assertTrue(all(abs(fact.信頼度 - 0.43) < 1e-9 for fact in facts))
        self.assertEqual(結果.情報源_信頼度, 0.5)
        self.assertTrue(all('情報源_信頼度:0.500000' in fact.provenance for fact in facts))

    def test_R信頼係数は0から1へclampする(self) -> None:
        high = K3相当能力核()
        HDSIR知識適合器(high).投入(_ir(), provenance=("fixture", "high"), 信頼係数=2.0)
        self.assertTrue(all(fact.信頼度 <= 1.0 for fact in HDS証拠事実(high)))

        low = K3相当能力核()
        HDSIR知識適合器(low).投入(_ir(), provenance=("fixture", "low"), 信頼係数=-1.0)
        self.assertTrue(all(fact.信頼度 == 0.0 for fact in HDS証拠事実(low)))


if __name__ == "__main__":
    unittest.main()

from __future__ import annotations

import unittest

from minidora import HDS選択実行系
from minidora.HDS選択実行系 import _参照provenance
from minidora.HDS資料K import HDSIR知識適合器, HDS証拠事実
from minidora.HDS中間表現 import HDSIR, HDS実行核, HDS座標, HDS関係
from minidora.K3機能 import K3相当能力核
from minidora.参照 import 参照記録


def _record(identifier: str, content: str, label: str) -> 参照記録:
    return 参照記録(
        identifier,
        "ProteinX",
        content,
        f"fixture://{identifier}",
        "fixture-R",
        条件=(("hds_query_kind", '選択肢'), ('hds_query_選択肢', label)),
    )


def _資料() -> HDSIR:
    return HDSIR(
        原文="ProteinX performs catalysis.",
        正規化文="ProteinX performs catalysis.",
        認知世界ID='経路-境界-test',
        座標=(
            HDS座標("s", "対象.始点", "ProteinX"),
            HDS座標("o", "対象.終点", "catalysis"),
        ),
        関係=(HDS関係("r", ("s",), ("o",), "機能"),),
        残差=(),
        意味作用履歴=(),
        実行核=HDS実行核(),
        入力言語="en",
    )


class HDS検索経路境界試験(unittest.TestCase):
    def test_query種別と候補はprovenanceへだけ保持する(self) -> None:
        record = _record("cat-1", "ProteinX catalysis is experimentally observed.", "A")
        provenance = _参照provenance(record)
        self.assertIn('query_kind:選択肢', provenance)
        self.assertIn('query_選択肢:A', provenance)

    def test_query経路から擬似K証拠を生成する関数を持たない(self) -> None:
        self.assertFalse(hasattr(HDS選択実行系, "_検索経路証拠"))

    def test_query候補ラベルはFact引数へ混入しない(self) -> None:
        record = _record("cat-1", "ProteinX catalysis is experimentally observed.", "A")
        模型核 = K3相当能力核()
        HDSIR知識適合器(模型核).投入(_資料(), provenance=_参照provenance(record))
        facts = HDS証拠事実(模型核)
        self.assertTrue(facts)
        self.assertTrue(any('query_選択肢:A' in tuple(str(x) for x in fact.provenance) for fact in facts))
        self.assertFalse(any("A" in tuple(str(x) for x in fact.args) for fact in facts))


if __name__ == "__main__":
    unittest.main()

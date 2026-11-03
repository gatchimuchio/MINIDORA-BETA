from __future__ import annotations

import unittest

from minidora.HDS資料K import HDSIR知識適合器, HDS証拠事実
from minidora.HDS中間表現 import HDSIR, HDS実行核, HDS座標, HDS関係, HDS残差
from minidora.K3機能 import K3相当能力核


class HDS資料K試験(unittest.TestCase):
    def test_取得資料はHDS_IRから構造FactとしてKへ入る(self) -> None:
        ir = HDSIR(
            原文="Foo binds Bar when Baz is absent.",
            正規化文="Foo binds Bar when Baz is absent.",
            認知世界ID="cw:test",
            座標=(
                HDS座標("src", '情報源_text', "Foo binds Bar when Baz is absent."),
                HDS座標("raw", "対象.原文保持", "Foo binds Bar when Baz is absent."),
                HDS座標("foo", "対象.実体", "Foo"),
                HDS座標("bar", "対象.実体", "Bar"),
                HDS座標("baz", "対象.実体", "Baz is absent"),
            ),
            関係=(
                HDS関係("r1", ("foo",), ("bar",), "作用"),
                HDS関係("r2", ("baz",), ("foo", "bar"), "条件"),
            ),
            残差=(),
            意味作用履歴=(),
            実行核=HDS実行核(),
            種別="意味構造",
            閉包状態='CLOSED_FOR_意味_TRANSFER',
            入力言語="en",
        )
        模型核 = K3相当能力核()
        結果 = HDSIR知識適合器(模型核).投入(ir, provenance=("web", "https://example.test"))

        self.assertEqual(結果.関係事実数, 2)
        self.assertEqual(結果.座標事実数, 3)
        self.assertGreaterEqual(結果.追加事実数, 5)
        self.assertGreaterEqual(結果.証拠事実数, 5)
        facts = tuple(模型核.K._facts.values())
        self.assertTrue(any(f.predicate == 'hds_関係_条件' and "Baz is absent" in f.args for f in facts))
        self.assertTrue(any(f.predicate == 'hds_関係_作用' and "Foo" in f.args and "Bar" in f.args for f in facts))
        self.assertFalse(any(f.predicate == "retrieved_document" for f in facts))
        self.assertFalse(any(f.predicate == "hds_coordinate" and f.args[0] in {'情報源_text', "対象.原文保持"} for f in facts))

    def test_同一意味Factでも独立情報源証拠を潰さない(self) -> None:
        ir = HDSIR(
            原文="Alpha is catalytic.",
            正規化文="Alpha is catalytic.",
            認知世界ID="cw:test",
            座標=(HDS座標("alpha", "対象.実体", "Alpha"),),
            関係=(),
            残差=(),
            意味作用履歴=(),
            実行核=HDS実行核(),
            種別="意味構造",
            閉包状態='CLOSED_FOR_意味_TRANSFER',
            入力言語="en",
        )
        模型核 = K3相当能力核()
        適合器 = HDSIR知識適合器(模型核)
        適合器.投入(ir, provenance=("web", "doc:1"))
        適合器.投入(ir, provenance=("web", "doc:2"))

        # Kの意味Factはcanonical化したまま、証拠台帳だけsource別に保持する。
        self.assertEqual(len(模型核.K.find("hds_coordinate", ("対象.実体", "Alpha"))), 1)
        証拠 = [f for f in HDS証拠事実(模型核) if f.predicate == "hds_coordinate"]
        self.assertEqual(len(証拠), 2)
        self.assertEqual(len({f.fact_id for f in 証拠}), 2)
        self.assertTrue(any("doc:1" in f.provenance for f in 証拠))
        self.assertTrue(any("doc:2" in f.provenance for f in 証拠))

    def test_残差も捨てずKへ保持する(self) -> None:
        ir = HDSIR(
            原文="It changes.",
            正規化文="It changes.",
            認知世界ID="cw:test",
            座標=(HDS座標("src", "対象.原文保持", "It changes."),),
            関係=(),
            残差=(HDS残差("res:1", "coreference_unresolved", "It", "参照先未確定"),),
            意味作用履歴=(),
            実行核=HDS実行核(),
            種別="意味構造",
            閉包状態="PARTIALLY_CLOSED",
            入力言語="en",
        )
        模型核 = K3相当能力核()
        結果 = HDSIR知識適合器(模型核).投入(ir)
        self.assertEqual(結果.残差数, 1)
        self.assertTrue(模型核.K.find('hds_残差'))
        self.assertFalse(模型核.K.find("hds_coordinate"))


if __name__ == "__main__":
    unittest.main()

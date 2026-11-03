from __future__ import annotations

import unittest

from minidora.HDS候補再照合 import HDS候補証拠, HDS候補横断調停


class HDS候補横断調停試験(unittest.TestCase):
    def test_同一情報源のfactとdocumentを二重加点しない(self) -> None:
        結果 = HDS候補横断調停(
            ("A", "B"),
            (
                HDS候補証拠("A", "doc:1", 5.0, ("f1",), "fact"),
                HDS候補証拠("A", "doc:1", 4.5, ("f1", "f2"), "document"),
            ),
            証拠重み=(1.0, 0.5, 0.25),
            証拠上限=3,
        )
        self.assertEqual(結果["A"].独立出典数, 1)
        self.assertEqual(len(結果["A"].採用証拠), 1)
        self.assertAlmostEqual(結果["A"].合計得点, 5.0)
        self.assertEqual(結果["A"].採用証拠[0].経路, "fact")

    def test_同一情報源では高得点documentよりatomic_factを優先する(self) -> None:
        結果 = HDS候補横断調停(
            ("A", "B"),
            (
                HDS候補証拠("A", "doc:1", 3.0, ('関係-fact',), "fact"),
                HDS候補証拠("A", "doc:1", 9.0, ("f1", "f2", "f3"), "document"),
            ),
            証拠重み=(1.0,),
            証拠上限=1,
        )
        self.assertEqual(結果["A"].採用証拠[0].経路, "fact")
        self.assertEqual(結果["A"].採用証拠[0].事実ID, ('関係-fact',))
        self.assertAlmostEqual(結果["A"].合計得点, 3.0)

    def test_directは高得点factより優先する(self) -> None:
        結果 = HDS候補横断調停(
            ("A", "B"),
            (
                HDS候補証拠("A", "doc:1", 2.0, ("direct",), "direct"),
                HDS候補証拠("A", "doc:1", 8.0, ("fact",), "fact"),
            ),
            証拠重み=(1.0,),
            証拠上限=1,
        )
        self.assertEqual(結果["A"].採用証拠[0].経路, "direct")
        self.assertAlmostEqual(結果["A"].合計得点, 2.0)

    def test_同じ粒度のfact同士は高得点を使う(self) -> None:
        結果 = HDS候補横断調停(
            ("A", "B"),
            (
                HDS候補証拠("A", "doc:1", 3.0, ("f-low",), "fact"),
                HDS候補証拠("A", "doc:1", 5.0, ("f-high",), "fact"),
            ),
            証拠重み=(1.0,),
            証拠上限=1,
        )
        self.assertEqual(結果["A"].採用証拠[0].事実ID, ("f-high",))
        self.assertAlmostEqual(結果["A"].合計得点, 5.0)

    def test_全候補共通情報源はmarginとprovenanceへ残さない(self) -> None:
        結果 = HDS候補横断調停(
            ("A", "B"),
            (
                HDS候補証拠("A", "common", 4.0, ("a-common",), "fact"),
                HDS候補証拠("B", "common", 4.0, ("b-common",), "fact"),
                HDS候補証拠("A", "exclusive-a", 3.0, ("a-only",), "fact"),
            ),
            証拠重み=(1.0, 0.5),
            証拠上限=2,
        )
        self.assertFalse(any(x.出典ID == "common" for x in 結果["A"].採用証拠))
        self.assertFalse(any(x.出典ID == "common" for x in 結果["B"].採用証拠))
        self.assertEqual(結果["A"].独立出典数, 1)
        self.assertEqual(結果["B"].独立出典数, 0)
        self.assertAlmostEqual(結果["A"].合計得点, 3.0)
        self.assertEqual(結果["B"].合計得点, 0.0)

    def test_僅差の共通情報源は相対差だけを残す(self) -> None:
        結果 = HDS候補横断調停(
            ("A", "B", "C", "D"),
            (
                HDS候補証拠("A", "common", 10.0, ("a",), "fact"),
                HDS候補証拠("B", "common", 9.8, ("b",), "fact"),
                HDS候補証拠("C", "common", 9.7, ("c",), "fact"),
                HDS候補証拠("D", "common", 9.6, ("d",), "fact"),
                HDS候補証拠("D", "exclusive-d", 3.0, ("d-only",), "fact"),
            ),
            証拠重み=(1.0, 0.5),
            証拠上限=2,
        )
        common_a = next(x for x in 結果["A"].採用証拠 if x.出典ID == "common")
        self.assertGreater(common_a.識別係数, 0.0)
        self.assertLess(common_a.識別係数, 0.02)
        self.assertEqual(結果["B"].合計得点, 0.0)
        self.assertEqual(結果["C"].合計得点, 0.0)
        self.assertGreater(結果["D"].合計得点, 結果["A"].合計得点)

    def test_独立情報源は別々に加点する(self) -> None:
        結果 = HDS候補横断調停(
            ("A", "B"),
            (
                HDS候補証拠("A", "doc:1", 4.0, ("f1",), "fact"),
                HDS候補証拠("A", "doc:2", 3.0, ("f2",), "fact"),
            ),
            証拠重み=(1.0, 0.5),
            証拠上限=2,
        )
        self.assertEqual(結果["A"].独立出典数, 2)
        self.assertAlmostEqual(結果["A"].合計得点, 5.5)
        self.assertEqual(結果["B"].合計得点, 0.0)


if __name__ == "__main__":
    unittest.main()

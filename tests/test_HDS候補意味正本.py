from __future__ import annotations

import unittest

from minidora.HDS候補意味正本 import HDS候補意味IR辞書
from minidora.HDS構文化器_v1 import 公開HDSコンパイラ
from minidora.HDS中間表現 import HDSIR


class HDS候補意味正本試験(unittest.TestCase):
    def test_問題束が全候補意味IRを一度形成して保持する(self) -> None:
        構文化器 = 公開HDSコンパイラ()
        束 = 構文化器.問題コンパイル束(
            "Which molecule inhibits Enzyme X?",
            ("Molecule A", "Molecule B", "Molecule C", "Molecule D"),
        )
        self.assertEqual(tuple(束.候補意味IR辞書), tuple("ABCD"))
        self.assertTrue(all(isinstance(x, HDSIR) for x in 束.候補意味IR辞書.values()))
        self.assertEqual(
            tuple(ir.原文 for ir in 束.候補意味IR辞書.values()),
            ("Molecule A", "Molecule B", "Molecule C", "Molecule D"),
        )

    def test_候補意味正本は欠落や重複を補完しない(self) -> None:
        構文化器 = 公開HDSコンパイラ()
        a = 構文化器.意味コンパイル("A")
        b = 構文化器.意味コンパイル("B")
        self.assertEqual(tuple(HDS候補意味IR辞書({"B": b, "A": a}, ("A", "B"))), ("A", "B"))
        with self.assertRaises(ValueError):
            HDS候補意味IR辞書({"A": a}, ("A", "B"))
        with self.assertRaises(ValueError):
            HDS候補意味IR辞書((("A", a), ("A", b)), ("A",))


if __name__ == "__main__":
    unittest.main()

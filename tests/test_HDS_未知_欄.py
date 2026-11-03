from __future__ import annotations

import unittest

from minidora.HDS構文化器 import 公開HDSコンパイラ
from minidora.HDS中間表現 import 値状態
from minidora.HDS参照 import HDS参照問合せ候補


class HDS不足スロット試験(unittest.TestCase):
    def setUp(self) -> None:
        self.構文化器 = 公開HDSコンパイラ()

    def test_英語未知始点を候補へ置換したR_queryにする(self) -> None:
        ir = self.構文化器.問題IR(
            "Which molecule causes apoptosis under hypoxia?",
            ("Protein A", "Protein B", "Protein C", "Protein D"),
        )
        未知 = [c for c in ir.座標 if c.種別 == "目的.未知始点"]
        self.assertEqual(len(未知), 1)
        self.assertEqual(未知[0].値状態, 値状態.未観測)
        self.assertEqual(str(未知[0].内容).casefold(), "molecule")
        関係 = next(r for r in ir.関係 if r.種別 == "因果" and r.値状態 == 値状態.未観測)
        self.assertIn("不足位置=始点", 関係.条件)
        queries = tuple(q.casefold() for q in HDS参照問合せ候補(ir))
        for 選択肢 in ("protein a", "protein b", "protein c", "protein d"):
            matched = [q for q in queries if 選択肢 in q]
            self.assertEqual(len(matched), 1, queries)
            self.assertIn("apoptosis", matched[0])
            self.assertIn("hypoxia", matched[0])
            self.assertIn("cause", matched[0])

    def test_英語未知終点を候補へ置換する(self) -> None:
        ir = self.構文化器.問題IR(
            "Protein A inhibits which pathway under hypoxia?",
            ("glycolysis", "apoptosis", "translation", "transport"),
        )
        未知 = [c for c in ir.座標 if c.種別 == "目的.未知終点"]
        self.assertEqual(len(未知), 1)
        self.assertEqual(str(未知[0].内容).casefold(), "pathway")
        queries = tuple(q.casefold() for q in HDS参照問合せ候補(ir))
        self.assertTrue(any(
            "protein a" in q and "glycolysis" in q and "hypoxia" in q and "inhibit" in q
            for q in queries
        ), queries)

    def test_受動態でも意味方向に沿って未知終点を作る(self) -> None:
        ir = self.構文化器.問題IR(
            "Which disease is caused by Protein A?",
            ("Disease A", "Disease B", "Disease C", "Disease D"),
        )
        関係 = next(r for r in ir.関係 if r.種別 == "因果")
        coords = ir.座標辞書()
        self.assertEqual(str(coords[関係.始点[0]].内容), "Protein A")
        self.assertEqual(coords[関係.終点[0]].種別, "目的.未知終点")
        queries = tuple(q.casefold() for q in HDS参照問合せ候補(ir))
        self.assertTrue(any(
            "protein a" in q and "disease a" in q and "cause" in q
            for q in queries
        ), queries)

    def test_日本語未知始点も同じ意味構造へ落とす(self) -> None:
        ir = self.構文化器.問題IR(
            "どのタンパク質がアポトーシスを引き起こす？",
            ("タンパク質A", "タンパク質B", "タンパク質C", "タンパク質D"),
        )
        未知 = [c for c in ir.座標 if c.種別 == "目的.未知始点"]
        self.assertEqual(len(未知), 1)
        self.assertEqual(str(未知[0].内容), "タンパク質")
        queries = HDS参照問合せ候補(ir)
        self.assertIn("タンパク質A 引き起こす アポトーシス", queries)

    def test_関係が確定しない疑問文へ不足スロットを捏造しない(self) -> None:
        ir = self.構文化器.問題IR(
            "Which of the following statements is correct under hypoxia?",
            ("A statement", "B statement", "C statement", "D statement"),
        )
        generic_kinds = {"問い適合", "命題適合", "説明適合"}
        concrete_missing = [
            r for r in ir.関係
            if r.種別 not in generic_kinds and any("不足位置=" in cond for cond in r.条件)
        ]
        self.assertEqual(concrete_missing, [])
        generic = [r for r in ir.関係 if r.種別 in generic_kinds]
        self.assertTrue(generic)

    def test_選択極性は外部検索語へ漏らさない(self) -> None:
        ir = self.構文化器.問題IR(
            "Which molecule is least likely to cause apoptosis under hypoxia?",
            ("Protein A", "Protein B", "Protein C", "Protein D"),
        )
        queries = tuple(q.casefold() for q in HDS参照問合せ候補(ir))
        self.assertFalse(any("least likely" in q for q in queries if "protein " in q))
        self.assertFalse(any("始点" in q or "終点" in q for q in queries))


if __name__ == "__main__":
    unittest.main()

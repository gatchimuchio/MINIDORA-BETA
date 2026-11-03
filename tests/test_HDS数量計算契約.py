from __future__ import annotations

import unittest

from minidora.HDS構文化器_v1 import 公開HDSコンパイラ
from minidora.HDS観測計画 import HDS追加観測要求群


class HDS数量計算契約試験(unittest.TestCase):
    def setUp(self) -> None:
        self.構文化器 = 公開HDSコンパイラ()

    def test_明示算術は実行可能として固定する(self) -> None:
        束 = self.構文化器.コンパイル束("2+3")
        self.assertEqual(束.数量計算契約.状態, "実行可能")
        self.assertTrue(束.数量計算契約.計算P実行可能)
        self.assertEqual([x.値 for x in 束.数量計算契約.問い数量], ["2", "3"])

    def test_数量候補があって法則が無ければ法則不足を保持する(self) -> None:
        束 = self.構文化器.問題コンパイル束(
            "A state has lifetime 1e-9 s. Which energy difference is resolvable?",
            ("1e-9 eV", "1e-6 eV", "1e-3 eV", "1 eV"),
        )
        契約 = 束.数量計算契約
        self.assertEqual(契約.状態, "法則不足")
        self.assertFalse(契約.計算P実行可能)
        self.assertIn("計算法則未形成", 契約.不足)
        self.assertTrue(any(x.値 == "1e-9" and "s" in x.単位 for x in 契約.問い数量))
        self.assertEqual(set(契約.候補被覆ラベル), set("ABCD"))

    def test_法則不足は問題文そのものを法則観測へ追加する(self) -> None:
        問い = "A state has lifetime 1e-9 s. Which energy difference is resolvable?"
        束 = self.構文化器.問題コンパイル束(
            問い,
            ("1e-9 eV", "1e-6 eV", "1e-3 eV", "1 eV"),
        )
        法則 = [x for x in 束.参照観測要求 if x.ID == "数量法則:0"]
        self.assertEqual(len(法則), 1)
        self.assertEqual(法則[0].外部検索表層, 問い)
        self.assertEqual(法則[0].関係種別, "数量計算法則")
        self.assertEqual(法則[0].段階, "fallback")
        self.assertFalse(法則[0].必須被覆)
        追加 = HDS追加観測要求群(
            束.参照観測要求,
            残差群=("HDS選択:数量法則不足",),
            世代=1,
        )
        self.assertEqual([x.ID for x in 追加], ["数量法則:0"])

    def test_実行可能算術には法則観測を追加しない(self) -> None:
        束 = self.構文化器.コンパイル束("2+3")
        self.assertFalse(any(x.ID == "数量法則:0" for x in 束.参照観測要求))

    def test_数値を含むだけの同定問題は法則不足へ昇格しない(self) -> None:
        束 = self.構文化器.問題コンパイル束(
            "At 403 K, which compound is observed by NMR?",
            ("1,2-dimethylbenzene", "compound B", "compound C", "compound D"),
        )
        self.assertEqual(束.数量計算契約.状態, "数量含有")
        self.assertFalse(any(x.ID == "数量法則:0" for x in 束.参照観測要求))

    def test_反応番号を含む候補も数量回答扱いしない(self) -> None:
        束 = self.構文化器.問題コンパイル束(
            "Which product forms after heating at 350 K?",
            ("product 1", "product 2", "product 3", "product 4"),
        )
        self.assertEqual(束.数量計算契約.状態, "数量含有")
        self.assertFalse(any(x.ID == "数量法則:0" for x in 束.参照観測要求))

    def test_非数量問題は非数量のままにする(self) -> None:
        束 = self.構文化器.問題コンパイル束(
            "Which molecule inhibits Enzyme X?",
            ("Protein A", "Protein B", "Protein C", "Protein D"),
        )
        self.assertEqual(束.数量計算契約.状態, "非数量")
        self.assertFalse(束.数量計算契約.数量問題)


if __name__ == "__main__":
    unittest.main()

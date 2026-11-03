from __future__ import annotations

import unittest

from minidora.HDS構文化器 import 公開HDSコンパイラ
from minidora.HDS参照 import HDS参照問合せ候補


class 公開HDSコンパイラ試験(unittest.TestCase):
    def setUp(self) -> None:
        self.構文化器 = 公開HDSコンパイラ()

    def test_日本語を規定言語とし外部英語は表層だけ保持する(self) -> None:
        ir = self.構文化器.コンパイル("Protein A causes apoptosis.")
        self.assertEqual(self.構文化器.基底言語, "日本語")
        self.assertEqual(self.構文化器.基底言語コード, "ja")
        self.assertEqual(ir.入力言語, "en")
        self.assertIn("因果", {関係.種別 for 関係 in ir.関係})
        self.assertTrue(any(coord.種別 == "関係.述語" and "causes" in str(coord.内容) for coord in ir.座標))

    def test_受動因果を同じ有向関係へ正規化する(self) -> None:
        active = self.構文化器.コンパイル("Protein A causes apoptosis.")
        passive = self.構文化器.コンパイル("Apoptosis is caused by Protein A.")

        def edge(ir):
            関係 = next(item for item in ir.関係 if item.種別 == "因果")
            coords = ir.座標辞書()
            return str(coords[関係.始点[0]].内容), str(coords[関係.終点[0]].内容)

        self.assertEqual(edge(active), ("Protein A", "apoptosis"))
        self.assertEqual(edge(passive), ("Protein A", "Apoptosis"))

    def test_反転選択意図を検索前に保持する(self) -> None:
        ir = self.構文化器.コンパイル("Which mechanism is least likely to increase ATP production?")
        controls = {str(coord.内容) for coord in ir.座標 if coord.種別 == "制御.選択意図"}
        conditions = {str(coord.内容).casefold() for coord in ir.座標 if coord.種別 == "条件.検索極性"}
        self.assertEqual(controls, {"反転"})
        self.assertIn("least likely", conditions)

    def test_数量と単位を分離して関係を保持する(self) -> None:
        ir = self.構文化器.コンパイル("The sample was heated to 37 °C under hypoxia.")
        self.assertTrue(any(coord.種別 == "値.数量" and str(coord.内容) == "37" for coord in ir.座標))
        self.assertTrue(any(coord.種別 == "属性.単位" and "°C" in str(coord.内容) for coord in ir.座標))
        self.assertTrue(any(関係.種別 == "数量単位" for 関係 in ir.関係))
        self.assertTrue(any(coord.種別 == "条件.前提" and "under hypoxia" in str(coord.内容).casefold() for coord in ir.座標))

    def test_十冪科学表記を単一数量へ正規化する(self) -> None:
        ir = self.構文化器.コンパイル("Lifetime is 10^-9 s, another is 2×10^{-8} s, and concentration is 6.3e-7 M.")
        values = [str(x.内容) for x in ir.座標 if x.種別 == "値.数量"]
        self.assertIn("1e-9", values)
        self.assertIn("2e-8", values)
        self.assertIn("6.3e-7", values)
        self.assertNotIn("-9", values)
        coords = ir.座標辞書()
        pairs = {
            str(coords[r.始点[0]].内容): str(coords[r.終点[0]].内容)
            for r in ir.関係 if r.種別 == "数量単位"
        }
        self.assertEqual(pairs["1e-9"], "s")
        self.assertEqual(pairs["2e-8"], "s")
        self.assertEqual(pairs["6.3e-7"], "M")

    def test_R問い合わせへ構造化された焦点が反映される(self) -> None:
        ir = self.構文化器.問題IR(
            "Which molecule causes apoptosis under hypoxia?",
            ("Protein A", "Protein B", "Protein C", "Protein D"),
        )
        queries = HDS参照問合せ候補(ir)
        self.assertGreaterEqual(len(queries), 6)
        lowered = tuple(query.casefold() for query in queries)
        self.assertTrue(any("apoptosis" in query and "hypoxia" in query for query in lowered))
        self.assertTrue(any("cause" in query for query in lowered))
        for 選択肢 in ("protein a", "protein b", "protein c", "protein d"):
            self.assertTrue(any(選択肢 in query for query in lowered))


if __name__ == "__main__":
    unittest.main()

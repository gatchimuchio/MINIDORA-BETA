from __future__ import annotations

import unittest

from minidora.HDS構文化記録 import HDS原理段階
from minidora.HDS構文化器_v1 import 公開HDSコンパイラ
from minidora.HDS資料K import HDSIR知識適合器, HDS証拠事実
from minidora.HDS探索方針 import HDS努力水準
from minidora.HDS参照 import HDS参照問合せ候補
from minidora.K3機能 import K3相当能力核


class HDS構文化器構造V1試験(unittest.TestCase):
    def setUp(self) -> None:
        self.構文化器 = 公開HDSコンパイラ()

    def test_AI世界文で発話主体と作用主体を分離する(self) -> None:
        結果 = self.構文化器.詳細コンパイル("AIが世界を変える。")
        self.assertIn("AI", 結果.認知世界.作用主体)
        self.assertIn("世界", 結果.認知世界.対象)
        for missing in ("発話主体", "時間", "空間", "目的", "機構"):
            self.assertIn(missing, 結果.未固定座標)

    def test_動態を静止命題へ潰さない(self) -> None:
        結果 = self.構文化器.詳細コンパイル("初期状態S0から、条件CならS1へ遷移し、失敗時はrollbackして次状態へ戻す。")
        kinds = {item.種別 for item in 結果.監査項目}
        for expected in ("初期状態", "遷移", "分岐", "帰還"):
            self.assertIn(expected, kinds)
        self.assertIn("可逆性要求", 結果.要求種別)
        self.assertIn("時間帰属要求", 結果.要求種別)

    def test_定義前提射程不確実性を分ける(self) -> None:
        text = "AIとは人工知能を指す。データは固定と仮定する。この条件下のみ有効であり、結果には不確実性がある。"
        結果 = self.構文化器.詳細コンパイル(text)
        kinds = {item.種別 for item in 結果.監査項目}
        for expected in ("定義", "前提", "射程", "不確実性"):
            self.assertIn(expected, kinds)

    def test_可能性は不可能性監査要求へ落とす(self) -> None:
        結果 = self.構文化器.詳細コンパイル("この構成は実現可能である。")
        self.assertIn("不可能性要求", 結果.要求種別)
        request = next(item for item in 結果.監査要求 if item.種別 == "不可能性要求")
        self.assertIn("不可能性証拠", request.必要情報)
        self.assertIn("対称な否定候補", request.必要情報)

    def test_原理語を採用済み原理へ昇格しない(self) -> None:
        結果 = self.構文化器.詳細コンパイル("観測されたパターンから原理候補Pを考える。")
        self.assertEqual(結果.原理探索.段階, HDS原理段階.原理候補)
        self.assertIn("原理探索要求", 結果.要求種別)
        self.assertIn("反証条件", 結果.原理探索.必要監査)
        self.assertNotIn("SCOPED_PRINCIPLE", {str(c.内容) for c in 結果.IR.座標})

    def test_単一因果は原理候補へ自動昇格しない(self) -> None:
        結果 = self.構文化器.詳細コンパイル("Protein A causes apoptosis.")
        self.assertEqual(結果.原理探索.段階, HDS原理段階.影)

    def test_監査メタをR_queryへ漏らさない(self) -> None:
        ir = self.構文化器.問題IR(
            "Which molecule causes apoptosis?",
            ("Protein A", "Protein B", "Protein C", "Protein D"),
        )
        queries = HDS参照問合せ候補(ir)
        joined = " ".join(queries)
        for forbidden in ("監査", "保持", "原則暫定", "最終採否委譲"):
            self.assertNotIn(forbidden, joined)

    def test_監査メタをK_factへ昇格しない(self) -> None:
        模型核 = K3相当能力核()
        ir = self.構文化器.コンパイル("Protein A causes apoptosis.")
        HDSIR知識適合器(模型核).投入(ir, provenance=("fixture",))
        payload = " ".join(str(fact.args) for fact in HDS証拠事実(模型核))
        for forbidden in ("監査.", "保持.", "暫定性."):
            self.assertNotIn(forbidden, payload)

    def test_監査メタだけで努力水準を膨らませない(self) -> None:
        self.assertEqual(HDS努力水準(self.構文化器.コンパイル("2+3")), "low")

    def test_日本語を基底規定言語として維持する(self) -> None:
        結果 = self.構文化器.詳細コンパイル("Protein A causes apoptosis.")
        self.assertEqual(self.構文化器.規定言語, "日本語")
        self.assertEqual(self.構文化器.基底言語, "日本語")
        self.assertEqual(self.構文化器.基底言語コード, "ja")
        self.assertEqual(結果.IR.入力言語, "en")
        self.assertEqual(self.構文化器.構造版, "v1.3")


if __name__ == "__main__":
    unittest.main()

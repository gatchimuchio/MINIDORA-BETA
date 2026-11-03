import unittest
from fractions import Fraction

from minidora import ミニドラ
from minidora.言語確率法則 import MINIDORA厳密言語模型
from minidora.規定参照 import LLM成立規定版, 厳密LM中核, 能力作用観測単位


class MINIDORAv05統合試験(unittest.TestCase):
    def test_実行系は厳密LM核と能力核を分離する(self):
        body = ミニドラ()
        self.assertIsInstance(body.言語模型核, MINIDORA厳密言語模型)
        self.assertIs(body.模型核, body.能力模型核)
        self.assertIsNot(body.言語模型核, body.能力模型核)
        self.assertTrue(body.言語模型監査().合格)

    def test_候補scoreはLM確率として流用されない(self):
        body = ミニドラ()
        probability = body.言語確率("任意")
        ability_結果 = body.言語評価("未知", ("候補A", "候補B"))
        self.assertIsInstance(probability, Fraction)
        self.assertGreater(probability, 0)
        self.assertTrue(hasattr(ability_結果, "候補差"))

    def test_形成済み厳密LMを注入できる(self):
        lm = MINIDORA厳密言語模型.形成(("猫。",) * 10 + ("犬。",), 次数=2)
        body = ミニドラ(言語模型核_=lm)
        self.assertGreater(body.言語確率("猫。"), body.言語確率("犬。"))

    def test_成立規定v8と能力作用観測単位を参照する(self):
        self.assertEqual(LLM成立規定版, "2026-08-28-成立規定-8")
        self.assertEqual(
            厳密LM中核,
            ("完全言語状態空間", "持続模型状態", "整合した言語確率法則", "局所条件から完全法則への接続"),
        )
        self.assertEqual(
            能力作用観測単位,
            ("状態担体", "作用", "状態差", "後続利用", "参照変更", "経路変更", "計算量変更", "再参照", "再結合", "循環尺度"),
        )


if __name__ == "__main__":
    unittest.main()

"""βの三機接続。ここでの人工作用成功を一般能力の証拠へ拡張しない。"""
import unittest

from minidora.HDS実行主体 import HDS関数作用, HDS作用結果, HDS作用状態, HDS終端
from minidora.engine_beta import ミニドラβ実行機


def 作用():
    return HDS関数作用(
        "成果形成",
        lambda 状態: HDS作用結果(
            HDS作用状態.成立,
            追加状態=frozenset({"成果成立"}),
            成果=(("成果", "操作完了"),),
        ),
        出力状態=("成果成立",),
    )


class β三機試験(unittest.TestCase):
    def test_入力駆動出力を一度ずつ通す(self):
        β = ミニドラβ実行機()
        結果 = β.実行(
            "状況を処理", 案件ID="案件A", 入力ID="入力A",
            要求状態=("成果成立",), 追加作用=(作用(),), 成果名="成果",
        )
        self.assertEqual(結果.駆動.終端, HDS終端.採用)
        self.assertEqual(結果.入力.原本.本文, "状況を処理")
        self.assertEqual(結果.出力.状態, "成立")
        self.assertTrue(結果.表現.合格)
        self.assertIn("操作完了", 結果.表現.本文)
        self.assertEqual(β.入力機.変換回数, 1)
        self.assertEqual(β.出力機.計装["表現実行数"], 1)
        self.assertIs(β.結果を参照(案件ID="案件A", 入力ID="入力A"), 結果)

    def test_同じ入力版で駆動を繰り返さない(self):
        β = ミニドラβ実行機()
        条件 = dict(案件ID="案件A", 入力ID="入力A", 要求状態=("成果成立",),
                  追加作用=(作用(),), 成果名="成果")
        β.実行("状況を処理", **条件)
        with self.assertRaisesRegex(ValueError, "再駆動"):
            β.実行("状況を処理", **条件)
        self.assertEqual(β.入力機.変換回数, 1)
        self.assertEqual(β.出力機.計装["表現実行数"], 1)

    def test_重複受付の前で再駆動を拒否する(self):
        β = ミニドラβ実行機()
        β.実行("原文A", 案件ID="案件C", 入力ID="入力C",
             要求状態=("成果成立",), 追加作用=(作用(),), 成果名="成果")
        with self.assertRaisesRegex(ValueError, "再駆動"):
            β.実行("別の原文", 案件ID="案件C", 入力ID="入力C",
                 要求状態=("成果成立",), 追加作用=(作用(),), 成果名="成果")
        self.assertEqual(β.入力機.変換回数, 1)
        self.assertEqual(len(β.入力機.受付記録()), 1)

    def test_未達状態を成功へ昇格しない(self):
        β = ミニドラβ実行機()
        結果 = β.実行(
            "未達の状態", 案件ID="案件B", 入力ID="入力B",
            要求状態=("未成立",), 成果名="成果",
        )
        self.assertNotEqual(結果.駆動.終端, HDS終端.採用)
        self.assertNotEqual(結果.出力.状態, "成立")

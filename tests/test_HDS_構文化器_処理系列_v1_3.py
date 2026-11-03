from __future__ import annotations

import unittest

from minidora.HDS適合器 import HDS独立コンパイル
from minidora.HDS構文化器_v1 import 公開HDSコンパイラ
from minidora.計算実行境界 import 計算実行境界


class HDS構文化器処理系列試験(unittest.TestCase):
    def setUp(self) -> None:
        self.構文化器 = 公開HDSコンパイラ()

    def test_意味監査構造_v1_3とKernel処理系列_v2_0(self) -> None:
        self.assertEqual(self.構文化器.構造版, "v1.3")
        self.assertEqual(self.構文化器.処理系列版, "v2.0")
        self.assertEqual(self.構文化器.規定言語, "日本語")
        self.assertEqual(self.構文化器.基底言語, "日本語")
        self.assertEqual(self.構文化器.基底言語コード, "ja")

    def test_意味コンパイル正本はPと計算初期状態を内包しない(self) -> None:
        ir = self.構文化器.意味コンパイル("2+3")
        self.assertIsNone(ir.手順)
        self.assertEqual(ir.初期状態, {})
        self.assertEqual(ir.種別, "算術")
        self.assertIn("計算P非内包", ir.実行核.検証)
        self.assertEqual(ir.閉包状態, 'CLOSED_FOR_意味_TRANSFER')

    def test_コンパイル束は三成果を別フィールドで保持する(self) -> None:
        bundle = self.構文化器.コンパイル束("2+3")
        self.assertIsNone(bundle.意味IR.手順)
        self.assertTrue(bundle.計算計画.手順.命令列)
        self.assertEqual(bundle.計算計画.種別, "算術")
        self.assertEqual(bundle.計算計画.初期状態, {"入力0": 2, "入力1": 3})
        self.assertEqual(bundle.作用差分構造.作用数, 0)
        self.assertIs(bundle.カーネル正本, bundle)
        self.assertIs(bundle.正本, bundle.コア入力)
        self.assertTrue(bundle.カーネル署名)
        self.assertEqual(bundle.正本.版, "HDS-コア入力-v1")
        self.assertFalse(hasattr(bundle.正本, "計算計画"))
        self.assertIsInstance(bundle.参照観測要求, tuple)

    def test_選択問題束は意味中核Rを同一成果で保持する(self) -> None:
        bundle = self.構文化器.問題コンパイル束(
            "Which molecule causes apoptosis under hypoxia?",
            ("Protein A", "Protein B", "Protein C", "Protein D"),
        )
        self.assertIs(bundle.カーネル正本, bundle)
        self.assertIs(bundle.正本, bundle.コア入力)
        self.assertTrue(bundle.カーネル署名)
        self.assertTrue(bundle.参照観測要求)
        labels = {item.候補ラベル for item in bundle.参照観測要求 if item.必須被覆}
        self.assertEqual(labels, {"A", "B", "C", "D"})

    def test_Kernel束は監査成果を一級成果として保持する(self) -> None:
        bundle = self.構文化器.コンパイル束("この構成は実現可能である。")
        self.assertTrue(bundle.失敗署名候補)
        self.assertTrue(bundle.チェックリスト)
        self.assertTrue(bundle.監査参照候補)
        self.assertTrue(bundle.認知世界差分.現行世界参照)
        self.assertTrue(bundle.カーネル署名)

    def test_詳細問題IRも通常問題IRと同じ問い閉包を観測する(self) -> None:
        detailed = self.構文化器.詳細問題IR(
            "Which statement about entropy is correct?",
            ("A", "B", "C"),
        )
        normal = self.構文化器.問題IR(
            "Which statement about entropy is correct?",
            ("A", "B", "C"),
        )
        self.assertEqual(
            any(item.種別 == "問い適合" for item in detailed.IR.関係),
            any(item.種別 == "問い適合" for item in normal.関係),
        )

    def test_形成済み束の計算降下は自然言語を再解析しない(self) -> None:
        bundle = self.構文化器.コンパイル束("2+3")

        class _再解析禁止:
            def 計画(self, 問合せ):
                raise AssertionError("計算降下で自然言語を再解析した")

        self.構文化器._計算計画器 = _再解析禁止()
        lowered = self.構文化器.計算降下(bundle)
        結果 = 計算実行境界().実行(lowered.計算IR, lowered.初期状態)
        self.assertEqual(結果.出力, 5)
        self.assertIn("自然言語再解析なし", lowered.計算IR.検証)

    def test_計算コンパイルは意味IRを汚さず計算IRを形成する(self) -> None:
        結果 = self.構文化器.計算コンパイル("10-4")
        self.assertIsNone(結果.意味IR.手順)
        self.assertEqual(結果.意味IR.初期状態, {})
        self.assertEqual(結果.種別, "算術")
        executed = 計算実行境界().実行(結果.計算IR, 結果.初期状態)
        self.assertEqual(executed.出力, 6)

    def test_旧コンパイルだけが最外周でPを再付与する(self) -> None:
        意味 = self.構文化器.意味コンパイル("2+3")
        legacy = self.構文化器.コンパイル("2+3")
        self.assertIsNone(意味.手順)
        self.assertIsNotNone(legacy.手順)
        self.assertEqual(legacy.初期状態, {"入力0": 2, "入力1": 3})
        self.assertTrue(legacy.実行可能)
        self.assertIn("互換橋", legacy.実行核.境界)
        self.assertFalse(any(str(x.種別).startswith(("監査.", "保持.", "暫定性.", "帰還.")) for x in legacy.座標))
        カーネル意味座標 = tuple(
            x for x in 意味.座標
            if not str(x.種別).startswith(("監査.", "保持.", "暫定性.", "帰還."))
        )
        self.assertEqual(カーネル意味座標, legacy.座標)
        self.assertTrue(set(legacy.関係).issubset(set(意味.関係)))

    def test_詳細コンパイルと選択問題IRは意味正本なのでPを持たない(self) -> None:
        detailed = self.構文化器.詳細コンパイル("A causes B")
        選択肢 = self.構文化器.問題IR("Which is correct?", ("A", "B", "C"))
        self.assertIsNone(detailed.IR.手順)
        self.assertEqual(detailed.IR.初期状態, {})
        self.assertIsNone(選択肢.手順)
        self.assertEqual(選択肢.初期状態, {})
        self.assertTrue(選択肢.参照必須)

    def test_参照問題の意味IRはPなしでも参照必要性を保持する(self) -> None:
        ir = self.構文化器.意味コンパイル("東京の人口は？")
        self.assertIsNone(ir.手順)
        self.assertTrue(ir.参照必須)
        self.assertEqual(ir.種別, "参照")

    def test_独立資料コンパイルは意味入口を優先しPを混入しない(self) -> None:
        ir = HDS独立コンパイル(self.構文化器, "A inhibits B")
        self.assertIsNone(ir.手順)
        self.assertEqual(ir.初期状態, {})
        self.assertTrue(any(rel.種別 == "阻害" for rel in ir.関係))


if __name__ == "__main__":
    unittest.main()

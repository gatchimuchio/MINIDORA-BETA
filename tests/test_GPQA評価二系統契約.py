from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import unittest


根 = Path(__file__).resolve().parents[1]


def 評価契約を読む():
    仕様 = importlib.util.spec_from_file_location("評価契約試験対象", 根 / "tools" / "評価契約.py")
    本体 = importlib.util.module_from_spec(仕様)
    仕様.loader.exec_module(本体)
    return 本体


class GPQA評価二系統契約試験(unittest.TestCase):
    def test_機械可読契約は並列40と直列90分を固定する(self) -> None:
        契約 = json.loads((根 / "評価" / "GPQA評価二系統契約_v1.json").read_text(encoding="utf-8"))
        self.assertEqual(契約["全問題数"], 198)
        self.assertEqual(契約["並列性能継承"]["正答下限"], 40)
        self.assertFalse(契約["並列性能継承"]["問題間状態継承"])
        self.assertEqual(契約["並列性能継承"]["wall_clock上限分"], 90)
        self.assertTrue(契約["直列学習効果"]["問題間状態継承"])
        self.assertEqual(契約["直列学習効果"]["wall_clock上限分"], 90)
        self.assertEqual(契約["比較"]["学習効果差分"], "直列正答 - 並列正答")

    def test_評価契約実装も同じ下限と時間を返す(self) -> None:
        道具 = 評価契約を読む()
        条件 = 道具.GPQA評価二系統条件()
        self.assertEqual(条件["並列性能継承"]["正答下限"], 40)
        self.assertEqual(条件["並列性能継承"]["wall_clock上限分"], 90)
        self.assertEqual(条件["直列学習効果"]["wall_clock上限分"], 90)
        self.assertFalse(条件["並列性能継承"]["問題間状態継承"])
        self.assertTrue(条件["直列学習効果"]["問題間状態継承"])

    def test_直列workflowは240分を廃止し90分を強制する(self) -> None:
        本文 = (根 / ".github" / "workflows" / "GPQA現行測定.yml").read_text(encoding="utf-8")
        self.assertIn("timeout-minutes: 90", 本文)
        self.assertNotIn("timeout-minutes: 240", 本文)
        self.assertIn("同一Core直列学習実測", 本文)
        self.assertIn("runtime_seconds.txt", 本文)

    def test_現行正本は二系統の責任を分離する(self) -> None:
        本文 = (根 / "現行正本.md").read_text(encoding="utf-8")
        self.assertIn("GPQA評価方式: 並列性能継承 + 直列学習効果", 本文)
        self.assertIn("GPQA性能継承下限: 並列 40/198", 本文)
        self.assertIn("GPQA全数run wall-clock上限: 90分", 本文)
        self.assertIn("学習効果は `直列正答 - 並列正答`", 本文)
        self.assertIn("`timeout-minutes: 240` は性能評価条件として採用しない", 本文)


if __name__ == "__main__":
    unittest.main()

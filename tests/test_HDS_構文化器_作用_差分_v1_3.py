from __future__ import annotations

import unittest

from minidora.HDS構文化器_v1 import 公開HDSコンパイラ


class HDS構文化器作用差分V13試験(unittest.TestCase):
    def setUp(self) -> None:
        self.構文化器 = 公開HDSコンパイラ()

    def test_状態差が次作用の入力状態へ接続する(self) -> None:
        structure = self.構文化器.作用差分コンパイル(
            "未確認から証拠が揃ったなら確認済みに遷移する。"
            "確認済みから矛盾があるなら再検証へ遷移する。"
        )
        self.assertEqual(structure.作用数, 2)
        self.assertEqual(structure.状態差数, 2)
        self.assertEqual(structure.後続利用数, 1)
        first_delta = structure.状態差[0]
        link = structure.後続利用[0]
        second_作用 = structure.作用[1]
        self.assertEqual(first_delta.後状態, "確認済み")
        self.assertEqual(link.成立状態, "確認済み")
        self.assertEqual(link.後続作用ID, second_作用.作用ID)
        self.assertIn("矛盾がある", link.追加条件)

    def test_無関係な遷移を後続利用へ結ばない(self) -> None:
        structure = self.構文化器.作用差分コンパイル(
            "AからBへ遷移する。CからDへ遷移する。"
        )
        self.assertEqual(structure.作用数, 2)
        self.assertEqual(structure.状態差数, 2)
        self.assertEqual(structure.後続利用数, 0)

    def test_同一状態への無変化から新規後続利用を作らない(self) -> None:
        structure = self.構文化器.作用差分コンパイル(
            "AからAへ遷移する。AからBへ遷移する。"
        )
        self.assertFalse(structure.状態差[0].変化有無)
        self.assertEqual(structure.後続利用数, 0)

    def test_外国語入力でも内部作用種別は日本語(self) -> None:
        structure = self.構文化器.作用差分コンパイル(
            "transition from A to B. transition from B to C."
        )
        self.assertEqual(tuple(item.種別 for item in structure.作用), ("遷移", "遷移"))
        self.assertEqual(structure.後続利用数, 1)

    def test_作用差分構造は意味IRと計算計画から分離する(self) -> None:
        bundle = self.構文化器.コンパイル束(
            "AからBへ遷移する。BからCへ遷移する。"
        )
        self.assertIsNone(bundle.意味IR.手順)
        self.assertEqual(bundle.作用差分構造.後続利用数, 1)
        self.assertFalse(hasattr(bundle.作用差分構造, "採否"))

    def test_計算降下は作用差分を自動実行命令化しない(self) -> None:
        bundle = self.構文化器.コンパイル束(
            "AからBへ遷移する。BからCへ遷移する。"
        )
        lowered = self.構文化器.計算降下(bundle)
        self.assertEqual(lowered.作用差分構造, bundle.作用差分構造)
        self.assertIn("作用差分構造は計算Pへ自動降下しない", lowered.計算IR.境界)


if __name__ == "__main__":
    unittest.main()

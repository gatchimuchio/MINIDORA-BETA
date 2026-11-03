from __future__ import annotations

from dataclasses import replace
import unittest

from minidora.HDS実行主体 import HDS実行主体, HDS実行状態, HDS関数作用, HDS作用結果, HDS作用状態
from minidora.HDS駆動コア import HDS駆動コア
from minidora.統合駆動_v2.形成 import (
    HDS経験,
    HDS形成関係,
    HDS形成採用状態,
    経験から形成,
    再実行で検証,
    形成隔離を審査,
)
from minidora.統合駆動_v2.自動形成 import 自動形成文脈


F = frozenset


class HDS形成隔離復帰試験(unittest.TestCase):
    def 経験(self):
        return HDS経験(
            "e1", F({"入力"}), F({"完了"}), ("A",), True,
            "支持1", "文脈", 作用契約=(("A", "v1"),),
        )

    def test_反例は削除せず隔離し再検証と明示審査を分離する(self):
        e = self.経験()
        r = 経験から形成(e)
        self.assertEqual(r.採用状態, HDS形成採用状態.試行)
        self.assertFalse(r.使用可能)

        r = 再実行で検証(r, replace(e, ID="e2", 根拠署名="支持2"), "再現/v1")
        self.assertEqual(r.採用状態, HDS形成採用状態.有効)
        self.assertTrue(r.使用可能)

        r = 再実行で検証(
            r,
            replace(e, ID="bad", 成功=False, 根拠署名="失敗観測", 失敗署名="反例1"),
            "再現/v1",
        )
        self.assertEqual(r.採用状態, HDS形成採用状態.隔離)
        self.assertFalse(r.使用可能)
        self.assertIn("反例1", r.反例)
        self.assertIn("反例1", r.未解決反例)

        r = 再実行で検証(r, replace(e, ID="e3", 根拠署名="支持3"), "再現/v2")
        self.assertEqual(r.採用状態, HDS形成採用状態.隔離)
        self.assertFalse(r.使用可能)
        self.assertEqual(r.復帰検証契約, "再現/v2")

        with self.assertRaises(ValueError):
            形成隔離を審査(r, "復帰", "審査済み", "監査者", ())

        r = 形成隔離を審査(r, "復帰", "当該反例を別途審査済み", "監査者", ("反例1",))
        self.assertTrue(r.使用可能)
        self.assertEqual(r.採用状態, HDS形成採用状態.有効)
        self.assertIn("反例1", r.反例)
        self.assertIn("反例1", r.除外反例)
        self.assertEqual(r.未解決反例, ())

        # 同じ意味の反例が後着しても、過去の除外判断で黙殺しない。
        r = r.反例追加("反例1")
        self.assertEqual(r.採用状態, HDS形成採用状態.隔離)
        self.assertIn("反例1", r.未解決反例)
        self.assertFalse(r.使用可能)

    def test_復帰には隔離後の再実行検証が必要(self):
        e = self.経験()
        r = 再実行で検証(経験から形成(e), replace(e, ID="e2", 根拠署名="支持2"), "再現/v1")
        r = r.反例追加("反例1")
        with self.assertRaises(ValueError):
            形成隔離を審査(r, "復帰", "審査済み", "監査者", ("反例1",))

    def test_棄却後は自動再検証で復帰しない(self):
        e = self.経験()
        r = 再実行で検証(経験から形成(e), replace(e, ID="e2", 根拠署名="支持2"), "再現/v1")
        r = r.反例追加("反例1")
        r = 形成隔離を審査(r, "棄却", "再利用不可", "監査者", ("反例1",))
        self.assertEqual(r.採用状態, HDS形成採用状態.棄却)
        self.assertFalse(r.使用可能)
        r2 = 再実行で検証(r, replace(e, ID="e3", 根拠署名="支持3"), "再現/v2")
        self.assertEqual(r2.採用状態, HDS形成採用状態.棄却)
        self.assertFalse(r2.使用可能)

    def test_形成手順の実再利用失敗は当該形成を隔離する(self):
        作用 = HDS関数作用(
            "A",
            lambda 状態: HDS作用結果(HDS作用状態.失敗, 理由=("実行反例",)),
            出力状態=("完了",),
            純粋作用=True,
        )
        初期 = HDS実行状態(要求状態=F({"完了"}))
        文脈 = 自動形成文脈(初期, (作用,))
        関係 = HDS形成関係(
            "r", F(), F({"完了"}), ("A",), 文脈, ("支持",),
            検証契約="再現/v1", 作用契約=(("A", "v1"),),
            採用状態=HDS形成採用状態.有効,
        )
        結果 = HDS実行主体((作用,), 最大作用回数=4).実行(replace(初期, 形成関係=(関係,)))
        更新 = next(x for x in 結果.状態.形成関係 if x.ID == "r")
        self.assertEqual(更新.採用状態, HDS形成採用状態.隔離)
        self.assertFalse(更新.使用可能)
        self.assertTrue(更新.未解決反例)

    def test_中核明示審査APIは継続形成だけを更新する(self):
        e = self.経験()
        r = 再実行で検証(経験から形成(e), replace(e, ID="e2", 根拠署名="支持2"), "再現/v1")
        r = r.反例追加("反例1")
        r = 再実行で検証(r, replace(e, ID="e3", 根拠署名="支持3"), "再現/v2")
        中核 = HDS駆動コア()
        中核._継続形成関係 = (r,)
        before = 中核.継続状態署名
        更新 = 中核.形成関係を審査(r.ID, "復帰", "反例を審査済み", "監査者", ("反例1",))
        self.assertTrue(更新.使用可能)
        self.assertNotEqual(before, 中核.継続状態署名)

    def test_旧来の検証済み直接構築は互換維持(self):
        r = HDS形成関係(
            "r", F(), F({"完了"}), ("A",), "文脈", ("支持",),
            検証契約="再現/v1", 作用契約=(("A", "v1"),),
        )
        self.assertTrue(r.使用可能)


if __name__ == "__main__":
    unittest.main()

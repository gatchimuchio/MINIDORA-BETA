from __future__ import annotations

import unittest

from minidora.HDS構文化失敗集 import HDS失敗署名Bank
from minidora.HDS構文化記録_v1_1 import HDS失敗署名候補, HDS失敗署名状態
from minidora.HDS構文化記録_v1_2 import HDS改善対象
from minidora.HDS構文化器_v1 import 公開HDSコンパイラ


class HDS構文化器構造V12試験(unittest.TestCase):
    def setUp(self) -> None:
        self.構文化器 = 公開HDSコンパイラ()

    @staticmethod
    def 候補(
        signature_id: str,
        *,
        failure_class: str = "closure_failure",
        symptom: str = "未閉包",
        cause: str = "参照・関係・閉包条件の一部が未解決",
        triggers: tuple[str, ...] = ("対象未固定",),
    ) -> HDS失敗署名候補:
        return HDS失敗署名候補(
            signature_id,
            failure_class,
            symptom,
            cause,
            起動条件=triggers,
            影響範囲=("R query",),
            回復=("追加観測",),
            次探索軸=("closure",),
            再利用チェック=("G02 Closure Gate",),
        )

    def test_構造_v1_2機能をv1_3でも維持する(self) -> None:
        self.assertEqual(self.構文化器.構造版, "v1.3")
        self.assertEqual(self.構文化器.規定言語, "日本語")
        self.assertEqual(self.構文化器.基底言語, "日本語")
        self.assertEqual(self.構文化器.基底言語コード, "ja")

    def test_一回観測はPROBATIONのまま改善候補を作らない(self) -> None:
        bank = HDS失敗署名Bank()
        snapshot = bank.観測((self.候補("c1"),), Run参照="run-1")
        self.assertEqual(snapshot.観測数, 1)
        self.assertEqual(len(snapshot.署名), 1)
        self.assertEqual(snapshot.署名[0].状態, HDS失敗署名状態.候補)
        self.assertEqual(snapshot.改善候補, ())

    def test_同一Runの同一観測を二重計上しない(self) -> None:
        bank = HDS失敗署名Bank()
        候補 = self.候補("c1")
        bank.観測((候補,), Run参照="run-1")
        snapshot = bank.観測((候補,), Run参照="run-1")
        self.assertEqual(snapshot.観測数, 1)
        self.assertEqual(snapshot.署名[0].独立Run数, 1)
        self.assertEqual(snapshot.署名[0].反復回数, 1)

    def test_独立Run反復でSignatureをACTIVEへ昇格する(self) -> None:
        bank = HDS失敗署名Bank()
        bank.観測((self.候補("c1", symptom="症状A"),), Run参照="run-1")
        snapshot = bank.観測((self.候補("c2", symptom="症状B"),), Run参照="run-2")
        record = snapshot.署名[0]
        self.assertEqual(record.状態, HDS失敗署名状態.有効)
        self.assertEqual(record.独立Run数, 2)
        self.assertEqual(record.反復回数, 2)
        self.assertEqual(set(record.症状履歴), {"症状A", "症状B"})
        self.assertEqual(len(snapshot.改善候補), 1)

    def test_共通起動条件と局所条件を分離する(self) -> None:
        bank = HDS失敗署名Bank()
        bank.観測((self.候補("c1", triggers=("共通", "局所A")),), Run参照="run-1")
        snapshot = bank.観測((self.候補("c2", triggers=("共通", "局所B")),), Run参照="run-2")
        record = snapshot.署名[0]
        self.assertEqual(record.共通起動条件, ("共通",))
        self.assertEqual(set(record.局所起動条件), {"局所A", "局所B"})

    def test_構造原因が異なる失敗を統合しない(self) -> None:
        bank = HDS失敗署名Bank()
        bank.観測((self.候補("c1", cause="原因A"),), Run参照="run-1")
        snapshot = bank.観測((self.候補("c2", cause="原因B"),), Run参照="run-2")
        self.assertEqual(len(snapshot.署名), 2)
        self.assertTrue(all(item.状態 == HDS失敗署名状態.候補 for item in snapshot.署名))

    def test_改善候補は自動適用せず上位採否を要求する(self) -> None:
        bank = HDS失敗署名Bank()
        bank.観測((self.候補("c1"),), Run参照="run-1")
        snapshot = bank.観測((self.候補("c2"),), Run参照="run-2")
        improvement = snapshot.改善候補[0]
        self.assertEqual(improvement.改善対象, HDS改善対象.座標生成規則)
        self.assertTrue(improvement.昇格可能)
        self.assertTrue(improvement.自動適用禁止)
        self.assertIn("上位判断主体", " ".join(improvement.昇格条件))

    def test_関係_failureは作用素集合の改善候補へ落とす(self) -> None:
        bank = HDS失敗署名Bank()
        a = self.候補("r1", failure_class='関係_failure', cause="状態遷移の端点または条件が未固定")
        b = self.候補("r2", failure_class='関係_failure', cause="状態遷移の端点または条件が未固定")
        bank.観測((a,), Run参照="run-1")
        snapshot = bank.観測((b,), Run参照="run-2")
        self.assertEqual(snapshot.改善候補[0].改善対象, HDS改善対象.作用素集合)

    def test_Bank_JSON往復で監査状態を保持する(self) -> None:
        bank = HDS失敗署名Bank()
        bank.観測((self.候補("c1", symptom="症状A"),), Run参照="run-1")
        bank.観測((self.候補("c2", symptom="症状B"),), Run参照="run-2")
        payload = bank.JSON化()
        restored = HDS失敗署名Bank.JSONから復元(payload)
        self.assertEqual(restored.JSON化(), payload)
        self.assertTrue(restored.snapshot().旧記録保持)
        self.assertTrue(restored.snapshot().自動自己改変禁止)

    def test_構文化器帰還APIはBankだけを更新し通常コンパイルを変えない(self) -> None:
        text = "AIが世界を変える。"
        before = self.構文化器.詳細コンパイル(text)
        bank = HDS失敗署名Bank()
        self.構文化器.失敗帰還(before, bank, Run参照="run-1")
        after = self.構文化器.詳細コンパイル(text)
        self.assertEqual(before.IR, after.IR)
        self.assertEqual(before.失敗署名候補, after.失敗署名候補)
        self.assertGreaterEqual(bank.snapshot().観測数, 1)


if __name__ == "__main__":
    unittest.main()

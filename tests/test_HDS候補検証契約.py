from __future__ import annotations

from dataclasses import replace
import unittest

from minidora.HDS候補検証契約 import HDS候補検証契約群
from minidora.HDS構文化器_v1 import 公開HDSコンパイラ
from minidora.HDS中間表現 import HDSIR, HDS実行核, HDS座標, HDS関係, 値状態
from minidora.HDS観測計画 import HDS参照観測要求群


def _検証IR() -> HDSIR:
    return HDSIR(
        原文="Which molecule causes apoptosis under hypoxia?",
        正規化文="Which molecule causes apoptosis under hypoxia?",
        認知世界ID="candidate-verification-contract",
        座標=(
            HDS座標("u", "目的.未知始点", "molecule", 値状態.未観測),
            HDS座標("k", "対象.終点", "apoptosis"),
            HDS座標("search", "検索.英語正規化", "molecule causes apoptosis under hypoxia"),
            HDS座標("選択肢:A", "目的.候補", "Protein A"),
            HDS座標("選択肢:B", "目的.候補", "Protein B"),
            HDS座標("選択肢:C", "目的.候補", "Protein C"),
            HDS座標("選択肢:D", "目的.候補", "Protein D"),
        ),
        関係=(
            HDS関係(
                "question", ("u",), ("k",), "因果",
                条件=("検索述語=causes", "不足位置=始点", "条件範囲=under hypoxia"),
                値状態=値状態.未観測,
            ),
        ),
        残差=(),
        意味作用履歴=(),
        実行核=HDS実行核(),
        参照必須=True,
        種別="knowledge_query",
        入力言語="en",
    )


class HDS候補検証契約試験(unittest.TestCase):
    def test_候補ごとに同じ問い関係と条件範囲を保持する(self) -> None:
        ir = _検証IR()
        観測要求 = HDS参照観測要求群(ir)
        契約群 = HDS候補検証契約群(ir, 観測要求)

        self.assertEqual([x.候補ラベル for x in 契約群], list("ABCD"))
        for 契約, 表層 in zip(契約群, ("Protein A", "Protein B", "Protein C", "Protein D")):
            self.assertEqual(契約.候補表層, 表層)
            self.assertTrue(契約.関係)
            self.assertTrue(any(x.関係ID == "question" and x.関係種別 == "因果" for x in 契約.関係))
            self.assertTrue(any(x.未知位置 == "始点" for x in 契約.関係))
            self.assertTrue(any(x.既知端点 == ("apoptosis",) for x in 契約.関係))
            self.assertTrue(any(("条件範囲", "under hypoxia") in x.条件範囲 for x in 契約.関係))
            self.assertTrue(契約.観測)
            self.assertTrue(any(x.観測ID == f"関係:question:候補:{契約.候補ラベル}" for x in 契約.観測))

    def test_観測未形成でも候補自体は契約から消さない(self) -> None:
        ir = _検証IR()
        観測要求 = tuple(x for x in HDS参照観測要求群(ir) if x.候補ラベル == "A")
        契約群 = HDS候補検証契約群(ir, 観測要求)
        self.assertEqual(len(契約群), 4)
        self.assertTrue(契約群[0].観測)
        self.assertEqual(契約群[1].観測, ())

    def test_観測側の候補表層がKernel正本と違えば拒否する(self) -> None:
        ir = _検証IR()
        観測要求 = list(HDS参照観測要求群(ir))
        index = next(i for i, x in enumerate(観測要求) if x.候補ラベル == "A")
        観測要求[index] = replace(観測要求[index], 候補表層="Protein X")
        with self.assertRaises(ValueError):
            HDS候補検証契約群(ir, tuple(観測要求))

    def test_問題コンパイラは候補検証契約をKernel署名へ固定する(self) -> None:
        構文化器 = 公開HDSコンパイラ()
        束 = 構文化器.問題コンパイル束(
            "Which molecule causes apoptosis under hypoxia?",
            ("Protein A", "Protein B", "Protein C", "Protein D"),
        )
        self.assertEqual([x.候補ラベル for x in 束.候補検証契約], list("ABCD"))
        self.assertTrue(all(x.観測 for x in 束.候補検証契約))
        self.assertNotEqual(
            束.カーネル署名,
            replace(束, 候補検証契約=()).カーネル署名,
        )


if __name__ == "__main__":
    unittest.main()

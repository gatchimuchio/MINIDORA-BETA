from __future__ import annotations

import unittest

from minidora.HDS選択実行系 import HDS選択実行結果
from minidora.HDS中間表現 import HDSIR, HDS実行核, HDS座標
from minidora.HDS統合実行系 import HDS駆動選択実行
from minidora.hds統合判断主体 import HDS作用種別, MINIDORAHDS判断主体
from minidora.参照 import 参照記録


def 選択中間表現(*, required: bool = False) -> HDSIR:
    return HDSIR(
        原文="正しい候補を選べ A:猫 B:犬",
        正規化文="正しい候補を選べ A:猫 B:犬",
        認知世界ID="test-world",
        座標=(HDS座標('選択肢:A', "候補", "猫"), HDS座標('選択肢:B', "候補", "犬")),
        関係=(), 残差=(), 意味作用履歴=(), 実行核=HDS実行核(), 参照必須=required,
    )


def proposed() -> HDS選択実行結果:
    return HDS選択実行結果("PROPOSE", "A", "猫", ("TEST_PROPOSE",), None, 2, 1, 0, 0, 1, 0)


def suspended() -> HDS選択実行結果:
    return HDS選択実行結果("SUSPEND", None, None, ("TEST_AMBIGUOUS",), None, 2, 0, 0, 0, 0, 0)


class Fake実行系:
    def __init__(self, 参照_available: bool) -> None:
        self.参照供給器 = object() if 参照_available else None
        self.K3能力核 = object()
    def コンパイル(self, 問合せ: str):
        raise AssertionError('injected evaluatorでは構文化器を呼ばない')


class HDS統合判断主体試験(unittest.TestCase):
    def test_参照_計算_COMMITを判断主体が順番に承認する(self):
        ref = 参照記録("r1", "q", "猫が正しい", "test", "test")
        結果 = HDS駆動選択実行(
            Fake実行系(True), 選択中間表現(),
            参照実行=lambda _: (ref,),
            評価実行=lambda _ir, refs: proposed() if refs else suspended(),
        )
        self.assertEqual(結果.状態, "APPROVE")
        self.assertEqual(結果.値, "猫")
        self.assertEqual(結果.認知世界.状態, "COMMITTED")
        self.assertEqual(tuple(作用 for 作用, _ in 結果.認知世界.作用履歴), ('参照', "EVALUATE", "COMMIT"))
        self.assertEqual(結果.選択.状態, "PROPOSE")
        self.assertIn('HDS_JUDGEMENT_主体_COMMIT', 結果.理由)

    def test_候補生成結果だけでは自己COMMITしない(self):
        主体 = MINIDORAHDS判断主体()
        world = 主体.開始(選択中間表現(), 参照利用可能=False)
        self.assertEqual(主体.次作用(world).作用, HDS作用種別.候補計算)
        world = 主体.評価帰還(world, proposed())
        self.assertEqual(world.状態, "OPEN")
        self.assertEqual(主体.次作用(world).作用, HDS作用種別.確定)
        self.assertEqual(主体.確定(world).状態, "COMMITTED")

    def test_必須参照が無ければ計算へ進まずSUSPENDする(self):
        結果 = HDS駆動選択実行(
            Fake実行系(False), 選択中間表現(required=True), 参照必須=True,
            評価実行=lambda _ir, _refs: (_ for _ in ()).throw(AssertionError("評価してはならない")),
        )
        self.assertEqual(結果.状態, "SUSPEND")
        self.assertEqual(結果.認知世界.状態, "SUSPENDED")
        self.assertEqual(tuple(作用 for 作用, _ in 結果.認知世界.作用履歴), ("SUSPEND",))
        self.assertIn('HDS_REQUIRED_参照_UNAVAILABLE', 結果.理由)

    def test_評価が閉じなければSUSPENDし捏造回答を作らない(self):
        結果 = HDS駆動選択実行(Fake実行系(False), 選択中間表現(), 評価実行=lambda _ir, _refs: suspended())
        self.assertEqual(結果.状態, "SUSPEND")
        self.assertIsNone(結果.値)
        self.assertEqual(tuple(作用 for 作用, _ in 結果.認知世界.作用履歴), ("EVALUATE", "SUSPEND"))
        self.assertIn("TEST_AMBIGUOUS", 結果.認知世界.残差)

    def test_留保後も理由付きで再開放できる(self):
        主体 = MINIDORAHDS判断主体()
        world = 主体.開始(選択中間表現(), 参照利用可能=False)
        world = 主体.評価帰還(world, suspended())
        world = 主体.留保(world, ("不足",))
        reopened = 主体.再開放(world, "新しい観測が到着")
        self.assertEqual(reopened.状態, "OPEN")
        self.assertIsNone(reopened.評価状態)
        self.assertGreater(reopened.版, world.版)
        self.assertIn("REOPEN:新しい観測が到着", reopened.残差)

    def test_作用予算超過は留保し無限循環しない(self):
        主体 = MINIDORAHDS判断主体()
        world = 主体.開始(選択中間表現(), 参照利用可能=True, 作用予算=1)
        world = 主体.参照帰還(world, 参照数=1)
        next_作用 = 主体.次作用(world)
        self.assertEqual(next_作用.作用, HDS作用種別.留保)
        self.assertIn('HDS_作用_予算_EXHAUSTED', next_作用.理由)


if __name__ == "__main__":
    unittest.main()

from __future__ import annotations

import unittest
from dataclasses import fields

from minidora.hds介入制御 import (
    HDS介入記録,
    HDS指令,
    HDS指令種別,
    HDS監督状態,
    介入観測,
    既存作用,
    既存作用機会,
    既存判定,
    残差種別,
    標準HDS介入制御,
)


class HDS介入制御Test(unittest.TestCase):
    def test_HDS監督面に回答ラベルと得点がない(self):
        names = {f.name for f in fields(HDS監督状態)}
        self.assertNotIn("回答", names)
        self.assertNotIn("得点", names)
        self.assertNotIn("候補", names)

    def test_HDS指令に回答フィールドがない(self):
        names = {f.name for f in fields(HDS指令)}
        self.assertNotIn("回答", names)
        self.assertNotIn("候補", names)
        self.assertNotIn("得点", names)

    def test_正常閉包では不介入(self):
        状態 = HDS監督状態(既存判定.承認, True, False, True, "r", "c", frozenset())
        out = 標準HDS介入制御().判定(介入観測(状態, (), (), 6))
        self.assertEqual(out.種別, HDS指令種別.不介入)

    def test_候補競合では既存作用候補から選ぶ(self):
        状態 = HDS監督状態(
            既存判定.保留, False, False, True, "r", "c",
            frozenset({残差種別.候補競合}),
        )
        offers = (
            既存作用機会(
                既存作用.作業再作用,
                frozenset({残差種別.候補競合}),
                '作業:r1',
                1,
                True,
                ('EXISTING_作業_関係_AVAILABLE',),
            ),
            既存作用機会(
                既存作用.参照取得,
                frozenset({残差種別.候補競合, 残差種別.観測不足}),
                '参照:r1',
                4,
                True,
                ('EXISTING_参照_PROVIDER_AVAILABLE',),
            ),
        )
        out = 標準HDS介入制御().判定(介入観測(状態, offers, (), 6))
        self.assertEqual(out.種別, HDS指令種別.既存作用起動)
        self.assertEqual(out.作用, 既存作用.作業再作用)

    def test_同じ作用入力署名は再要求しない(self):
        状態 = HDS監督状態(
            既存判定.保留, False, False, False, "r", "c",
            frozenset({残差種別.候補識別不足}),
        )
        offer = 既存作用機会(
            既存作用.局所再照合,
            frozenset({残差種別.候補識別不足}),
            "local:r1",
        )
        used = (HDS介入記録(既存作用.局所再照合, "local:r1", (残差種別.候補識別不足,), False),)
        out = 標準HDS介入制御().判定(介入観測(状態, (offer,), used, 6))
        self.assertEqual(out.種別, HDS指令種別.停止要求)


if __name__ == "__main__":
    unittest.main()

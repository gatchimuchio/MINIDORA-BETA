from __future__ import annotations

import unittest

from minidora.HDS中間表現 import HDSIR, HDS実行核
from minidora.hds入力参照境界 import HDS入力資料本文, HDS入力資料整列, HDS入力出典ID
from minidora.参照 import 参照記録


def ir(text):
    return HDSIR(
        原文=text,
        正規化文=text,
        認知世界ID="input-ref-test",
        座標=(),
        関係=(),
        残差=(),
        意味作用履歴=(),
        実行核=HDS実行核(),
    )


class HDS入力参照境界試験(unittest.TestCase):
    def test_識別子をMINIDORA入力の参照同一性として保持する(self):
        r = 参照記録("DOI:AbC", "x", "x", "fixture://x", "p")
        self.assertEqual(HDS入力出典ID(r), "DOI:AbC")

    def test_空識別子だけprovider由来へ縮退する(self):
        r = 参照記録("", "x", "x", "fixture://x", "fixture")
        self.assertEqual(HDS入力出典ID(r), "fixture:fixture://x")

    def test_失敗資料を除外しMINIDORA入力の添字を揃える(self):
        refs = (
            参照記録("bad", "bad", "bad", "fixture://bad", "fixture", 信頼=0.9),
            参照記録("good", "good", "good", "fixture://good", "fixture", 信頼=0.25),
        )
        bundle = HDS入力資料整列(refs, (ValueError("bad"), ir("ok")), lambda x: x)
        self.assertEqual(bundle.失敗数, 1)
        self.assertEqual(tuple(x.原文 for x in bundle.IR群), ("ok",))
        self.assertEqual(bundle.出典ID群, ("good",))
        self.assertEqual(bundle.信頼群, (0.25,))
        self.assertEqual(tuple(x.識別子 for x in bundle.成功記録群), ("good",))

    def test_型付き資料は対象意味キー値を保持して構文化器へ渡す(self):
        record = 参照記録(
            "calc:1", "request", "raw", "internal://compute", "compute",
            意味キー="計算結果", 値=47, 意味確定=True,
        )
        packaged = HDS入力資料本文(record)
        self.assertIn("request", packaged)
        self.assertIn("計算結果", packaged)
        self.assertIn("47", packaged)

    def test_未構造資料は内容を改変しない(self):
        record = 参照記録("raw:1", "x", "original text", "fixture://x", "fixture")
        self.assertEqual(HDS入力資料本文(record), "original text")


if __name__ == "__main__":
    unittest.main()

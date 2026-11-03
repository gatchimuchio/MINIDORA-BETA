from __future__ import annotations

import unittest

from minidora.hds参照拡張 import HDS候補被覆優先統合
from minidora.参照 import 参照記録


def rec(情報源: str, 選択肢: str | None = None) -> 参照記録:
    conditions = (('hds_query_選択肢', 選択肢),) if 選択肢 is not None else ()
    return 参照記録(情報源, 情報源, 情報源, "test", "test", 条件=conditions)


class HDS参照ExtensionTest(unittest.TestCase):
    def test_候補情報源をgenericより先に予算へ残す(self):
        out = HDS候補被覆優先統合(
            (rec("generic1"), rec("generic2")),
            (rec("A", "A"), rec("B", "B"), rec("C", "C"), rec("D", "D")),
            ("A", "B", "C", "D"),
            4,
        )
        self.assertEqual({row.識別子 for row in out}, {"A", "B", "C", "D"})

    def test_同一情報源はquery_provenanceだけ統合する(self):
        out = HDS候補被覆優先統合(
            (rec("shared", "A"),),
            (rec("shared", "B"),),
            ("A", "B"),
            4,
        )
        self.assertEqual(len(out), 1)
        labels = {v for k, v in out[0].条件 if k == 'hds_query_選択肢'}
        self.assertEqual(labels, {"A", "B"})


if __name__ == "__main__":
    unittest.main()

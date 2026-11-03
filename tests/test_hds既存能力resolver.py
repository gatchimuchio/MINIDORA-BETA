from __future__ import annotations

import unittest

from minidora.hds既存能力resolver import (
    既存MINIDORA提案解決,
    既存提案源,
    既存提案状態,
    既存能力提案,
)


def p(情報源, answer=None, *, valid=True, direct=False):
    return 既存能力提案(
        情報源,
        既存提案状態.承認候補 if valid else 既存提案状態.保留,
        answer if valid else None,
        根拠成立=valid,
        一意=valid,
        直接検証済み=direct,
    )


class ExistingResolverTest(unittest.TestCase):
    def test_HDS提案を型として持たない(self):
        self.assertFalse(hasattr(既存提案源, "HDS"))

    def test_K3と能力模型が一致すれば既存MINIDORAが閉じる(self):
        out = 既存MINIDORA提案解決([
            p(既存提案源.K3, "A"),
            p(既存提案源.能力模型, "A"),
        ])
        self.assertEqual(out.状態, 既存提案状態.承認候補)
        self.assertEqual(out.回答, "A")

    def test_既存能力が競合したらHDSに勝者を選ばせず保留(self):
        out = 既存MINIDORA提案解決([
            p(既存提案源.K3, "A"),
            p(既存提案源.能力模型, "B"),
        ])
        self.assertEqual(out.状態, 既存提案状態.保留)
        self.assertIsNone(out.回答)
        self.assertIn('候補_CONFLICT', out.残差)

    def test_直接関係検証は既存強証拠として利用できる(self):
        out = 既存MINIDORA提案解決([
            p(既存提案源.直接関係, "C", direct=True),
            p(既存提案源.能力模型, "B"),
        ])
        self.assertEqual(out.状態, 既存提案状態.承認候補)
        self.assertEqual(out.回答, "C")


if __name__ == "__main__":
    unittest.main()

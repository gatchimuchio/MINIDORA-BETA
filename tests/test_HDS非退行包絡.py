from __future__ import annotations

from dataclasses import dataclass
import unittest

from minidora.HDS非退行包絡 import HDS非退行包絡, HDS証拠優越包絡


@dataclass(frozen=True, slots=True)
class 模擬結果:
    状態: str
    回答: str | None
    証拠: int = 0


def 承認済み(結果: object) -> bool:
    return isinstance(結果, 模擬結果) and 結果.状態 == "APPROVE" and 結果.回答 is not None


class HDS非退行包絡試験(unittest.TestCase):
    def test_基準承認済みは拡張を実行せず完全保持する(self):
        基準 = 模擬結果("APPROVE", "C", 2)
        呼出 = []

        def 拡張():
            呼出.append("実行")
            return 模擬結果("APPROVE", "A", 9)

        判定 = HDS非退行包絡(
            基準,
            基準承認判定=承認済み,
            拡張実行=拡張,
            拡張承認判定=承認済み,
            拡張採用証明=lambda _基準, _拡張: True,
        )
        self.assertIs(判定.出力, 基準)
        self.assertIs(判定.基準結果, 基準)
        self.assertIsNone(判定.拡張結果)
        self.assertTrue(判定.基準固定)
        self.assertFalse(判定.拡張採用)
        self.assertEqual(呼出, [])

    def test_承認済み基準も明示的な証拠優越時だけ更新できる(self):
        基準 = 模擬結果("APPROVE", "A", 2)
        拡張 = 模擬結果("APPROVE", "B", 5)
        判定 = HDS証拠優越包絡(
            基準,
            拡張,
            基準承認判定=承認済み,
            拡張承認判定=承認済み,
            証拠優越証明=lambda 前, 後: 後.証拠 >= 前.証拠 + 2,
        )
        self.assertIs(判定.出力, 拡張)
        self.assertTrue(判定.拡張採用)
        self.assertFalse(判定.基準固定)

    def test_承認済み基準は証拠優越不足なら保持する(self):
        基準 = 模擬結果("APPROVE", "A", 2)
        拡張 = 模擬結果("APPROVE", "B", 3)
        判定 = HDS証拠優越包絡(
            基準,
            拡張,
            基準承認判定=承認済み,
            拡張承認判定=承認済み,
            証拠優越証明=lambda 前, 後: 後.証拠 >= 前.証拠 + 2,
        )
        self.assertIs(判定.出力, 基準)
        self.assertFalse(判定.拡張採用)
        self.assertTrue(判定.基準固定)

    def test_基準未承認でも証明なし拡張はshadowに留める(self):
        基準 = 模擬結果("SUSPEND", None, 1)
        拡張 = 模擬結果("APPROVE", "B", 4)
        判定 = HDS非退行包絡(
            基準,
            基準承認判定=承認済み,
            拡張実行=lambda: 拡張,
            拡張承認判定=承認済み,
        )
        self.assertIs(判定.出力, 基準)
        self.assertIs(判定.拡張結果, 拡張)
        self.assertTrue(判定.基準固定)
        self.assertFalse(判定.拡張採用)
        self.assertIn("HDS_EXTENSION_PROOF_MISSING", 判定.理由)

    def test_基準未承認かつ明示証明ありだけ拡張採用する(self):
        基準 = 模擬結果("SUSPEND", None, 1)
        拡張 = 模擬結果("APPROVE", "B", 4)
        判定 = HDS非退行包絡(
            基準,
            基準承認判定=承認済み,
            拡張実行=lambda: 拡張,
            拡張承認判定=承認済み,
            拡張採用証明=lambda 前, 後: isinstance(前, 模擬結果) and isinstance(後, 模擬結果) and 後.証拠 > 前.証拠,
        )
        self.assertIs(判定.出力, 拡張)
        self.assertFalse(判定.基準固定)
        self.assertTrue(判定.拡張採用)

    def test_拡張が未承認なら証明が真でも採用しない(self):
        基準 = 模擬結果("SUSPEND", None, 1)
        拡張 = 模擬結果("SUSPEND", None, 99)
        判定 = HDS非退行包絡(
            基準,
            基準承認判定=承認済み,
            拡張実行=lambda: 拡張,
            拡張承認判定=承認済み,
            拡張採用証明=lambda _前, _後: True,
        )
        self.assertIs(判定.出力, 基準)
        self.assertFalse(判定.拡張採用)
        self.assertIn("HDS_EXTENSION_NOT_APPROVED", 判定.理由)

    def test_基準未承認で拡張なしなら基準を保持する(self):
        基準 = 模擬結果("SUSPEND", None)
        判定 = HDS非退行包絡(基準, 基準承認判定=承認済み)
        self.assertIs(判定.出力, 基準)
        self.assertTrue(判定.基準固定)
        self.assertFalse(判定.拡張採用)


if __name__ == "__main__":
    unittest.main()

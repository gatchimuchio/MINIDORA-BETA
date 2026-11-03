from __future__ import annotations

import unittest

from minidora import 公開HDSコンパイラ
from minidora.HDS実行系射影 import HDSK資料射影


def _条件値(関係, key: str) -> str:
    prefix = key + "="
    for raw in 関係.条件:
        value = str(raw)
        if value.startswith(prefix):
            return value[len(prefix):].strip()
    return ""


def _阻害関係(ir):
    rows = [関係 for 関係 in ir.関係 if str(関係.種別) == "阻害"]
    if not rows:
        raise AssertionError("阻害関係が存在しない")
    return next((r for r in rows if _条件値(r, '範囲結合') == '構文化器'), rows[0])


def _K内容(ir) -> tuple[str, ...]:
    return tuple(str(coord.内容) for coord in HDSK資料射影(ir).座標)


def _K阻害関係(ir):
    return [関係 for 関係 in HDSK資料射影(ir).関係 if str(関係.種別) == "阻害"]


class HDS英語関係範囲試験(unittest.TestCase):
    def setUp(self) -> None:
        self.構文化器 = 公開HDSコンパイラ()

    def test_does_notを実体端点と否定範囲へ分離する(self) -> None:
        ir = self.構文化器.コンパイル("Compound A does not inhibit Enzyme X.")
        関係 = _阻害関係(ir)
        coords = ir.座標辞書()
        self.assertEqual(str(coords[関係.始点[0]].内容), "Compound A")
        self.assertEqual(str(coords[関係.終点[0]].内容), "Enzyme X")
        self.assertEqual(_条件値(関係, "極性"), "否定")
        self.assertEqual(_条件値(関係, '範囲結合'), '構文化器')

        projected = _K阻害関係(ir)
        self.assertEqual(len(projected), 1)
        self.assertEqual(_条件値(projected[0], "極性"), "否定")
        self.assertIn("Compound A", _K内容(ir))
        self.assertNotIn("Compound A does not", _K内容(ir))
        self.assertTrue(any(str(c.種別) == "表層.端点原形" and str(c.内容) == "Compound A does not" for c in ir.座標))

    def test_mayを実体端点と可能範囲へ分離し資料で保持する(self) -> None:
        ir = self.構文化器.コンパイル("Compound A may inhibit Enzyme X.")
        関係 = _阻害関係(ir)
        coords = ir.座標辞書()
        self.assertEqual(str(coords[関係.始点[0]].内容), "Compound A")
        self.assertEqual(str(coords[関係.終点[0]].内容), "Enzyme X")
        self.assertEqual(_条件値(関係, "様相"), "可能")
        self.assertEqual(_条件値(関係, "様相表層"), "may")

        projected = _K阻害関係(ir)
        self.assertEqual(len(projected), 1)
        self.assertEqual(_条件値(projected[0], "様相"), "可能")
        self.assertIn("Compound A", _K内容(ir))
        self.assertNotIn("Compound A may", _K内容(ir))
        self.assertTrue(any(str(c.種別) == "表層.端点原形" and str(c.内容) == "Compound A may" for c in ir.座標))

    def test_mustを必要範囲として資料で保持する(self) -> None:
        ir = self.構文化器.コンパイル("Compound A must inhibit Enzyme X.")
        関係 = _阻害関係(ir)
        coords = ir.座標辞書()
        self.assertEqual(str(coords[関係.始点[0]].内容), "Compound A")
        self.assertEqual(_条件値(関係, "様相"), "必要")
        projected = _K阻害関係(ir)
        self.assertEqual(len(projected), 1)
        self.assertEqual(_条件値(projected[0], "様相"), "必要")
        self.assertNotIn("Compound A must", _K内容(ir))

    def test_if条件を同じ関係へ結び資料で保持する(self) -> None:
        ir = self.構文化器.コンパイル("If condition X, Compound A inhibits Enzyme X.")
        関係 = _阻害関係(ir)
        self.assertEqual(_条件値(関係, '条件範囲'), "If condition X")
        projected = _K阻害関係(ir)
        self.assertEqual(len(projected), 1)
        self.assertEqual(_条件値(projected[0], '条件範囲'), "If condition X")

    def test_通常肯定文には範囲を捏造しない(self) -> None:
        ir = self.構文化器.コンパイル("Compound A inhibits Enzyme X.")
        関係 = _阻害関係(ir)
        self.assertEqual(_条件値(関係, "極性"), "")
        self.assertEqual(_条件値(関係, "様相"), "")
        self.assertEqual(_条件値(関係, '条件範囲'), "")
        self.assertTrue(any(str(r.種別) == "阻害" for r in HDSK資料射影(ir).関係))
        self.assertFalse(any(str(c.種別) == "表層.端点原形" for c in ir.座標))

    def test_質問文は宣言範囲層で再解釈しない(self) -> None:
        ir = self.構文化器.コンパイル("Which compound may inhibit Enzyme X?")
        scoped = [r for r in ir.関係 if _条件値(r, '範囲結合') == '構文化器']
        self.assertEqual(scoped, [])


if __name__ == "__main__":
    unittest.main()

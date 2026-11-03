from dataclasses import replace
import unittest

from minidora import (
    HDSIR,
    HDS実行核,
    HDS座標,
    値状態,
    ミニドラ,
    要求,
    手順,
    命令,
    作用,
    実行状態,
)


def _加算IR() -> HDSIR:
    procedure = 手順(
        "fixture:HDS-IR加算",
        (
            命令(
                "加算",
                作用.加算,
                引数=("$a", "$b"),
                更新先="結果",
                根拠=("fixture:HDS-IR",),
            ),
        ),
        由来="fixture compiler",
    )
    return HDSIR(
        原文="2と3を足す",
        正規化文="2と3を足す",
        認知世界ID="fixture:world",
        座標=(
            HDS座標("a", "対象.現在状態", 2),
            HDS座標("b", "対象.現在状態", 3),
            HDS座標('作用', "手段.作用", "加算"),
        ),
        関係=(),
        残差=(),
        意味作用履歴=(),
        実行核=HDS実行核("加算", ("a", "b"), "結果"),
        初期状態={"a": 2, "b": 3},
        閉包状態="作用閉包",
        表現状態="意味保持",
        手順=procedure,
    )


class _固定構文化器:
    def __init__(self, ir: HDSIR) -> None:
        self.ir = ir

    def コンパイル(self, 入力, *, 前回結果=None, HDS履歴=(), 文脈=None):
        return self.ir


class HDSIR実行境界試験(unittest.TestCase):
    def test_確定した実行入力は実行可能(self):
        ir = _加算IR()
        self.assertTrue(ir.実行可能)
        self.assertEqual(ir.実行阻害理由, ())

    def test_未確定の実行入力は手順があっても保留(self):
        ir = _加算IR()
        coordinates = (
            replace(ir.座標[0], 値状態=値状態.未確定),
            *ir.座標[1:],
        )
        uncertain_ir = replace(ir, 座標=coordinates)

        self.assertFalse(uncertain_ir.実行可能)
        self.assertIn("実行入力未確定:a", uncertain_ir.実行阻害理由)

        結果 = ミニドラ(HDSコンパイラ_=_固定構文化器(uncertain_ir)).実行(要求("実行して"))
        self.assertIsNone(結果.値)
        self.assertEqual(結果.採否.状態, 実行状態.保留)
        self.assertIn("実行入力未確定:a", 結果.採否.理由)

    def test_実行核が参照する座標欠落は実行不能(self):
        ir = replace(_加算IR(), 座標=(HDS座標("a", "対象.現在状態", 2),))
        self.assertFalse(ir.実行可能)
        self.assertIn("実行入力座標欠落:b", ir.実行阻害理由)

    def test_実行核外の未確定座標だけでは局所閉包を壊さない(self):
        ir = _加算IR()
        open_文脈 = HDS座標(
            '文脈:未知',
            "文脈.未解",
            None,
            値状態=値状態.未確定,
        )
        projected = replace(ir, 座標=ir.座標 + (open_文脈,))
        self.assertTrue(projected.実行可能)
        self.assertEqual(projected.実行阻害理由, ())


if __name__ == "__main__":
    unittest.main()

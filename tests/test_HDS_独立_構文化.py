from __future__ import annotations

import unittest

from minidora.HDS適合器 import HDS文脈, HDS独立コンパイル
from minidora.HDS中間表現 import HDSIR, HDS実行核


class _Recorder構文化器:
    def __init__(self) -> None:
        self.calls: list[tuple[str, object, tuple[HDSIR, ...], HDS文脈 | None]] = []

    def コンパイル(
        self,
        入力: str,
        *,
        前回結果=None,
        HDS履歴: tuple[HDSIR, ...] = (),
        文脈: HDS文脈 | None = None,
    ) -> HDSIR:
        self.calls.append((入力, 前回結果, HDS履歴, 文脈))
        return HDSIR(
            原文=入力,
            正規化文=入力,
            認知世界ID="independent-compile-test",
            座標=(),
            関係=(),
            残差=(),
            意味作用履歴=(),
            実行核=HDS実行核("意味構造転送"),
            種別="意味構造",
            閉包状態='CLOSED_FOR_意味_TRANSFER',
        )


class _Legacy構文化器:
    def __init__(self) -> None:
        self.calls: list[str] = []

    def コンパイル(self, 入力: str) -> HDSIR:
        self.calls.append(入力)
        return HDSIR(
            原文=入力,
            正規化文=入力,
            認知世界ID="legacy-independent-test",
            座標=(),
            関係=(),
            残差=(),
            意味作用履歴=(),
            実行核=HDS実行核("意味構造転送"),
        )


class HDS独立コンパイル試験(unittest.TestCase):
    def test_前回結果履歴現在焦点を独立文書へ渡さない(self) -> None:
        構文化器 = _Recorder構文化器()
        HDS独立コンパイル(構文化器, "external document")

        self.assertEqual(len(構文化器.calls), 1)
        text, previous, history, 文脈 = 構文化器.calls[0]
        self.assertEqual(text, "external document")
        self.assertIsNone(previous)
        self.assertEqual(history, ())
        self.assertIsInstance(文脈, HDS文脈)
        self.assertEqual(文脈.記憶版, 0)
        self.assertIsNone(文脈.現在焦点)
        self.assertIsNone(文脈.直前結果)
        self.assertEqual(文脈.記憶引用, ())

    def test_旧式構文化器には存在しない文脈引数を押し込まない(self) -> None:
        構文化器 = _Legacy構文化器()
        結果 = HDS独立コンパイル(構文化器, "legacy data")
        self.assertEqual(結果.原文, "legacy data")
        self.assertEqual(構文化器.calls, ["legacy data"])


if __name__ == "__main__":
    unittest.main()

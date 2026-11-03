from __future__ import annotations

import unittest

from minidora import 公開HDSコンパイラ
from minidora.HDS選択実行系 import HDS選択推論実行
from minidora.HDS中間表現 import HDSIR
from minidora.hds局所再照合 import HDS局所Window候補
from minidora.K3機能 import K3相当能力核
from minidora.参照 import 参照記録


class 局所Window選択試験(unittest.TestCase):
    def setUp(self) -> None:
        self.構文化器 = 公開HDSコンパイラ()
        self.question = self.構文化器.問題IR(
            "Which machine uses engine X?",
            ("Alpha", "Beta", "Gamma", "Delta"),
        )

    def test_問いと候補差分が同じ局所窓にある箇所だけ選ぶ(self) -> None:
        refs = (
            参照記録(
                "doc-a", "fixture",
                "Unrelated introductory background is given here. Alpha uses engine X during operation. Another unrelated sentence follows afterward.",
                "fixture://a", "fixture", 1.0,
            ),
            参照記録(
                "doc-b", "fixture",
                "Beta appears in this unrelated sentence only. The remaining text discusses a different subject without engine terminology.",
                "fixture://b", "fixture", 1.0,
            ),
        )
        windows = HDS局所Window候補(self.question, refs, 上限=8)
        self.assertTrue(windows)
        self.assertTrue(any("Alpha uses engine X" in row.内容 for row in windows))
        self.assertFalse(any(row.内容.casefold() == row.参照.内容.casefold() for row in windows))
        self.assertTrue(all(row.問い一致数 > 0 and row.候補差分一致数 > 0 for row in windows))

    def test_候補語だけで問い語がなければ選ばない(self) -> None:
        refs = (
            参照記録(
                "doc-a", "fixture",
                "Alpha appears in a completely different historical discussion. Another sentence only describes Alpha by name.",
                "fixture://a", "fixture", 1.0,
            ),
        )
        self.assertEqual(HDS局所Window候補(self.question, refs), ())


class _全文だけ弱化構文化器:
    並列安全 = True

    def __init__(self) -> None:
        self.base = 公開HDSコンパイラ()

    def コンパイル(self, text: str) -> HDSIR:
        if text.startswith("Unrelated introductory background"):
            # 全文一括処理では局所関係を失った状態を再現する一般fixture。
            return self.base.コンパイル("Unrelated introductory background only.")
        return self.base.コンパイル(text)


class 局所再照合実行系試験(unittest.TestCase):
    def test_全文で失った有向関係を局所再解析して回収する(self) -> None:
        base = 公開HDSコンパイラ()
        構文化器 = _全文だけ弱化構文化器()
        question = base.問題IR(
            "Which machine uses engine X?",
            ("Alpha", "Beta", "Gamma", "Delta"),
        )
        refs = (
            参照記録(
                "doc-a", "fixture",
                "Unrelated introductory background is given here. Alpha uses engine X during operation. Another unrelated sentence follows afterward.",
                "fixture://a", "fixture", 1.0,
            ),
        )

        baseline = HDS選択推論実行(
            question,
            refs,
            コンパイル=構文化器.コンパイル,
            基礎能力核=K3相当能力核(),
            作業再作用=False,
            局所再照合=False,
        )
        reconstructed = HDS選択推論実行(
            question,
            refs,
            コンパイル=構文化器.コンパイル,
            基礎能力核=K3相当能力核(),
            作業再作用=True,
            局所再照合=True,
        )

        self.assertEqual(baseline.状態, "SUSPEND")
        self.assertEqual(reconstructed.状態, "APPROVE", reconstructed.理由)
        self.assertEqual(reconstructed.回答ラベル, "A")
        self.assertIn("LEGACY_LOCAL_WINDOW_RECHECK", reconstructed.理由)
        self.assertGreater(reconstructed.局所Window数, 0)
        self.assertGreater(reconstructed.局所Windowコンパイル数, 0)
        self.assertEqual(reconstructed.局所再照合数, 1)
        self.assertEqual(reconstructed.作業関係K昇格数, 0)


if __name__ == "__main__":
    unittest.main()

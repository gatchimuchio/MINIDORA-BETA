from __future__ import annotations

import unittest

from minidora.K3機能 import 候補
from minidora.K3_HDSネイティブ import HDS候補診断, _例外消去候補


def _候補(label: str, 信頼度: float = 0.8) -> 候補:
    return 候補(
        answer=label,
        関係="test",
        信頼度=信頼度,
        expert="fixture",
        proof_fact_ids=(f"fact:{label}",),
        provenance=("fixture",),
    )


def _diag(
    label: str,
    sources: int = 1,
    proofs: int = 1,
    distinctive_sources: int = 1,
) -> HDS候補診断:
    return HDS候補診断(
        候補=label,
        合計得点=3.0,
        証拠得点=3.0,
        関係図得点=0.0,
        関係図補正係数=0.0,
        独立出典数=sources,
        採用証拠数=1,
        関係図深さ=None,
        根拠事実数=proofs,
        識別語数=1,
        識別一致出典数=distinctive_sources,
    )


class K3例外消去試験(unittest.TestCase):
    def test_4択中3択が固有意味の独立根拠付きなら残り1択を消去できる(self) -> None:
        choices = (("A", "a"), ("B", "b"), ("C", "c"), ("D", "d"))
        scored = [(3.0, _候補("A")), (2.9, _候補("B")), (2.8, _候補("C"))]
        diagnostics = (
            _diag("A"), _diag("B"), _diag("C"),
            _diag("D", sources=0, proofs=0, distinctive_sources=0),
        )
        結果 = _例外消去候補(choices, scored, diagnostics)
        self.assertIsNotNone(結果)
        self.assertEqual(結果.answer, "D")
        self.assertEqual(set(結果.proof_fact_ids), {"fact:A", "fact:B", "fact:C"})

    def test_未確認候補が2択残るなら消去しない(self) -> None:
        choices = (("A", "a"), ("B", "b"), ("C", "c"), ("D", "d"))
        scored = [(3.0, _候補("A")), (2.9, _候補("B"))]
        diagnostics = (
            _diag("A"), _diag("B"),
            _diag("C", 0, 0, 0), _diag("D", 0, 0, 0),
        )
        self.assertIsNone(_例外消去候補(choices, scored, diagnostics))

    def test_全候補に根拠がある時は最小得点を例外扱いしない(self) -> None:
        choices = (("A", "a"), ("B", "b"), ("C", "c"), ("D", "d"))
        scored = [(3.0, _候補("A")), (2.9, _候補("B")), (2.8, _候補("C")), (0.5, _候補("D"))]
        diagnostics = (_diag("A"), _diag("B"), _diag("C"), _diag("D"))
        self.assertIsNone(_例外消去候補(choices, scored, diagnostics))

    def test_独立出典が無い候補は確認済みに数えない(self) -> None:
        choices = (("A", "a"), ("B", "b"), ("C", "c"))
        scored = [(3.0, _候補("A")), (2.9, _候補("B"))]
        diagnostics = (
            _diag("A"), _diag("B", sources=0, distinctive_sources=0),
            _diag("C", sources=0, proofs=0, distinctive_sources=0),
        )
        self.assertIsNone(_例外消去候補(choices, scored, diagnostics))

    def test_共通語だけの出典は例外消去の確認根拠に数えない(self) -> None:
        choices = (("A", "a"), ("B", "b"), ("C", "c"))
        scored = [(3.0, _候補("A")), (2.9, _候補("B"))]
        diagnostics = (
            _diag("A"),
            _diag("B", distinctive_sources=0),
            _diag("C", sources=0, proofs=0, distinctive_sources=0),
        )
        self.assertIsNone(_例外消去候補(choices, scored, diagnostics))


if __name__ == "__main__":
    unittest.main()

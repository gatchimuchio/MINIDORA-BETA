from __future__ import annotations

import unittest

from minidora.K3_HDSネイティブ import HDS意味署名, _group_score, _候補識別語, _証拠群


def _sig(*terms: str) -> HDS意味署名:
    return HDS意味署名(frozenset(terms), frozenset(), frozenset())


def _証拠(*terms: str) -> _証拠群:
    return _証拠群(
        "e",
        '情報源:e',
        frozenset(terms),
        frozenset(),
        frozenset(),
        ("fact:e",),
        1.0,
        "fact",
    )


class K3候補差分意味試験(unittest.TestCase):
    def test_候補集合の共通語を除いて固有語を抽出する(self) -> None:
        distinctive = _候補識別語({
            "A": _sig("shared", "alpha"),
            "B": _sig("shared", "beta"),
            "C": _sig("shared", "gamma"),
        })
        self.assertEqual(distinctive["A"], frozenset({"alpha"}))
        self.assertEqual(distinctive["B"], frozenset({"beta"}))
        self.assertEqual(distinctive["C"], frozenset({"gamma"}))

    def test_共通語だけの証拠より候補固有語まで含む証拠を強く評価する(self) -> None:
        question = _sig("question")
        候補 = _sig("shared", "alpha")
        common = _証拠("question", "shared")
        specific = _証拠("question", "shared", "alpha")
        common_score = _group_score(question, 候補, common, 識別語=frozenset({"alpha"}))
        specific_score = _group_score(question, 候補, specific, 識別語=frozenset({"alpha"}))
        self.assertGreater(specific_score, common_score)
        self.assertGreater(common_score, 0.0)

    def test_候補に識別語が無い場合は無理に差を作らない(self) -> None:
        distinctive = _候補識別語({
            "A": _sig("same"),
            "B": _sig("same"),
        })
        self.assertEqual(distinctive["A"], frozenset())
        self.assertEqual(distinctive["B"], frozenset())
        score = _group_score(_sig("question"), _sig("same"), _証拠("question", "same"))
        self.assertGreater(score, 0.0)


if __name__ == "__main__":
    unittest.main()

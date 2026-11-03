from __future__ import annotations

import unittest

from minidora.HDS資料K import HDSIR知識適合器
from minidora.HDS中間表現 import HDSIR, HDS実行核, HDS座標, HDS関係
from minidora.K3機能 import K3相当能力核
from minidora.K3_HDSネイティブ import HDSIRネイティブ適合器


def _ir(text: str, coords: tuple[HDS座標, ...], relations: tuple[HDS関係, ...] = ()) -> HDSIR:
    return HDSIR(
        原文=text,
        正規化文=text,
        認知世界ID='情報源-reconcile:test',
        座標=coords,
        関係=relations,
        残差=(),
        意味作用履歴=(),
        実行核=HDS実行核("意味構造転送"),
        種別="意味構造",
        閉包状態='CLOSED_FOR_意味_TRANSFER',
        入力言語="en",
    )


def _question() -> tuple[HDSIR, dict[str, HDSIR]]:
    question = _ir(
        "What does Alpha use?",
        (
            HDS座標("alpha", "対象.実体", "Alpha", 原文範囲=(10, 15)),
            HDS座標("use", "関係.述語表層", "use", 原文範囲=(16, 19)),
            HDS座標('選択肢:A', "目的.候補", "engine"),
            HDS座標('選択肢:B', "目的.候補", "stone"),
        ),
        (HDS関係("qr", ("alpha",), ("use",), "記述→問い"),),
    )
    choices = {
        "A": _ir("engine", (HDS座標("a", "対象.実体", "engine", 原文範囲=(0, 6)),)),
        "B": _ir("stone", (HDS座標("b", "対象.実体", "stone", 原文範囲=(0, 5)),)),
    }
    return question, choices


class HDS出典調停統合試験(unittest.TestCase):
    def test_同一情報源の文書証拠と関係図を独立根拠として二重評価しない(self) -> None:
        模型核 = K3相当能力核()
        資料 = _ir(
            "Alpha uses engine.",
            (
                HDS座標("alpha", "対象.実体", "Alpha", 原文範囲=(0, 5)),
                HDS座標("engine", "対象.実体", "engine", 原文範囲=(11, 17)),
            ),
            (HDS関係("r", ("alpha",), ("engine",), "作用"),),
        )
        HDSIR知識適合器(模型核).投入(資料, provenance=("fixture", "doc:1"))
        question, choices = _question()

        結果 = HDSIRネイティブ適合器(模型核).実行(question, 候補IR=choices)
        diag = next(item for item in 結果.候補診断 if item.候補 == "A")

        self.assertEqual(結果.状態, "APPROVE")
        self.assertEqual(結果.回答ラベル, "A")
        self.assertEqual(diag.独立出典数, 1)
        self.assertEqual(diag.採用証拠数, 1)
        self.assertGreater(diag.関係図得点, 0.0)
        self.assertAlmostEqual(diag.関係図補正係数, 0.45)

    def test_別情報源の支持は独立出典として追加される(self) -> None:
        模型核 = K3相当能力核()
        for doc_id in ("1", "2"):
            資料 = _ir(
                "Alpha uses engine.",
                (
                    HDS座標("alpha", "対象.実体", "Alpha", 原文範囲=(0, 5)),
                    HDS座標("engine", "対象.実体", "engine", 原文範囲=(11, 17)),
                ),
                (HDS関係("r", ("alpha",), ("engine",), "作用"),),
            )
            HDSIR知識適合器(模型核).投入(資料, provenance=("fixture", "doc:" + doc_id))
        question, choices = _question()

        結果 = HDSIRネイティブ適合器(模型核).実行(question, 候補IR=choices)
        diag = next(item for item in 結果.候補診断 if item.候補 == "A")

        self.assertEqual(結果.状態, "APPROVE")
        self.assertEqual(結果.回答ラベル, "A")
        self.assertGreaterEqual(diag.独立出典数, 2)
        self.assertGreaterEqual(diag.採用証拠数, 2)


if __name__ == "__main__":
    unittest.main()

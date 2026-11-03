from __future__ import annotations

import unittest

from minidora.HDS資料K import HDSIR知識適合器, HDS証拠事実
from minidora.HDS中間表現 import HDSIR, HDS実行核, HDS座標, HDS関係, HDS残差
from minidora.K3機能 import K3相当能力核
from minidora.K3_HDSネイティブ import HDSIRネイティブ適合器


def _ir(
    text: str,
    coords: tuple[HDS座標, ...],
    relations: tuple[HDS関係, ...] = (),
    residuals: tuple[HDS残差, ...] = (),
) -> HDSIR:
    return HDSIR(
        原文=text,
        正規化文=text,
        認知世界ID='残差-証拠-test',
        座標=coords,
        関係=relations,
        残差=residuals,
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
            HDS座標("q-alpha", "対象.実体", "Alpha", 原文範囲=(10, 15)),
            HDS座標('選択肢:A', "目的.候補", "engine"),
            HDS座標('選択肢:B', "目的.候補", "stone"),
        ),
    )
    choices = {
        "A": _ir("engine", (HDS座標("a", "対象.実体", "engine", 原文範囲=(0, 6)),)),
        "B": _ir("stone", (HDS座標("b", "対象.実体", "stone", 原文範囲=(0, 5)),)),
    }
    return question, choices


def _資料(residuals: tuple[HDS残差, ...]) -> HDSIR:
    return _ir(
        "Alpha uses engine; note uncertain.",
        (
            HDS座標("alpha", "対象.実体", "Alpha", 原文範囲=(0, 5)),
            HDS座標("engine", "対象.実体", "engine", 原文範囲=(11, 17)),
            HDS座標("note", "文脈.注記", "uncertain note", 原文範囲=(19, 33)),
        ),
        (HDS関係("use", ("alpha",), ("engine",), "作用"),),
        residuals,
    )


class HDS残差証拠境界試験(unittest.TestCase):
    def test_意味_lossは情報源全体を確定証拠へ昇格させない(self) -> None:
        模型核 = K3相当能力核()
        資料 = _資料((HDS残差("res:loss", '意味_loss', "Alpha uses engine", "意味構造を保持できない"),))
        ingest = HDSIR知識適合器(模型核).投入(資料, provenance=("fixture", "doc:loss"))
        question, choices = _question()
        結果 = HDSIRネイティブ適合器(模型核).実行(question, 候補IR=choices)

        self.assertTrue(ingest.意味_loss)
        self.assertGreater(ingest.証拠阻害事実数, 0)
        self.assertEqual(結果.状態, "SUSPEND", 結果.候補診断)
        self.assertIsNone(結果.回答ラベル)
        self.assertTrue(any('残差_blocked:意味_loss' in fact.provenance for fact in HDS証拠事実(模型核)))

    def test_影響座標だけを局所留保し無関係な確定関係は使える(self) -> None:
        模型核 = K3相当能力核()
        資料 = _資料((HDS残差("res:note", "note_unresolved", "uncertain note", "注記だけ未解", 影響座標=("note",)),))
        ingest = HDSIR知識適合器(模型核).投入(資料, provenance=("fixture", "doc:local"))
        question, choices = _question()
        結果 = HDSIRネイティブ適合器(模型核).実行(question, 候補IR=choices)

        self.assertFalse(ingest.意味_loss)
        self.assertGreaterEqual(ingest.証拠阻害事実数, 1)
        self.assertEqual(結果.状態, "APPROVE", 結果.候補診断)
        self.assertEqual(結果.回答ラベル, "A", 結果.候補診断)

    def test_関係終点が残差影響ならその関係を確定根拠にしない(self) -> None:
        模型核 = K3相当能力核()
        資料 = _資料((HDS残差("res:engine", "entity_unresolved", "engine", "対象同定未解", 影響座標=("engine",)),))
        HDSIR知識適合器(模型核).投入(資料, provenance=("fixture", "doc:blocked-edge"))
        question, choices = _question()
        結果 = HDSIRネイティブ適合器(模型核).実行(question, 候補IR=choices)

        self.assertEqual(結果.状態, "SUSPEND", 結果.候補診断)
        self.assertIsNone(結果.回答ラベル)


if __name__ == "__main__":
    unittest.main()

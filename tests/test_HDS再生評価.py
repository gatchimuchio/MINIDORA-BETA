from __future__ import annotations

import unittest

from minidora.HDS中間表現 import HDSIR, HDS実行核, HDS座標, HDS関係
from minidora.HDS再生 import HDSIR辞書化
from minidora.HDS再生評価 import HDS再生Case評価, HDS再生評価


def _ir(text: str, coords: tuple[HDS座標, ...], relations: tuple[HDS関係, ...] = ()) -> HDSIR:
    return HDSIR(
        原文=text,
        正規化文=text,
        認知世界ID='再生-評価-test',
        座標=coords,
        関係=relations,
        残差=(),
        意味作用履歴=(),
        実行核=HDS実行核("意味構造転送"),
        種別="意味構造",
        閉包状態='CLOSED_FOR_意味_TRANSFER',
        入力言語="en",
    )


def _question() -> HDSIR:
    return _ir(
        "What does Alpha use?",
        (
            HDS座標("alpha", "対象.実体", "Alpha"),
            HDS座標('選択肢:A', "目的.候補", "engine"),
            HDS座標('選択肢:B', "目的.候補", "stone"),
        ),
    )


def _候補(text: str) -> HDSIR:
    return _ir(text, (HDS座標('候補', "対象.実体", text),))


def _資料(target: str) -> HDSIR:
    return _ir(
        f"Alpha uses {target}.",
        (
            HDS座標("alpha", "対象.実体", "Alpha"),
            HDS座標("target", "対象.実体", target),
        ),
        (HDS関係("use", ("alpha",), ("target",), "作用"),),
    )


class HDS再生評価試験(unittest.TestCase):
    def test_情報源_信頼度を再生して高信頼情報源側を選ぶ(self) -> None:
        row = {
            "契約形式": 'minidora.hds-選択肢-再生.v1',
            "id": '信頼度-case',
            "question_ir": HDSIR辞書化(_question()),
            "choices_ir": {
                "A": HDSIR辞書化(_候補("engine")),
                "B": HDSIR辞書化(_候補("stone")),
            },
            '資料': [
                {
                    "provenance": ["fixture", "doc:engine"],
                    '情報源_信頼度': 0.2,
                    "ir": HDSIR辞書化(_資料("engine")),
                },
                {
                    "provenance": ["fixture", "doc:stone"],
                    '情報源_信頼度': 0.9,
                    "ir": HDSIR辞書化(_資料("stone")),
                },
            ],
            "gold": "B",
        }

        detail = HDS再生Case評価(row)
        self.assertEqual(detail.状態, "APPROVE", detail.候補診断)
        self.assertEqual(detail.予測, "B", detail.候補診断)
        self.assertTrue(detail.正解)

    def test_集計は回答率正答率理由努力分布を返す(self) -> None:
        row = {
            "契約形式": 'minidora.hds-選択肢-再生.v1',
            "id": "one",
            "question_ir": HDSIR辞書化(_question()),
            "choices_ir": {
                "A": HDSIR辞書化(_候補("engine")),
                "B": HDSIR辞書化(_候補("stone")),
            },
            '資料': [
                {
                    "provenance": ["fixture", "doc:engine"],
                    '情報源_信頼度': 1.0,
                    "ir": HDSIR辞書化(_資料("engine")),
                }
            ],
            "gold": "A",
        }
        結果 = HDS再生評価((row,))
        self.assertEqual(結果["total"], 1)
        self.assertEqual(結果["with_gold"], 1)
        self.assertEqual(結果["correct"], 1)
        self.assertEqual(結果["answered"], 1)
        self.assertEqual(結果["suspended"], 0)
        self.assertIn(結果["details"][0]['計算量'], {"low", "high", "max"})


if __name__ == "__main__":
    unittest.main()

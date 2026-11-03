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
        認知世界ID="test",
        座標=coords,
        関係=relations,
        残差=(),
        意味作用履歴=(),
        実行核=HDS実行核("意味構造転送"),
        種別="意味構造",
        閉包状態='CLOSED_FOR_意味_TRANSFER',
        入力言語="en",
    )


def _候補(a: str, b: str) -> dict[str, HDSIR]:
    return {
        "A": _ir(a, (HDS座標("a", "対象.実体", a, 原文範囲=(0, len(a))),)),
        "B": _ir(b, (HDS座標("b", "対象.実体", b, 原文範囲=(0, len(b))),)),
    }


class HDS構造照合試験(unittest.TestCase):
    def test_問題候補資料を全てHDS構造で照合する(self) -> None:
        模型核 = K3相当能力核()
        資料_ir = _ir(
            "Alpha uses an engine.",
            (
                HDS座標("alpha", "対象.実体", "Alpha", 原文範囲=(0, 5)),
                HDS座標("use", "関係.述語表層", "uses", 原文範囲=(6, 10)),
                HDS座標("engine", "対象.実体", "engine", 原文範囲=(14, 20)),
            ),
            (HDS関係("r", ("alpha",), ("engine",), "作用"),),
        )
        HDSIR知識適合器(模型核).投入(資料_ir, provenance=('test-資料', "doc:1"))

        question_ir = _ir(
            "What does Alpha use?",
            (
                HDS座標("q-alpha", "対象.実体", "Alpha", 原文範囲=(10, 15)),
                HDS座標("q-use", "関係.述語表層", "use", 原文範囲=(16, 19)),
                HDS座標('選択肢:A', "目的.候補", "engine"),
                HDS座標('選択肢:B', "目的.候補", "stone"),
            ),
            (HDS関係("qr", ("q-alpha",), ("q-use",), "記述→問い"),),
        )

        結果 = HDSIRネイティブ適合器(模型核).実行(question_ir, 候補IR=_候補("engine", "stone"))
        self.assertEqual(結果.状態, "APPROVE")
        self.assertEqual(結果.回答ラベル, "A")
        self.assertGreater(結果.根拠事実数, 0)

    def test_複数HDS関係を跨いで候補へ到達する(self) -> None:
        模型核 = K3相当能力核()
        first = _ir(
            "Alpha activates Beta.",
            (
                HDS座標("alpha", "対象.実体", "Alpha", 原文範囲=(0, 5)),
                HDS座標("beta", "対象.実体", "Beta", 原文範囲=(16, 20)),
            ),
            (HDS関係("r1", ("alpha",), ("beta",), "因果"),),
        )
        second = _ir(
            "Beta produces Engine.",
            (
                HDS座標("beta", "対象.実体", "Beta", 原文範囲=(0, 4)),
                HDS座標("engine", "対象.実体", "Engine", 原文範囲=(14, 20)),
            ),
            (HDS関係("r2", ("beta",), ("engine",), "因果"),),
        )
        HDSIR知識適合器(模型核).投入(first, provenance=('test-資料', "doc:1"))
        HDSIR知識適合器(模型核).投入(second, provenance=('test-資料', "doc:2"))

        question_ir = _ir(
            "What follows from Alpha?",
            (
                HDS座標("alpha", "対象.実体", "Alpha", 原文範囲=(18, 23)),
                HDS座標('選択肢:A', "目的.候補", "Engine"),
                HDS座標('選択肢:B', "目的.候補", "Stone"),
            ),
        )
        結果 = HDSIRネイティブ適合器(模型核).実行(question_ir, 候補IR=_候補("Engine", "Stone"))
        self.assertEqual(結果.状態, "APPROVE")
        self.assertEqual(結果.回答ラベル, "A")
        self.assertGreaterEqual(結果.根拠事実数, 2)

    def test_関係が一致し候補差が無いなら推測しない(self) -> None:
        模型核 = K3相当能力核()
        for doc_id, obj in (("1", "engine"), ("2", "stone")):
            資料_ir = _ir(
                f"Alpha uses {obj}.",
                (
                    HDS座標("alpha", "対象.実体", "Alpha", 原文範囲=(0, 5)),
                    HDS座標("use", "関係.述語表層", "uses", 原文範囲=(6, 10)),
                    HDS座標("obj", "対象.実体", obj, 原文範囲=(11, 11 + len(obj))),
                ),
                (HDS関係("r", ("alpha",), ("obj",), "作用"),),
            )
            HDSIR知識適合器(模型核).投入(資料_ir, provenance=('test-資料', "doc:" + doc_id))

        question_ir = _ir(
            "What does Alpha use?",
            (
                HDS座標("q-alpha", "対象.実体", "Alpha", 原文範囲=(10, 15)),
                HDS座標('選択肢:A', "目的.候補", "engine"),
                HDS座標('選択肢:B', "目的.候補", "stone"),
            ),
        )
        結果 = HDSIRネイティブ適合器(模型核).実行(question_ir, 候補IR=_候補("engine", "stone"))
        self.assertEqual(結果.状態, "SUSPEND")
        self.assertIsNone(結果.回答ラベル)
        self.assertIn('AMBIGUOUS_証拠', 結果.理由)

    def test_問い候補関係図で英語屈折差を共有正規化する(self) -> None:
        模型核 = K3相当能力核()
        資料_ir = _ir(
            "Alpha uses engines.",
            (
                HDS座標("alpha", "対象.実体", "Alpha", 原文範囲=(0, 5)),
                HDS座標("engines", "対象.実体", "engines", 原文範囲=(11, 18)),
            ),
            (HDS関係("r", ("alpha",), ("engines",), "作用"),),
        )
        HDSIR知識適合器(模型核).投入(資料_ir, provenance=('test-資料', "doc:plural"))
        question_ir = _ir(
            "What does Alpha use?",
            (
                HDS座標("alpha", "対象.実体", "Alpha", 原文範囲=(10, 15)),
                HDS座標('選択肢:A', "目的.候補", "engine"),
                HDS座標('選択肢:B', "目的.候補", "stone"),
            ),
        )
        結果 = HDSIRネイティブ適合器(模型核).実行(question_ir, 候補IR=_候補("engine", "stone"))
        self.assertEqual(結果.状態, "APPROVE")
        self.assertEqual(結果.回答ラベル, "A")

    def test_同一HDS文書に分散した意味を低重みで再統合する(self) -> None:
        模型核 = K3相当能力核()
        資料_ir = _ir(
            "ProteinX has a documented catalytic function.",
            (
                HDS座標("protein", "対象.実体", "ProteinX", 原文範囲=(0, 8)),
                # Compiler導出座標を想定し、原文範囲を持たない。
                HDS座標("function", "対象.機能", "catalysis"),
            ),
        )
        HDSIR知識適合器(模型核).投入(資料_ir, provenance=('test-資料', "doc:distributed"))
        question_ir = _ir(
            "Which function belongs to ProteinX?",
            (
                HDS座標("protein", "対象.実体", "ProteinX", 原文範囲=(26, 34)),
                HDS座標('選択肢:A', "目的.候補", "catalysis"),
                HDS座標('選択肢:B', "目的.候補", "transport"),
            ),
        )
        結果 = HDSIRネイティブ適合器(模型核).実行(
            question_ir,
            候補IR=_候補("catalysis", "transport"),
        )
        self.assertEqual(結果.状態, "APPROVE")
        self.assertEqual(結果.回答ラベル, "A")
        self.assertGreaterEqual(結果.根拠事実数, 2)

    def test_別文書の問い語と候補語を勝手に結合しない(self) -> None:
        模型核 = K3相当能力核()
        HDSIR知識適合器(模型核).投入(
            _ir("Alpha is observed.", (HDS座標("alpha", "対象.実体", "Alpha", 原文範囲=(0, 5)),)),
            provenance=('test-資料', "doc:a"),
        )
        HDSIR知識適合器(模型核).投入(
            _ir("Engine is observed.", (HDS座標("engine", "対象.実体", "engine", 原文範囲=(0, 6)),)),
            provenance=('test-資料', "doc:b"),
        )
        question_ir = _ir(
            "What belongs to Alpha?",
            (
                HDS座標("alpha", "対象.実体", "Alpha", 原文範囲=(16, 21)),
                HDS座標('選択肢:A', "目的.候補", "engine"),
                HDS座標('選択肢:B', "目的.候補", "stone"),
            ),
        )
        結果 = HDSIRネイティブ適合器(模型核).実行(question_ir, 候補IR=_候補("engine", "stone"))
        self.assertEqual(結果.状態, "SUSPEND")
        self.assertIsNone(結果.回答ラベル)

    def test_4段未到達時だけ6段まで深さを拡張する(self) -> None:
        模型核 = K3相当能力核()
        連鎖 = ("Alpha", "Beta", "Gamma", "Delta", "Epsilon", "Zeta", "Engine")
        for index, (start, end) in enumerate(zip(連鎖, 連鎖[1:]), start=1):
            関係_ir = _ir(
                f"{start} leads to {end}.",
                (
                    HDS座標("start", "対象.実体", start, 原文範囲=(0, len(start))),
                    HDS座標("end", "対象.実体", end, 原文範囲=(10, 10 + len(end))),
                ),
                (HDS関係(f"r{index}", ("start",), ("end",), "因果"),),
            )
            HDSIR知識適合器(模型核).投入(関係_ir, provenance=('test-資料', f"doc:hop:{index}"))

        question_ir = _ir(
            "What ultimately follows from Alpha?",
            (
                HDS座標("alpha", "対象.実体", "Alpha", 原文範囲=(29, 34)),
                HDS座標('選択肢:A', "目的.候補", "Engine"),
                HDS座標('選択肢:B', "目的.候補", "Stone"),
            ),
        )
        結果 = HDSIRネイティブ適合器(模型核).実行(question_ir, 候補IR=_候補("Engine", "Stone"))
        self.assertEqual(結果.状態, "APPROVE")
        self.assertEqual(結果.回答ラベル, "A")
        self.assertGreaterEqual(結果.根拠事実数, 6)


if __name__ == "__main__":
    unittest.main()

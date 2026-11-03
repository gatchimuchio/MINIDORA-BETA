from __future__ import annotations

import unittest

from minidora.HDS資料K import HDSIR知識適合器
from minidora.HDS中間表現 import HDSIR, HDS実行核, HDS座標, HDS関係
from minidora.K3機能 import K3相当能力核
from minidora.K3_HDSネイティブ import HDSIRネイティブ適合器, _edge_similarity, _意味署名


def _ir(text: str, coords: tuple[HDS座標, ...], relations: tuple[HDS関係, ...] = ()) -> HDSIR:
    return HDSIR(
        原文=text,
        正規化文=text,
        認知世界ID='directed-関係-test',
        座標=coords,
        関係=relations,
        残差=(),
        意味作用履歴=(),
        実行核=HDS実行核("意味構造転送"),
        種別="意味構造",
        閉包状態='CLOSED_FOR_意味_TRANSFER',
        入力言語="en",
    )


def _方向候補(text: str, *, reverse: bool) -> HDSIR:
    starts = ("beta",) if reverse else ("alpha",)
    ends = ("alpha",) if reverse else ("beta",)
    return _ir(
        text,
        (
            HDS座標("alpha", "対象.実体", "Alpha", 原文範囲=(0, 5)),
            HDS座標("beta", "対象.実体", "Beta", 原文範囲=(6, 10)),
        ),
        (HDS関係('候補-direction', starts, ends, "因果"),),
    )


def _question(*, with_関係: bool = False) -> HDSIR:
    relations = (HDS関係('question-文脈', ("alpha",), ("beta",), "因果"),) if with_関係 else ()
    return _ir(
        "Which causal direction is supported?",
        (
            HDS座標("alpha", "対象.実体", "Alpha"),
            HDS座標("beta", "対象.実体", "Beta"),
            HDS座標('選択肢:A', "目的.候補", "Alpha causes Beta"),
            HDS座標('選択肢:B', "目的.候補", "Beta causes Alpha"),
        ),
        relations,
    )


def _run_direction_case(*, question_関係: bool = False):
    模型核 = K3相当能力核()
    資料_ir = _ir(
        "Alpha causes Beta.",
        (
            HDS座標("alpha", "対象.実体", "Alpha", 原文範囲=(0, 5)),
            HDS座標("beta", "対象.実体", "Beta", 原文範囲=(13, 17)),
        ),
        (HDS関係('資料-direction', ("alpha",), ("beta",), "因果"),),
    )
    HDSIR知識適合器(模型核).投入(資料_ir, provenance=('test-資料', "doc:direction"))
    candidates = {
        "A": _方向候補("Alpha causes Beta", reverse=False),
        "B": _方向候補("Beta causes Alpha", reverse=True),
    }
    return HDSIRネイティブ適合器(模型核).実行(_question(with_関係=question_関係), 候補IR=candidates)


class HDS関係方向試験(unittest.TestCase):
    def test_同じ語と関係種別でも逆向きは一致扱いしない(self) -> None:
        forward = _意味署名(_方向候補("Alpha causes Beta", reverse=False))
        reverse = _意味署名(_方向候補("Beta causes Alpha", reverse=True))
        証拠 = _意味署名(_方向候補("Alpha causes Beta", reverse=False))

        self.assertEqual(forward.語, reverse.語)
        self.assertEqual(forward.関係種別, reverse.関係種別)
        self.assertEqual(_edge_similarity(forward.関係辺, 証拠.関係辺), 1.0)
        self.assertEqual(_edge_similarity(reverse.関係辺, 証拠.関係辺), 0.0)

    def test_資料が支持する関係方向を選択する(self) -> None:
        結果 = _run_direction_case()

        self.assertEqual(結果.状態, "APPROVE")
        self.assertEqual(結果.回答ラベル, "A")
        diagnostics = {item.候補: item for item in 結果.候補診断}
        self.assertGreater(diagnostics["A"].証拠得点, diagnostics["B"].証拠得点)
        self.assertGreaterEqual(diagnostics["A"].識別一致出典数, 1)

    def test_問い側の共通方向を逆向き候補へ伝染させない(self) -> None:
        結果 = _run_direction_case(question_関係=True)

        self.assertEqual(結果.状態, "APPROVE")
        self.assertEqual(結果.回答ラベル, "A")
        diagnostics = {item.候補: item for item in 結果.候補診断}
        self.assertGreater(diagnostics["A"].証拠得点, diagnostics["B"].証拠得点)
        self.assertGreaterEqual(diagnostics["A"].識別一致出典数, 1)
        self.assertEqual(diagnostics["B"].識別一致出典数, 0)


if __name__ == "__main__":
    unittest.main()

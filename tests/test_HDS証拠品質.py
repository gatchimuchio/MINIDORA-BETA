from __future__ import annotations

import unittest

from minidora.HDS資料K import HDSIR知識適合器
from minidora.HDS中間表現 import HDSIR, HDS実行核, HDS座標, HDS関係, 値状態
from minidora.K3機能 import K3相当能力核
from minidora.K3_HDSネイティブ import HDSIRネイティブ適合器


def _ir(
    text: str,
    coords: tuple[HDS座標, ...],
    relations: tuple[HDS関係, ...] = (),
) -> HDSIR:
    return HDSIR(
        原文=text,
        正規化文=text,
        認知世界ID='証拠-test',
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
            HDS座標("alpha", "対象.実体", "Alpha", 原文範囲=(10, 15)),
            HDS座標('選択肢:A', "目的.候補", "engine"),
            HDS座標('選択肢:B', "目的.候補", "stone"),
        ),
    )


def _candidates() -> dict[str, HDSIR]:
    return {
        "A": _ir("engine", (HDS座標("a", "対象.実体", "engine", 原文範囲=(0, 6)),)),
        "B": _ir("stone", (HDS座標("b", "対象.実体", "stone", 原文範囲=(0, 5)),)),
    }


def _use_ir(obj: str, *, 状態: 値状態 = 値状態.確定) -> HDSIR:
    return _ir(
        f"Alpha uses {obj}.",
        (
            HDS座標("alpha", "対象.実体", "Alpha", 原文範囲=(0, 5)),
            HDS座標("obj", "対象.実体", obj, 原文範囲=(11, 11 + len(obj))),
        ),
        (HDS関係("use", ("alpha",), ("obj",), "作用", 値状態=状態),),
    )


class HDS証拠品質試験(unittest.TestCase):
    def test_独立2情報源の支持を1情報源へ潰さない(self) -> None:
        模型核 = K3相当能力核()
        適合器 = HDSIR知識適合器(模型核)
        適合器.投入(_use_ir("engine"), provenance=("web", "doc:engine:1"))
        適合器.投入(_use_ir("engine"), provenance=("web", "doc:engine:2"))
        適合器.投入(_use_ir("stone"), provenance=("web", "doc:stone:1"))

        結果 = HDSIRネイティブ適合器(模型核).実行(_question(), 候補IR=_candidates())
        self.assertEqual(結果.状態, "APPROVE", 結果.候補診断)
        self.assertEqual(結果.回答ラベル, "A", 結果.候補診断)
        engine_候補 = next(c for c in 結果.候補 if c.answer == "A")
        self.assertGreaterEqual(len(engine_候補.proof_fact_ids), 2)

    def test_未確定関係は同一文書共起だけで確定根拠へ昇格しない(self) -> None:
        模型核 = K3相当能力核()
        HDSIR知識適合器(模型核).投入(
            _use_ir("engine", 状態=値状態.未確定),
            provenance=("web", "doc:uncertain"),
        )

        結果 = HDSIRネイティブ適合器(模型核).実行(_question(), 候補IR=_candidates())
        self.assertEqual(結果.状態, "SUSPEND", 結果.候補診断)
        self.assertIsNone(結果.回答ラベル)

    def test_確定関係は同じ資料で承認可能(self) -> None:
        模型核 = K3相当能力核()
        HDSIR知識適合器(模型核).投入(
            _use_ir("engine", 状態=値状態.確定),
            provenance=("web", "doc:confirmed"),
        )

        結果 = HDSIRネイティブ適合器(模型核).実行(_question(), 候補IR=_candidates())
        self.assertEqual(結果.状態, "APPROVE", 結果.候補診断)
        self.assertEqual(結果.回答ラベル, "A", 結果.候補診断)


if __name__ == "__main__":
    unittest.main()

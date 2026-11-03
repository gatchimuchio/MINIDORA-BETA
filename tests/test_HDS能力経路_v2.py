from __future__ import annotations

import unittest

from minidora.HDS中間表現 import HDSIR, HDS実行核, HDS座標
from minidora.HDS参照 import HDS参照問合せ候補
from minidora.hds参照拡張 import HDS追加参照統合上限
from minidora.hds能力経路_v2 import (
    HDS参照検索V2,
    HDS局所観測view,
    HDS能力模型核V2,
)
from minidora.模型 import 成立候補, 言語状態
from minidora.参照 import 参照記録


def _ir() -> HDSIR:
    return HDSIR(
        原文="Which process involving ProteinX under severe hypoxic stress is correct?",
        正規化文="Which process involving ProteinX under severe hypoxic stress is correct?",
        認知世界ID='能力-経路-v2',
        座標=(
            HDS座標("protein", "対象.実体", "ProteinX"),
            HDS座標('関係', "関係.述語表層", "activates"),
            HDS座標('状態', "状態.環境", "severe hypoxic stress"),
            HDS座標('選択肢:A', "目的.候補", "catalysis"),
            HDS座標('選択肢:B', "目的.候補", "transport"),
            HDS座標('選択肢:C', "目的.候補", "folding"),
            HDS座標('選択肢:D', "目的.候補", "signaling"),
        ),
        関係=(),
        残差=(),
        意味作用履歴=(),
        実行核=HDS実行核("参照回答"),
        参照必須=True,
        種別='knowledge_選択肢',
        入力言語="en",
    )


def _問合せ選択肢(record: 参照記録) -> frozenset[str]:
    return frozenset(
        str(value)
        for key, value in record.条件
        if str(key) == 'hds_query_選択肢'
    )


class _CoverageProvider:
    並列安全 = True
    名称 = "coverage-fixture"

    def __init__(self, *, shared_ab: bool = False) -> None:
        self.calls: list[str] = []
        self.primary = HDS参照問合せ候補(_ir())[0]
        self.shared_ab = shared_ab

    def 検索(self, query: str, limit: int = 8):
        self.calls.append(query)
        if query == self.primary:
            return (
                参照記録("generic", "ProteinX", "generic evidence", "fixture://generic", self.名称, 0.8),
            )
        mapping = {
            "ProteinX catalysis": ("shared" if self.shared_ab else "A", "catalysis evidence"),
            "ProteinX transport": ("shared" if self.shared_ab else "B", "transport evidence"),
            "ProteinX folding": ("C", "folding evidence"),
            "ProteinX signaling": ("D", "signaling evidence"),
        }
        payload = mapping.get(query)
        if payload is None:
            return ()
        情報源_id, content = payload
        return (
            参照記録(情報源_id, "ProteinX", content, f"fixture://{情報源_id}", self.名称, 0.82),
        )


class HDS能力経路V2試験(unittest.TestCase):
    def test_追加参照統合は少数新規資料の格納余地を確保する(self) -> None:
        self.assertEqual(HDS追加参照統合上限(16, 4), 20)
        self.assertEqual(HDS追加参照統合上限(16, 16), 32)
        self.assertEqual(HDS追加参照統合上限(32, 4), 32)
        self.assertEqual(HDS追加参照統合上限(0, 1), 1)

    def test_generic主検索だけでは候補被覆を閉じない(self) -> None:
        provider = _CoverageProvider()
        records = HDS参照検索V2(provider, _ir(), 上限=4, 一問合せ上限=1, 最大問合せ並列=1)

        coverage = set()
        for record in records:
            coverage.update(_問合せ選択肢(record))
        self.assertEqual(coverage, set("ABCD"))
        self.assertEqual(len(records), 4)
        self.assertNotIn("generic", {record.識別子 for record in records})
        for query in (
            "ProteinX catalysis",
            "ProteinX transport",
            "ProteinX folding",
            "ProteinX signaling",
        ):
            self.assertIn(query, provider.calls)

    def test_同一情報源の複数候補queryは独立情報源へ増やさない(self) -> None:
        provider = _CoverageProvider(shared_ab=True)
        records = HDS参照検索V2(provider, _ir(), 上限=4, 一問合せ上限=1, 最大問合せ並列=1)

        shared = [record for record in records if record.識別子 == "shared"]
        self.assertEqual(len(shared), 1)
        self.assertEqual(_問合せ選択肢(shared[0]), {"A", "B"})
        coverage = set()
        for record in records:
            coverage.update(_問合せ選択肢(record))
        self.assertEqual(coverage, set("ABCD"))

    def test_local_viewは同情報源置換で信頼度とprovenanceを保持する(self) -> None:
        question = HDSIR(
            原文="Which ProteinX mechanism is correct?",
            正規化文="Which ProteinX mechanism is correct?",
            認知世界ID="local-view",
            座標=(
                HDS座標("protein", "対象.実体", "ProteinX"),
                HDS座標('選択肢:A', "目的.候補", "alpha signaling"),
                HDS座標('選択肢:B', "目的.候補", "beta transport"),
                HDS座標('選択肢:C', "目的.候補", "gamma folding"),
                HDS座標('選択肢:D', "目的.候補", "delta catalysis"),
            ),
            関係=(),
            残差=(),
            意味作用履歴=(),
            実行核=HDS実行核("参照回答"),
            参照必須=True,
            種別='knowledge_選択肢',
            入力言語="en",
        )
        参照 = 参照記録(
            "doi:test",
            "ProteinX",
            "ProteinX mechanism compares alpha signaling and beta transport in a broad overview. "
            "ProteinX beta transport is specifically increased under severe stress and is experimentally observed.",
            "fixture://doi-test",
            "fixture",
            0.82,
            条件=(("hds_query_kind", '選択肢'), ('hds_query_選択肢', "B")),
        )

        projected, changed = HDS局所観測view(question, (参照,))

        self.assertEqual(changed, 1)
        self.assertEqual(len(projected), 1)
        self.assertEqual(projected[0].識別子, 参照.識別子)
        self.assertEqual(projected[0].信頼, 0.82)
        self.assertIn(('hds_query_選択肢', "B"), projected[0].条件)
        self.assertIn(("hds_observation_view", "local"), projected[0].条件)
        self.assertNotEqual(projected[0].内容, 参照.内容)

    def test_v2模型核は同資料候補縮小再投票を行わない(self) -> None:
        模型核 = HDS能力模型核V2()
        結果 = 模型核.評価言語状態(
            言語状態("which"),
            (
                成立候補("A", 言語状態("alpha common x")),
                成立候補("B", 言語状態("beta common x")),
                成立候補("C", 言語状態("beta common y")),
            ),
            参照状態=(
                言語状態("alpha", 識別子="r1"),
                言語状態("beta", 識別子="r2"),
            ),
        )

        self.assertEqual(結果.統計.検査点再活性数, 0)
        self.assertEqual(結果.統計.大域再照合数, 0)
        self.assertEqual(結果.統計.候補横断更新数, 0)
        self.assertEqual(結果.統計.再作用回数, 0)
        self.assertFalse(any(cp.段階.startswith("RECONCILE_") for cp in 結果.検査点))


if __name__ == "__main__":
    unittest.main()

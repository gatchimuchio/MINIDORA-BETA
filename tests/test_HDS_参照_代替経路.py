from __future__ import annotations

import unittest

from minidora.HDS中間表現 import HDSIR, HDS実行核, HDS座標, HDS関係, 値状態
from minidora.HDS参照 import HDS参照問合せ候補, HDS参照検索, HDS参照縮退問合せ候補
from minidora.参照 import 参照記録


def _ir() -> HDSIR:
    return HDSIR(
        原文="Which process involving ProteinX under severe hypoxic stress is correct?",
        正規化文="Which process involving ProteinX under severe hypoxic stress is correct?",
        認知世界ID='代替経路-test',
        座標=(
            HDS座標("unknown", "目的.未知始点", "process", 値状態.未観測),
            HDS座標("protein", "対象.実体", "ProteinX"),
            HDS座標("search", "検索.英語正規化", "process involving ProteinX under severe hypoxic stress"),
            HDS座標('選択肢:A', "目的.候補", "catalysis"),
            HDS座標('選択肢:B', "目的.候補", "transport"),
            HDS座標('選択肢:C', "目的.候補", "folding"),
            HDS座標('選択肢:D', "目的.候補", "signaling"),
        ),
        関係=(
            HDS関係(
                "question",
                ("unknown",),
                ("protein",),
                "関連",
                条件=("検索述語=involving", "不足位置=始点", "条件範囲=under severe hypoxic stress"),
                値状態=値状態.未観測,
            ),
        ),
        残差=(),
        意味作用履歴=(),
        実行核=HDS実行核("参照回答"),
        参照必須=True,
        種別='knowledge_選択肢',
        入力言語="en",
    )


class _代替経路OnlyProvider:
    並列安全 = True
    名称 = '代替経路-only'

    def __init__(self) -> None:
        self.calls: list[str] = []

    def 検索(self, query: str, limit: int = 8):
        self.calls.append(query)
        # rich queryは0件。縮退した `ProteinX catalysis` だけが取得可能。
        if query == "ProteinX catalysis":
            return (
                参照記録("hit", "ProteinX", "ProteinX catalysis evidence", "fixture://hit", self.名称),
            )
        return ()


class HDS参照代替経路試験(unittest.TestCase):
    def test_縮退queryは全選択肢を対称に保持する(self) -> None:
        queries = HDS参照縮退問合せ候補(_ir())
        for 選択肢 in ("catalysis", "transport", "folding", "signaling"):
            self.assertIn(f"ProteinX {選択肢}", queries)

    def test_主検索が完全0件の時だけ縮退検索へ進む(self) -> None:
        provider = _代替経路OnlyProvider()
        primary = HDS参照問合せ候補(_ir())
        self.assertNotIn("ProteinX catalysis", primary)

        records = HDS参照検索(provider, _ir())
        self.assertEqual([record.識別子 for record in records], ["hit"])
        self.assertTrue(all(query in provider.calls for query in primary))
        self.assertIn("ProteinX catalysis", provider.calls)

    def test_一般queryが1件hitしても必須観測未被覆なら縮退検索を継続する(self) -> None:
        target_primary = "process involving ProteinX under severe hypoxic stress"

        class PrimaryProvider(_代替経路OnlyProvider):
            def 検索(self, query: str, limit: int = 8):
                self.calls.append(query)
                if query == target_primary:
                    return (
                        参照記録("primary", "ProteinX", "primary evidence", "fixture://primary", self.名称),
                    )
                if query == "ProteinX catalysis":
                    return (
                        参照記録("hit", "ProteinX", "ProteinX catalysis evidence", "fixture://hit", self.名称),
                    )
                return ()

        provider = PrimaryProvider()
        records = HDS参照検索(provider, _ir())
        self.assertEqual({record.識別子 for record in records}, {"primary", "hit"})
        self.assertIn("ProteinX catalysis", provider.calls)


if __name__ == "__main__":
    unittest.main()

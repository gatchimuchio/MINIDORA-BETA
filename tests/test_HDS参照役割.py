from __future__ import annotations

import unittest

from minidora.HDS中間表現 import HDSIR, HDS実行核, HDS座標
from minidora.HDS観測計画 import HDS参照観測要求
from minidora.HDS参照 import HDS参照問合せ候補, HDS参照縮退問合せ候補


def _ir() -> HDSIR:
    return HDSIR(
        原文="raw text that R must not reinterpret",
        正規化文="raw text that R must not reinterpret",
        認知世界ID="参照-降下-test",
        座標=(HDS座標("raw", "対象.実体", "RAW_ONLY_MARKER"),),
        関係=(),
        残差=(),
        意味作用履歴=(),
        実行核=HDS実行核("参照回答"),
        参照必須=True,
        種別="knowledge_query",
        閉包状態="作用閉包",
        入力言語="en",
    )


def _要求(
    ID: str,
    表層: str,
    *,
    必須: bool = False,
    段階: str = "primary",
    候補: str | None = None,
    関係ID: str | None = "r1",
) -> HDS参照観測要求:
    return HDS参照観測要求(
        ID=ID,
        関係ID=関係ID,
        関係種別="因果" if 関係ID else None,
        未知位置="始点" if 関係ID else None,
        既知端点=("target",) if 関係ID else (),
        条件範囲=(),
        候補ラベル=候補,
        候補表層=None if 候補 is None else f"choice-{候補}",
        外部言語="en",
        外部検索表層=表層,
        必須被覆=必須,
        段階=段階,
        provenance=("構文化器Kernel",),
    )


class HDS参照役割降下試験(unittest.TestCase):
    def test_Rは構文化器観測要求の検索表層だけを降下する(self) -> None:
        requests = (
            _要求("obs:A", "ProteinX activates apoptosis under hypoxia", 必須=True, 候補="A"),
            _要求("obs:B", "ProteinY activates apoptosis under hypoxia", 必須=True, 候補="B"),
        )
        queries = HDS参照問合せ候補(_ir(), 観測要求=requests)
        self.assertEqual(queries, tuple(x.外部検索表層 for x in requests))
        self.assertFalse(any("RAW_ONLY_MARKER" in query for query in queries))

    def test_必須観測は最大候補数より優先して全保持する(self) -> None:
        requests = tuple(
            _要求(f"obs:{label}", f"candidate {label} evidence", 必須=True, 候補=label)
            for label in ("A", "B", "C", "D")
        ) + (_要求("optional", "optional surface", 必須=False, 関係ID=None),)
        queries = HDS参照問合せ候補(_ir(), 最大候補数=2, 観測要求=requests)
        self.assertEqual(len(queries), 4)
        self.assertNotIn("optional surface", queries)

    def test_主観測と縮退観測を別入口へ降下する(self) -> None:
        requests = (
            _要求("primary:A", "primary A", 必須=True, 候補="A"),
            _要求("fallback:A", "fallback A", 段階="fallback", 候補="A"),
            _要求("fallback:audit", "audit fallback", 段階="fallback", 関係ID=None),
        )
        self.assertEqual(HDS参照問合せ候補(_ir(), 観測要求=requests), ("primary A",))
        self.assertEqual(
            HDS参照縮退問合せ候補(_ir(), 観測要求=requests),
            ("fallback A", "audit fallback"),
        )

    def test_長文検索表層は先頭と末尾焦点を保持して360文字以内へ降下する(self) -> None:
        filler = " ".join(f"context{i}" for i in range(120))
        surface = f"AlphaProtein {filler} final_focus_marker"
        query = HDS参照問合せ候補(
            _ir(), 観測要求=(_要求("long", surface, 必須=True),)
        )[0]
        self.assertLessEqual(len(query), 360)
        self.assertIn("AlphaProtein", query)
        self.assertIn("final_focus_marker", query)

    def test_同一検索表層でも観測IDが異なれば観測責任を潰さない(self) -> None:
        requests = (
            _要求("obs:A", "same surface", 必須=True, 候補="A"),
            _要求("obs:B", "same surface", 必須=True, 候補="B"),
        )
        self.assertEqual(
            HDS参照問合せ候補(_ir(), 観測要求=requests),
            ("same surface", "same surface"),
        )


if __name__ == "__main__":
    unittest.main()

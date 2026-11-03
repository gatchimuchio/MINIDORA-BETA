from __future__ import annotations

import unittest

from minidora.HDS選択仮説 import HDS候補代入仮説, HDS候補代入仮説群
from minidora.HDS資料K import HDSIR知識適合器
from minidora.HDS中間表現 import HDSIR, HDS実行核, HDS座標, HDS関係, 値状態
from minidora.K3機能 import K3相当能力核
from minidora.K3_HDSネイティブ import HDSIRネイティブ適合器


def _ir(text: str, coords: tuple[HDS座標, ...], relations: tuple[HDS関係, ...] = ()) -> HDSIR:
    return HDSIR(
        原文=text,
        正規化文=text,
        認知世界ID='選択肢-hypothesis-test',
        座標=coords,
        関係=relations,
        残差=(),
        意味作用履歴=(),
        実行核=HDS実行核("意味構造転送"),
        種別="意味構造",
        閉包状態='CLOSED_FOR_意味_TRANSFER',
        入力言語="en",
    )


def _候補(text: str) -> HDSIR:
    return _ir(text, (HDS座標('候補', "対象.実体", text),))


class HDS候補代入仮説試験(unittest.TestCase):
    def test_未知終点へ候補を代入して有向関係を作る(self) -> None:
        question = _ir(
            "What does Alpha use?",
            (
                HDS座標("alpha", "対象.始点", "Alpha"),
                HDS座標('未知', "目的.未知終点", "object", 値状態.未観測),
            ),
            (
                HDS関係(
                    "qrel", ("alpha",), ('未知',), "使用",
                    条件=("検索述語=uses", "不足位置=終点"), 値状態=値状態.未観測,
                ),
            ),
        )
        結果 = HDS候補代入仮説(question, "A", _候補("engine"))
        coords = 結果.座標辞書()
        hypothesis = [関係 for 関係 in 結果.関係 if 関係.由来 == "HDS候補代入仮説"]
        self.assertEqual(len(hypothesis), 1)
        関係 = hypothesis[0]
        self.assertEqual(関係.種別, "使用")
        self.assertEqual(関係.値状態, 値状態.推定)
        self.assertEqual(str(coords[関係.始点[0]].内容), "Alpha")
        self.assertEqual(str(coords[関係.終点[0]].内容), "engine")

    def test_未知始点へ候補を代入して方向を保持する(self) -> None:
        question = _ir(
            "What produces Product?",
            (
                HDS座標('未知', "目的.未知始点", "agent", 値状態.未観測),
                HDS座標("product", "対象.終点", "Product"),
            ),
            (
                HDS関係(
                    "qrel", ('未知',), ("product",), "生成",
                    条件=("検索述語=produces", "不足位置=始点"), 値状態=値状態.未観測,
                ),
            ),
        )
        結果 = HDS候補代入仮説(question, "B", _候補("Enzyme"))
        coords = 結果.座標辞書()
        関係 = next(関係 for 関係 in 結果.関係 if 関係.由来 == "HDS候補代入仮説")
        self.assertEqual(str(coords[関係.始点[0]].内容), "Enzyme")
        self.assertEqual(str(coords[関係.終点[0]].内容), "Product")

    def test_不足位置が明示されない問いへは介入しない(self) -> None:
        question = _ir(
            "Which option applies?",
            (HDS座標("alpha", "対象.実体", "Alpha"),),
        )
        候補 = _候補("engine")
        self.assertIs(HDS候補代入仮説(question, "A", 候補), 候補)

    def test_語集合が同一文書に共存しても関係端点で候補を分別する(self) -> None:
        模型核 = K3相当能力核()
        資料 = _ir(
            "Alpha uses engine; stone is also mentioned.",
            (
                HDS座標("alpha", "対象.実体", "Alpha"),
                HDS座標("engine", "対象.実体", "engine"),
                HDS座標("stone", "対象.実体", "stone"),
            ),
            (HDS関係("rel", ("alpha",), ("engine",), "使用"),),
        )
        HDSIR知識適合器(模型核).投入(資料, provenance=("fixture", 'doc:関係'))

        question = _ir(
            "What does Alpha use?",
            (
                HDS座標("alpha", "対象.始点", "Alpha"),
                HDS座標('未知', "目的.未知終点", "object", 値状態.未観測),
                HDS座標('選択肢:A', "目的.候補", "engine"),
                HDS座標('選択肢:B', "目的.候補", "stone"),
            ),
            (
                HDS関係(
                    "qrel", ("alpha",), ('未知',), "使用",
                    条件=("検索述語=uses", "不足位置=終点"), 値状態=値状態.未観測,
                ),
            ),
        )
        candidates = HDS候補代入仮説群(question, {"A": _候補("engine"), "B": _候補("stone")})
        結果 = HDSIRネイティブ適合器(模型核).実行(question, 候補IR=candidates)
        self.assertEqual(結果.状態, "APPROVE")
        self.assertEqual(結果.回答ラベル, "A")
        a = next(item for item in 結果.候補診断 if item.候補 == "A")
        b = next(item for item in 結果.候補診断 if item.候補 == "B")
        self.assertGreater(a.合計得点, b.合計得点)


if __name__ == "__main__":
    unittest.main()

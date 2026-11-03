from __future__ import annotations

import unittest

from minidora.HDS選択仮説 import HDS候補代入仮説群
from minidora.HDS資料K import HDSIR知識適合器
from minidora.HDS直接関係検証 import HDS直接関係検証
from minidora.HDS中間表現 import HDSIR, HDS実行核, HDS座標, HDS関係, 値状態
from minidora.K3機能 import K3相当能力核


def _ir(text: str, coords: tuple[HDS座標, ...], relations: tuple[HDS関係, ...] = ()) -> HDSIR:
    return HDSIR(
        原文=text,
        正規化文=text,
        認知世界ID='direct-関係-検証器-test',
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


def _candidates(question: HDSIR) -> dict[str, HDSIR]:
    raw = {
        "A": _ir("engine", (HDS座標("a", "対象.実体", "engine"),)),
        "B": _ir("stone", (HDS座標("b", "対象.実体", "stone"),)),
    }
    return HDS候補代入仮説群(question, raw)


def _主張候補(obj: str, *, negative: bool = False, origin: str = '公開HDS 構文化器') -> HDSIR:
    coords = [
        HDS座標("s", "対象.始点", "Alpha"),
        HDS座標("o", "対象.終点", obj),
    ]
    if negative:
        coords.append(HDS座標("neg", "状態.否定", "not"))
    return _ir(
        f"Alpha uses {obj}.",
        tuple(coords),
        (
            HDS関係(
                '候補-rel', ("s",), ("o",), "使用",
                値状態=値状態.確定, 由来=origin,
            ),
        ),
    )


def _資料(主体: str, obj: str) -> HDSIR:
    return _ir(
        f"{主体} uses {obj}.",
        (
            HDS座標("s", "対象.実体", 主体),
            HDS座標("o", "対象.実体", obj),
        ),
        (HDS関係("r", ("s",), ("o",), "使用"),),
    )


class HDS直接関係検証試験(unittest.TestCase):
    def test_同じ関係種別でも始点が違えば支持しない(self) -> None:
        question = _question()
        模型核 = K3相当能力核()
        HDSIR知識適合器(模型核).投入(_資料("Beta", "engine"), provenance=("fixture", "doc:beta"))
        候補, diagnostics = HDS直接関係検証(模型核, _candidates(question))
        self.assertIsNone(候補)
        self.assertTrue(all(item.得点 == 0.0 for item in diagnostics))

    def test_始点関係終点が一致した候補だけを選ぶ(self) -> None:
        question = _question()
        模型核 = K3相当能力核()
        HDSIR知識適合器(模型核).投入(_資料("Alpha", "engine"), provenance=("fixture", "doc:engine"))
        候補, diagnostics = HDS直接関係検証(模型核, _candidates(question))
        self.assertIsNotNone(候補)
        assert 候補 is not None
        self.assertEqual(候補.answer, "A")
        self.assertTrue(候補.proof_fact_ids)
        a = next(item for item in diagnostics if item.候補 == "A")
        b = next(item for item in diagnostics if item.候補 == "B")
        self.assertGreater(a.得点, b.得点)

    def test_競合候補も同じ問い関係で支持されれば決め打ちしない(self) -> None:
        question = _question()
        模型核 = K3相当能力核()
        適合器 = HDSIR知識適合器(模型核)
        適合器.投入(_資料("Alpha", "engine"), provenance=("fixture", "doc:engine"))
        適合器.投入(_資料("Alpha", "stone"), provenance=("fixture", "doc:stone"))
        候補, diagnostics = HDS直接関係検証(模型核, _candidates(question))
        self.assertIsNone(候補)
        self.assertGreater(next(x for x in diagnostics if x.候補 == "A").得点, 0.0)
        self.assertGreater(next(x for x in diagnostics if x.候補 == "B").得点, 0.0)

    def test_同一情報源の複数Factを独立根拠として水増ししない(self) -> None:
        question = _question()
        模型核 = K3相当能力核()
        適合器 = HDSIR知識適合器(模型核)
        適合器.投入(_資料("Alpha", "engine"), provenance=("fixture", "doc:same"))
        適合器.投入(_資料("Alpha", "engine"), provenance=("fixture", "doc:same"))
        _候補, diagnostics = HDS直接関係検証(模型核, _candidates(question))
        a = next(item for item in diagnostics if item.候補 == "A")
        self.assertEqual(a.独立出典数, 1)

    def test_候補自身の完全命題は二独立情報源で直接検証できる(self) -> None:
        模型核 = K3相当能力核()
        適合器 = HDSIR知識適合器(模型核)
        適合器.投入(_資料("Alpha", "engine"), provenance=("fixture", "doc:1"))
        適合器.投入(_資料("Alpha", "engine"), provenance=("fixture", "doc:2"))
        candidates = {
            "A": _主張候補("engine"),
            "B": _主張候補("stone"),
        }
        候補, diagnostics = HDS直接関係検証(模型核, candidates)
        self.assertIsNotNone(候補)
        assert 候補 is not None
        self.assertEqual(候補.answer, "A")
        a = next(item for item in diagnostics if item.候補 == "A")
        self.assertEqual(a.命題一致出典数, 2)
        self.assertEqual(a.仮説一致出典数, 0)

    def test_共有言語基底P由来の安全な完全命題も二独立情報源で検証する(self) -> None:
        模型核 = K3相当能力核()
        適合器 = HDSIR知識適合器(模型核)
        適合器.投入(_資料("Alpha", "engine"), provenance=("fixture", "doc:1"))
        適合器.投入(_資料("Alpha", "engine"), provenance=("fixture", "doc:2"))
        candidates = {
            "A": _主張候補("engine", origin="共有言語基底P"),
            "B": _主張候補("stone", origin="共有言語基底P"),
        }
        候補, diagnostics = HDS直接関係検証(模型核, candidates)
        self.assertIsNotNone(候補)
        assert 候補 is not None
        self.assertEqual(候補.answer, "A")
        a = next(item for item in diagnostics if item.候補 == "A")
        self.assertEqual(a.命題一致出典数, 2)

    def test_実行系や未知由来の候補関係を命題として採用しない(self) -> None:
        模型核 = K3相当能力核()
        適合器 = HDSIR知識適合器(模型核)
        適合器.投入(_資料("Alpha", "engine"), provenance=("fixture", "doc:1"))
        適合器.投入(_資料("Alpha", "engine"), provenance=("fixture", "doc:2"))
        candidates = {
            "A": _主張候補("engine", origin='HDS 実行系 K質問射影'),
            "B": _主張候補("stone", origin='HDS 実行系 K質問射影'),
        }
        候補, diagnostics = HDS直接関係検証(模型核, candidates)
        self.assertIsNone(候補)
        self.assertTrue(all(item.命題一致出典数 == 0 for item in diagnostics))

    def test_完全命題は単一情報源だけでは決め打ちしない(self) -> None:
        模型核 = K3相当能力核()
        HDSIR知識適合器(模型核).投入(_資料("Alpha", "engine"), provenance=("fixture", "doc:1"))
        candidates = {
            "A": _主張候補("engine"),
            "B": _主張候補("stone"),
        }
        候補, diagnostics = HDS直接関係検証(模型核, candidates)
        self.assertIsNone(候補)
        self.assertEqual(next(item for item in diagnostics if item.候補 == "A").命題一致出典数, 1)

    def test_否定候補を肯定資料から直接証明しない(self) -> None:
        模型核 = K3相当能力核()
        適合器 = HDSIR知識適合器(模型核)
        適合器.投入(_資料("Alpha", "engine"), provenance=("fixture", "doc:1"))
        適合器.投入(_資料("Alpha", "engine"), provenance=("fixture", "doc:2"))
        candidates = {
            "A": _主張候補("engine", negative=True),
            "B": _主張候補("stone"),
        }
        候補, diagnostics = HDS直接関係検証(模型核, candidates)
        self.assertIsNone(候補)
        self.assertEqual(next(item for item in diagnostics if item.候補 == "A").得点, 0.0)


if __name__ == "__main__":
    unittest.main()

from __future__ import annotations

import unittest

from minidora.HDS中間表現 import HDSIR, HDS実行核, HDS座標
from minidora.HDS参照 import HDS参照問合せ候補, HDS参照検索, HDS参照縮退問合せ候補
from minidora.実行系 import ミニドラ, 要求
from minidora.参照 import 参照記録
from minidora.命令 import 作用, 命令, 手順
from minidora.採否 import 実行状態


class _記録Provider:
    名称 = "query-recording-provider"

    def __init__(self) -> None:
        self.queries: list[str] = []

    def 検索(self, 問合せ: str, 上限: int = 8) -> tuple[参照記録, ...]:
        self.queries.append(問合せ)
        if "catalysis" in 問合せ.casefold():
            return (
                参照記録("doc:catalysis", "ProteinX", "ProteinX supports catalysis.", "fixture://catalysis", self.名称),
            )[:上限]
        if "transport" in 問合せ.casefold():
            return (
                参照記録("doc:transport", "ProteinX", "ProteinX transport hypothesis.", "fixture://transport", self.名称),
            )[:上限]
        return ()


class _共通記録Provider:
    名称 = "shared-query-provider"

    def 検索(self, 問合せ: str, 上限: int = 8) -> tuple[参照記録, ...]:
        if "catalysis" in 問合せ.casefold() or "transport" in 問合せ.casefold():
            return (
                参照記録("doc:shared", "ProteinX", "ProteinX has a documented function.", "fixture://shared", self.名称),
            )[:上限]
        return ()


class _部分被覆Provider:
    名称 = "partial-coverage-provider"

    def __init__(self) -> None:
        self.queries: list[str] = []

    def 検索(self, 問合せ: str, 上限: int = 8) -> tuple[参照記録, ...]:
        normalized = " ".join(問合せ.casefold().split())
        self.queries.append(normalized)
        if "catalysis" in normalized:
            return (
                参照記録("doc:catalysis", "ProteinX", "ProteinX supports catalysis.", "fixture://catalysis", self.名称),
            )[:上限]
        if normalized == "transport":
            return (
                参照記録("doc:transport", "ProteinX", "ProteinX transports cargo.", "fixture://transport", self.名称),
            )[:上限]
        return ()


def _procedure() -> 手順:
    return 手順(
        "参照先頭回答",
        (
            命令("参照列", 作用.取得, 対象="参照", 更新先="参照列"),
            命令("先頭", 作用.抽出, 引数=("$参照列", 0), 更新先="候補"),
            命令("内容", 作用.抽出, 引数=("$候補", "内容"), 更新先="結果"),
        ),
        由来="fixture",
    )


def _ir() -> HDSIR:
    return HDSIR(
        原文="Which function belongs to ProteinX?",
        正規化文="Which function belongs to ProteinX?",
        認知世界ID='参照:test',
        座標=(
            HDS座標("protein", "対象.実体", "ProteinX"),
            HDS座標("function", "目的.属性", "function"),
            HDS座標('選択肢:A', "目的.候補", "catalysis"),
            HDS座標('選択肢:B', "目的.候補", "transport"),
        ),
        関係=(),
        残差=(),
        意味作用履歴=(),
        実行核=HDS実行核("参照回答", ("protein",), "結果"),
        初期状態={},
        参照必須=True,
        種別="knowledge_query",
        閉包状態="作用閉包",
        手順=_procedure(),
        入力言語="en",
    )


class _構文化器:
    def コンパイル(self, 入力, *, 前回結果=None, HDS履歴=(), 文脈=None):
        return _ir()


class HDS参照拡張試験(unittest.TestCase):
    def test_関係未解決では全候補を対称な縮退queryとして保持する(self) -> None:
        queries = HDS参照縮退問合せ候補(_ir())
        self.assertGreaterEqual(len(queries), 4)
        self.assertTrue(any("catalysis" in q for q in queries))
        self.assertTrue(any("transport" in q for q in queries))
        self.assertEqual(sum("catalysis" in q for q in queries), sum("transport" in q for q in queries))

    def test_問題文だけで0件でも候補展開で資料を取得する(self) -> None:
        provider = _記録Provider()
        records = HDS参照検索(provider, _ir(), 上限=8, 一問合せ上限=4)
        ids = {record.識別子 for record in records}
        self.assertEqual(ids, {"doc:catalysis", "doc:transport"})
        self.assertTrue(any("catalysis" in q.casefold() for q in provider.queries))
        self.assertTrue(any("transport" in q.casefold() for q in provider.queries))
        conditions = {record.識別子: set(record.条件) for record in records}
        self.assertIn(('hds_query_選択肢', "A"), conditions["doc:catalysis"])
        self.assertIn(('hds_query_選択肢', "B"), conditions["doc:transport"])

    def test_同じ文書が複数候補queryで取れた場合は両候補provenanceを保持する(self) -> None:
        records = HDS参照検索(_共通記録Provider(), _ir(), 上限=8, 一問合せ上限=4)
        self.assertEqual(len(records), 1)
        labels = {value for key, value in records[0].条件 if key == 'hds_query_選択肢'}
        self.assertEqual(labels, {"A", "B"})

    def test_一部候補だけ主検索hitでも未被覆候補の縮退検索を継続する(self) -> None:
        provider = _部分被覆Provider()
        records = HDS参照検索(provider, _ir(), 上限=8, 一問合せ上限=4)
        self.assertEqual({record.識別子 for record in records}, {"doc:catalysis", "doc:transport"})
        conditions = {record.識別子: set(record.条件) for record in records}
        self.assertIn(('hds_query_選択肢', "A"), conditions["doc:catalysis"])
        self.assertIn(('hds_query_選択肢', "B"), conditions["doc:transport"])
        self.assertIn("transport", provider.queries)

    def test_実行系_HDS経路で展開検索を使用する(self) -> None:
        provider = _記録Provider()
        結果 = ミニドラ(provider, HDSコンパイラ_=_構文化器()).実行(要求("surface query"))
        self.assertEqual(結果.採否.状態, 実行状態.合格)
        self.assertIn(結果.値, {"ProteinX supports catalysis.", "ProteinX transport hypothesis."})
        self.assertEqual({r.識別子 for r in 結果.参照}, {"doc:catalysis", "doc:transport"})
        self.assertGreater(len(provider.queries), 1)


if __name__ == "__main__":
    unittest.main()

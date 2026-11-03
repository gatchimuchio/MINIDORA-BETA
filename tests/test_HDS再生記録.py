from __future__ import annotations

import json
from pathlib import Path
import tempfile
import unittest

from minidora.HDS中間表現 import HDSIR, HDS実行核, HDS座標
from minidora.HDS再生記録 import HDS選択肢再生収録, 再生JSONL保存, 再生入力問題
from minidora.参照 import 参照記録


class _構文化器:
    def __init__(self) -> None:
        self.calls: list[object] = []

    def 問題IR(self, question: str, choices: tuple[str, ...]) -> HDSIR:
        self.calls.append(("問題IR", question, choices))
        return HDSIR(
            原文=question,
            正規化文=question,
            認知世界ID='記録-test',
            座標=(
                HDS座標('主体', "対象.実体", "Alpha"),
                HDS座標('選択肢:A', "目的.候補", choices[0]),
                HDS座標('選択肢:B', "目的.候補", choices[1]),
            ),
            関係=(),
            残差=(),
            意味作用履歴=(),
            実行核=HDS実行核("意味構造転送"),
            参照必須=True,
            種別="meaning",
            閉包状態='CLOSED_FOR_意味_TRANSFER',
        )

    def コンパイル(self, 入力: str, **kwargs) -> HDSIR:
        self.calls.append(入力)
        coords = ()
        if 入力 in {"engine", "stone"}:
            coords = (HDS座標('候補', "対象.実体", 入力),)
        elif 入力 == "external evidence":
            coords = (HDS座標('証拠', "対象.実体", "engine"),)
        return HDSIR(
            原文=入力,
            正規化文=入力,
            認知世界ID='記録-test',
            座標=coords,
            関係=(),
            残差=(),
            意味作用履歴=(),
            実行核=HDS実行核("意味構造転送"),
            参照必須=False,
            種別="meaning",
            閉包状態='CLOSED_FOR_意味_TRANSFER',
        )


class _Provider:
    名称 = "fixture"

    def 検索(self, query: str, limit: int = 8):
        return (
            参照記録(
                "doc:1",
                "Alpha",
                "external evidence",
                "fixture://doc1",
                self.名称,
                信頼=0.8,
                条件=(
                    ('hds_query_選択肢', "A"),
                    ("hds_query_kind", '選択肢'),
                ),
            ),
        )


class _Legacy構文化器:
    def コンパイル(self, 入力: str, **kwargs) -> HDSIR:
        return HDSIR(
            原文=入力,
            正規化文=入力,
            認知世界ID='legacy-記録-test',
            座標=(),
            関係=(),
            残差=(),
            意味作用履歴=(),
            実行核=HDS実行核("意味構造転送"),
            種別="meaning",
            閉包状態='CLOSED_FOR_意味_TRANSFER',
        )


class HDS再生収録試験(unittest.TestCase):
    def test_goldを構文化器検索へ渡さず通常選択肢問題IRとprovenanceを固定する(self) -> None:
        構文化器 = _構文化器()
        rows, stats = HDS選択肢再生収録(
            (
                再生入力問題(
                    "case:1",
                    "question text",
                    {"A": "engine", "B": "stone"},
                    gold="A",
                ),
            ),
            構文化器=構文化器,
            provider=_Provider(),
        )

        self.assertEqual(len(rows), 1)
        row = rows[0]
        self.assertEqual(row["gold"], "A")
        self.assertIn(("問題IR", "question text", ("engine", "stone")), 構文化器.calls)
        self.assertNotIn("A", [call for call in 構文化器.calls if isinstance(call, str) and call not in {"engine", "stone"}])
        self.assertEqual(stats.問題数, 1)
        self.assertEqual(stats.選択肢コンパイル数, 2)
        self.assertEqual(stats.資料件数, 1)
        self.assertEqual(stats.資料コンパイル数, 1)
        self.assertEqual(row['資料'][0]['情報源_信頼度'], 0.8)
        self.assertNotIn("content", row['資料'][0])
        self.assertEqual(
            row['資料'][0]["provenance"],
            [
                "fixture",
                "fixture://doc1",
                "doc:1",
                'query_選択肢:A',
                'query_kind:選択肢',
                "query_kind:代替経路",
                "query_kind:代替経路_選択肢",
                'query_選択肢:B',
            ],
        )

    def test_問題IRを持たないlegacy構文化器は通常コンパイルへ代替経路する(self) -> None:
        rows, _stats = HDS選択肢再生収録(
            (再生入力問題("legacy", "plain question", {"A": "one", "B": "two"}),),
            構文化器=_Legacy構文化器(),
            provider=None,
        )
        self.assertEqual(rows[0]["question_ir"]["原文"], "plain question")

    def test_JSONL保存は1case1行でUTF8保存する(self) -> None:
        rows = ({"契約形式": 'minidora.hds-選択肢-再生.v1', "id": "日本語"},)
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "bundle.jsonl"
            再生JSONL保存(rows, path)
            lines = path.read_text(encoding="utf-8").splitlines()
        self.assertEqual(len(lines), 1)
        self.assertEqual(json.loads(lines[0])["id"], "日本語")


if __name__ == "__main__":
    unittest.main()

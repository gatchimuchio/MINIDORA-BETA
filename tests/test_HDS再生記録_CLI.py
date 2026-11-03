from __future__ import annotations

import json
from pathlib import Path
import subprocess
import sys
import tempfile
import textwrap
import unittest


ROOT = Path(__file__).resolve().parents[1]


class HDS再生収録CLI試験(unittest.TestCase):
    def test_private_pluginからbundleだけ生成できる(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            plugin = root / "private_plugin.py"
            plugin.write_text(
                textwrap.dedent(
                    '\n                    from minidora import HDSIR, HDS実行核, HDS座標, HDS関係, 参照記録\n\n                    class 構文化器:\n                        def コンパイル(self, 入力, **kwargs):\n                            if 入力 == "Question?":\n                                coords = (\n                                    HDS座標("主体", "対象.実体", "Alpha"),\n                                    HDS座標("選択肢:A", "目的.候補", "engine"),\n                                    HDS座標("選択肢:B", "目的.候補", "stone"),\n                                )\n                                relations = ()\n                            elif 入力 in {"engine", "stone"}:\n                                coords = (HDS座標("候補", "対象.実体", 入力),)\n                                relations = ()\n                            else:\n                                coords = (\n                                    HDS座標("alpha", "対象.実体", "Alpha"),\n                                    HDS座標("engine", "対象.実体", "engine"),\n                                )\n                                relations = (HDS関係("r", ("alpha",), ("engine",), "作用"),)\n                            return HDSIR(\n                                原文=入力,\n                                正規化文=入力,\n                                認知世界ID="private-plugin-test",\n                                座標=coords,\n                                関係=relations,\n                                残差=(),\n                                意味作用履歴=(),\n                                実行核=HDS実行核("意味構造転送"),\n                                参照必須=(入力 == "Question?"),\n                                種別="meaning",\n                                閉包状態="CLOSED_FOR_意味_TRANSFER",\n                            )\n\n                    class Provider:\n                        名称 = "private-fixture"\n                        def 検索(self, query, limit=8):\n                            return (参照記録("doc:1", "Alpha", "Alpha uses engine.", "private://doc1", self.名称, 信頼=0.7),)\n\n                    def make_構文化器():\n                        return 構文化器()\n\n                    def make_provider():\n                        return Provider()\n                    '
                ),
                encoding="utf-8",
            )
            dataset = root / "dataset.jsonl"
            dataset.write_text(
                json.dumps(
                    {
                        "id": "case:1",
                        "question": "Question?",
                        "choices": {"A": "engine", "B": "stone"},
                        "gold": "A",
                    },
                    ensure_ascii=False,
                ) + "\n",
                encoding="utf-8",
            )
            bundle = root / "bundle.jsonl"
            stats = root / "stats.json"
            completed = subprocess.run(
                [
                    sys.executable,
                    str(ROOT / "tools" / 'HDS再生記録.py'),
                    str(dataset),
                    str(bundle),
                    "--plugin-path",
                    str(root),
                    "--compiler",
                    'private_plugin:make_構文化器',
                    "--provider",
                    "private_plugin:make_provider",
                    "--stats",
                    str(stats),
                ],
                cwd=ROOT,
                text=True,
                encoding="utf-8",
                capture_output=True,
                check=False,
            )

            self.assertEqual(completed.returncode, 0, completed.stderr)
            row = json.loads(bundle.read_text(encoding="utf-8").splitlines()[0])
            要約 = json.loads(stats.read_text(encoding="utf-8"))

        self.assertEqual(row["gold"], "A")
        self.assertEqual(row['資料'][0]['情報源_信頼度'], 0.7)
        self.assertNotIn('構文化器', json.dumps(row, ensure_ascii=False))
        self.assertEqual(要約["problem_count"], 1)
        self.assertEqual(要約['選択肢_compile_count'], 2)
        self.assertEqual(要約['資料_compile_count'], 1)


if __name__ == "__main__":
    unittest.main()

from __future__ import annotations

import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from minidora.HDS中間表現 import HDSIR, HDS実行核, HDS座標, HDS関係, 値状態
from minidora.HDS再生 import HDSIR復元, HDSIR辞書化


ROOT = Path(__file__).resolve().parents[1]


def _ir(text: str, coords: tuple[HDS座標, ...], relations: tuple[HDS関係, ...] = ()) -> HDSIR:
    return HDSIR(
        原文=text,
        正規化文=text,
        認知世界ID='再生:test',
        座標=coords,
        関係=relations,
        残差=(),
        意味作用履歴=(),
        実行核=HDS実行核("意味構造転送"),
        種別="意味構造",
        閉包状態='CLOSED_FOR_意味_TRANSFER',
        入力言語="en",
    )


def _選択肢行(*, case_id: str) -> dict:
    question = _ir(
        "What does Alpha use?",
        (
            HDS座標("alpha", "対象.実体", "Alpha", 原文範囲=(10, 15)),
            HDS座標('選択肢:A', "目的.候補", "engine"),
            HDS座標('選択肢:B', "目的.候補", "stone"),
        ),
    )
    choices = {
        "A": _ir("engine", (HDS座標("a", "対象.実体", "engine", 原文範囲=(0, 6)),)),
        "B": _ir("stone", (HDS座標("b", "対象.実体", "stone", 原文範囲=(0, 5)),)),
    }
    資料 = _ir(
        "Alpha uses engine.",
        (
            HDS座標("alpha", "対象.実体", "Alpha", 原文範囲=(0, 5)),
            HDS座標("engine", "対象.実体", "engine", 原文範囲=(11, 17)),
        ),
        (HDS関係("use", ("alpha",), ("engine",), "作用"),),
    )
    return {
        "契約形式": 'minidora.hds-選択肢-再生.v1',
        "id": case_id,
        "question_ir": HDSIR辞書化(question),
        "choices_ir": {label: HDSIR辞書化(ir) for label, ir in choices.items()},
        '資料': [{"provenance": ["fixture", "doc:1"], "ir": HDSIR辞書化(資料)}],
        "gold": "A",
    }


class HDS再生試験(unittest.TestCase):
    def test_HDSIRをJSON形へ往復できる(self) -> None:
        original = _ir(
            "Alpha uses engine.",
            (
                HDS座標("alpha", "対象.実体", "Alpha", 値状態=値状態.確定, 原文範囲=(0, 5)),
                HDS座標("engine", "対象.実体", "engine", 値状態=値状態.推定),
            ),
            (HDS関係("r", ("alpha",), ("engine",), "作用"),),
        )
        restored = HDSIR復元(HDSIR辞書化(original))
        self.assertEqual(restored.原文, original.原文)
        self.assertEqual(restored.座標[1].値状態, 値状態.推定)
        self.assertEqual(restored.関係[0].始点, ("alpha",))
        self.assertEqual(restored.関係[0].終点, ("engine",))
        self.assertIsNone(restored.手順)

    def test_固定HDS_IRだけで選択肢_外部評価を再実行できる(self) -> None:
        row = _選択肢行(case_id="fixture:1")

        with tempfile.TemporaryDirectory() as tmp:
            input_path = Path(tmp) / "fixture.jsonl"
            input_path.write_text(json.dumps(row, ensure_ascii=False) + "\n", encoding="utf-8")
            completed = subprocess.run(
                [sys.executable, str(ROOT / "tools" / "HDS選択再生評価.py"), str(input_path)],
                cwd=ROOT,
                text=True,
                encoding="utf-8",
                capture_output=True,
                check=False,
            )
        self.assertEqual(completed.returncode, 0, completed.stderr)
        結果 = json.loads(completed.stdout)
        self.assertEqual(結果["total"], 1)
        self.assertEqual(結果["correct"], 1)
        self.assertEqual(結果["answered"], 1)
        self.assertEqual(結果["suspended"], 0)
        self.assertEqual(結果["details"][0]["predicted"], "A")

    def test_GPQA識別子の固定再生は汎用runnerでも拒否する(self) -> None:
        row = _選択肢行(case_id="gpqa:000")
        with tempfile.TemporaryDirectory() as tmp:
            input_path = Path(tmp) / "gpqa.jsonl"
            input_path.write_text(json.dumps(row, ensure_ascii=False) + "\n", encoding="utf-8")
            completed = subprocess.run(
                [sys.executable, str(ROOT / "tools" / "HDS選択再生評価.py"), str(input_path)],
                cwd=ROOT,
                text=True,
                encoding="utf-8",
                capture_output=True,
                check=False,
            )
        self.assertNotEqual(completed.returncode, 0)
        self.assertIn('GPQA_FIXED_参照_FORBIDDEN', completed.stderr)


if __name__ == "__main__":
    unittest.main()

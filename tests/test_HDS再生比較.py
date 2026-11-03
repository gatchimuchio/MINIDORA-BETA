from __future__ import annotations

import importlib.util
from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location(
    'HDS再生_比較_tool',
    ROOT / "tools" / 'HDS再生比較.py',
)
assert SPEC and SPEC.loader
モジュール = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(モジュール)
比較 = モジュール.比較


def _結果(details, correct, answered):
    return {
        "total": len(details),
        "correct": correct,
        "accuracy_percent": 100.0 * correct / len(details),
        "answered": answered,
        "answer_rate_percent": 100.0 * answered / len(details),
        "details": details,
    }


class HDS再生比較試験(unittest.TestCase):
    def test_SUSPENDから正答と誤答を別分類する(self) -> None:
        before = _結果(
            [
                {"id": "a", "status": "SUSPEND", "predicted": None, "correct": False, "reasons": ['NO_KNOWLEDGE_証拠']},
                {"id": "b", "status": "SUSPEND", "predicted": None, "correct": False, "reasons": ['NO_KNOWLEDGE_証拠']},
            ],
            0,
            0,
        )
        after = _結果(
            [
                {"id": "a", "status": "APPROVE", "predicted": "A", "correct": True, "reasons": ['証拠_PRESENT']},
                {"id": "b", "status": "APPROVE", "predicted": "B", "correct": False, "reasons": ['証拠_PRESENT']},
            ],
            1,
            2,
        )
        結果 = 比較(before, after)
        self.assertEqual(結果["transitions"]["SUSPEND_TO_CORRECT"], 1)
        self.assertEqual(結果["transitions"]["SUSPEND_TO_WRONG"], 1)
        self.assertEqual(結果["delta"]["correct"], 1)
        self.assertEqual(結果["delta"]["answered"], 2)
        self.assertEqual(結果["reason_delta"]["-NO_KNOWLEDGE_証拠"], 2)
        self.assertEqual(結果["reason_delta"]["+証拠_PRESENT"], 2)

    def test_正答退行を明示分類する(self) -> None:
        before = _結果(
            [{"id": "a", "status": "APPROVE", "predicted": "A", "correct": True, "reasons": []}],
            1,
            1,
        )
        after = _結果(
            [{"id": "a", "status": "APPROVE", "predicted": "B", "correct": False, "reasons": []}],
            0,
            1,
        )
        結果 = 比較(before, after)
        self.assertEqual(結果["transitions"]["CORRECT_TO_WRONG"], 1)
        self.assertEqual(結果["delta"]["correct"], -1)


if __name__ == "__main__":
    unittest.main()

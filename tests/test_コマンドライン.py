import json
import os
from pathlib import Path
import subprocess
import sys
import unittest


ROOT = Path(__file__).resolve().parents[1]


def _env() -> dict[str, str]:
    env = dict(os.environ)
    src = str(ROOT / "src")
    env["PYTHONPATH"] = src if not env.get("PYTHONPATH") else src + os.pathsep + env["PYTHONPATH"]
    env["PYTHONIOENCODING"] = "cp1252"
    return env


class CLI試験(unittest.TestCase):
    def test_python_m_minidoraで即時実行できる(self):
        completed = subprocess.run(
            [sys.executable, "-m", "minidora", "2+3"],
            cwd=ROOT,
            env=_env(),
            text=True,
            encoding="utf-8",
            capture_output=True,
            check=False,
        )
        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertEqual(completed.stdout.strip(), "5です。")

    def test_json出力は公開HDS_構文化器経路を機械可読で返す(self):
        completed = subprocess.run(
            [sys.executable, "-m", "minidora", "--json", "2+3"],
            cwd=ROOT,
            env=_env(),
            text=True,
            encoding="utf-8",
            capture_output=True,
            check=False,
        )
        self.assertEqual(completed.returncode, 0, completed.stderr)
        payload = json.loads(completed.stdout)
        self.assertEqual(payload["value"], 5)
        self.assertEqual(payload["status"], "合格")
        self.assertEqual(payload["plan"], "算術")
        self.assertTrue(payload["HDS中間表現"])
        self.assertEqual(payload['構文化器'], "公開HDSコンパイラ")

    def test_非UTF8ロケールでも日本語標準入力を処理できる(self):
        completed = subprocess.run(
            [sys.executable, "-m", "minidora"],
            cwd=ROOT,
            env=_env(),
            input="2と3を足して\n",
            text=True,
            encoding="utf-8",
            capture_output=True,
            check=False,
        )
        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertIn("5です。", completed.stdout)


if __name__ == "__main__":
    unittest.main()

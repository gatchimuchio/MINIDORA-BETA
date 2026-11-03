"""v2の利用入口と、保存された資産を信頼根にしない境界。"""
from copy import deepcopy
import json
import os
from pathlib import Path
import subprocess
import sys
from tempfile import TemporaryDirectory
import unittest
from minidora.HDS運用 import HDS運用セッション
from minidora.HDS運用.製品 import HDS製品ミニドラ
from minidora.HDS運用.値 import 封緘, 指紋


class 入口試験(unittest.TestCase):
    def CLI(self, *引数):
        根 = Path(__file__).resolve().parents[1]
        return subprocess.run([sys.executable, "-m", "minidora.HDS運用", *引数], cwd=根,
            env={**os.environ, "PYTHONPATH": str(根 / "src"), "PYTHONUTF8": "1"},
            capture_output=True, text=True, encoding="utf-8", timeout=60)

    def test_CLIの知識ファイル登録と導出と保存復元(self):
        with TemporaryDirectory() as 場所:
            場所 = Path(場所)
            (場所 / "規則.txt").write_text("すべての鳥は動物です。", encoding="utf-8")
            (場所 / "観測.txt").write_text("花子は鳥です。", encoding="utf-8")
            結果 = self.CLI("--知識", f"規則={場所 / '規則.txt'}", "--知識", f"観測={場所 / '観測.txt'}",
                              "--形成なし", "--保存", str(場所 / "状態.json"), "--json",
                              "知識から「花子は動物です」を確認して")
            self.assertEqual(結果.returncode, 0, 結果.stderr)
            self.assertIn("「支持」", json.loads(結果.stdout)["本文"])
            後 = self.CLI("--復元", str(場所 / "状態.json"), "--json", "資料「規則」の原文を再参照して")
            self.assertEqual(後.returncode, 0, 後.stderr)
            self.assertIn("すべての鳥", json.loads(後.stdout)["本文"])

    def test_CLIの形成無効化を保存する(self):
        with TemporaryDirectory() as 場所:
            保存 = Path(場所) / "state.json"
            結果 = self.CLI("--形成なし", "--保存", str(保存), "--json", "2+3")
            self.assertEqual(結果.returncode, 0, 結果.stderr)
            内容 = json.loads(保存.read_text(encoding="utf-8"))["内容"]
            self.assertFalse(内容["手順形成"])
            self.assertEqual(内容["形成手順"], {})

    def test_サーバにCLI知識ファイルを混ぜない(self):
        結果 = self.CLI("--serve", "--知識", "A=存在しない.txt")
        self.assertEqual(結果.returncode, 2)
        self.assertIn("併用できません", 結果.stderr)

    def test_製品の同じ入口から知識と記憶を使う(self):
        製品 = HDS製品ミニドラ(手順形成=False)
        for 入力 in ("知識「規則」を登録:すべての猫は哺乳類です。",
                    "知識「観測」を登録:太郎は猫です。", "知識から「太郎は哺乳類です」を確認して"):
            結果 = 製品.応答(入力, セッションID="知識")
            self.assertEqual(結果.状態, "APPROVE", 結果.本文)
            self.assertTrue(製品.監査台帳.検証(結果.追跡ID))
        self.assertIn("「支持」", 結果.本文)
        self.assertEqual(製品.応答("知識から「太郎は哺乳類です」を確認して", セッションID="別").状態, "SUSPEND")

    def test_配列に文字列を暗黙変換しない(self):
        会話 = HDS運用セッション()
        with self.assertRaises(TypeError):
            会話.文脈を取得(必須資料="資料A")
        with self.assertRaises(TypeError):
            会話.文脈を取得(検索語="検索語")

    def test_v1の保存形式を無言移行しない(self):
        会話 = HDS運用セッション()
        保存 = json.loads(会話.保存())["内容"]
        保存["版"] = "HDS-MINIDORA-全体運用-v1"
        with self.assertRaises(ValueError):
            HDS運用セッション.復元(json.dumps(封緘(保存), ensure_ascii=False))

    def test_壊れた手順の型は復元時点で拒否(self):
        会話 = HDS運用セッション()
        self.assertTrue(会話.応答("2+3").成立)
        原型 = json.loads(会話.保存())["内容"]
        for 値 in ([], None, "固定素材"):
            with self.subTest(値=値):
                保存 = deepcopy(原型)
                手順 = next(iter(保存["形成手順"].values()))
                手順["固定素材"] = 値
                手順["SHA256"] = 指紋({k: v for k, v in 手順.items() if k != "SHA256"})
                with self.assertRaises(ValueError):
                    HDS運用セッション.復元(json.dumps(封緘(保存), ensure_ascii=False))

    def test_外部作用へ書換えた手順は再ハッシュしても拒否(self):
        会話 = HDS運用セッション()
        self.assertTrue(会話.応答("2+3").成立)
        保存 = json.loads(会話.保存())["内容"]
        手順 = next(iter(保存["形成手順"].values()))
        手順["雛型"]["外部許可"] = True
        手順["SHA256"] = 指紋({k: v for k, v in 手順.items() if k != "SHA256"})
        with self.assertRaises(ValueError):
            HDS運用セッション.復元(json.dumps(封緘(保存), ensure_ascii=False))

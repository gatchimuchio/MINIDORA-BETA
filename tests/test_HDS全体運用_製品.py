"""実API・監査台帳・同期CLIとHDS通常運用の接続を検査する。"""
from http.client import HTTPConnection
from http.server import ThreadingHTTPServer
import json
import os
from pathlib import Path
import subprocess
import sys
from tempfile import TemporaryDirectory
from threading import Thread
import unittest
from unittest.mock import patch
from minidora.HDS運用.製品 import HDS製品ミニドラ
from minidora.製品版.api import APIHandler


class 製品接続試験(unittest.TestCase):
    def test_実製品応答にHDS履歴と検証可能な監査を返す(self):
        製品 = HDS製品ミニドラ()
        結果 = 製品.応答("2+3")
        self.assertEqual(結果.状態,"APPROVE")
        self.assertEqual(結果.経路,"HDS通常運用")
        self.assertTrue(製品.監査台帳.検証(結果.追跡ID))

    def test_異なるAPI会話を分離する(self):
        製品 = HDS製品ミニドラ()
        self.assertEqual(製品.応答("2+3",セッションID="A").状態,"APPROVE")
        self.assertEqual(製品.応答("詳しく説明して",セッションID="B").状態,"SUSPEND")

    def test_製品のセッション上限を守る(self):
        製品 = HDS製品ミニドラ(最大セッション数=1)
        製品.セッション("A")
        with self.assertRaises(ValueError):
            製品.セッション("B")

    def test_既存HTTP入口と同一生成元制限を利用する(self):
        with patch.dict(os.environ,{"MINIDORA_HTTP_LOG":"0"}):
            サーバ = ThreadingHTTPServer(("127.0.0.1",0),APIHandler)
            サーバ.app = HDS製品ミニドラ()
            サーバ.同一生成元限定 = True
            糸 = Thread(target=サーバ.serve_forever,daemon=True); 糸.start()
            接続 = HTTPConnection("127.0.0.1",サーバ.server_port,timeout=20)
            try:
                接続.request("POST","/api/chat",json.dumps({"message":"2+3","session_id":"A"}),{"Content-Type":"application/json"})
                応答 = 接続.getresponse(); self.assertEqual(応答.status,200)
                結果 = json.loads(応答.read())
                self.assertEqual(結果["status"],"APPROVE")
                接続.request("GET","/api/trace/"+結果["追跡_id"])
                応答 = 接続.getresponse(); self.assertTrue(json.loads(応答.read())["valid"])
                接続.request("POST","/api/chat",json.dumps({"message":"2+3"}),
                    {"Content-Type":"application/json","Origin":"https://example.test"})
                応答 = 接続.getresponse(); self.assertEqual(応答.status,403); 応答.read()
            finally:
                接続.close(); サーバ.shutdown(); サーバ.server_close(); 糸.join(5)

    def CLI(self,*引数):
        根 = Path(__file__).resolve().parents[1]
        return subprocess.run([sys.executable,"-m",*引数],cwd=根,
            env={**os.environ,"PYTHONPATH":str(根/"src"),"PYTHONUTF8":"1"},
            capture_output=True,text=True,encoding="utf-8",timeout=30)

    def test_既存製品の明示HDS入口(self):
        結果 = self.CLI("minidora.製品版","--HDS","2+3")
        self.assertEqual(結果.returncode,0,結果.stderr)
        self.assertEqual(結果.stdout.split("trace_id=")[0].strip(),"処理結果：\n5")

    def test_HDS専用JSON入口と保存復元(self):
        with TemporaryDirectory() as 場所:
            保存 = str(Path(場所)/"会話.json")
            結果 = self.CLI("minidora.HDS運用","--保存",保存,"--json","3*7")
            self.assertEqual(結果.returncode,0,結果.stderr)
            self.assertEqual(json.loads(結果.stdout)["状態"],"COMMIT")
            結果 = self.CLI("minidora.HDS運用","--復元",保存,"詳しく説明して")
            self.assertEqual(結果.returncode,0,結果.stderr)
            self.assertIn("21",結果.stdout)

    def test_旧製品の汎用フラグとHDSを混在させない(self):
        結果 = self.CLI("minidora.製品版","--HDS","--汎用","2+3")
        self.assertNotEqual(結果.returncode,0)

    def test_HDS製品の未完了は終了コードでも成功としない(self):
        結果 = self.CLI("minidora.製品版","--HDS","未対応の能力を発明して")
        self.assertEqual(結果.returncode,2)
        self.assertIn("未確定",結果.stdout)

"""Cloud Run用HTTP境界と既存ローカル限定入口を実通信で検査する。"""
from collections import deque
import base64
import hashlib
from http.client import HTTPConnection, HTTPResponse
from http.server import ThreadingHTTPServer
import json
import os
import socket
import secrets
from threading import Lock, Thread
from time import monotonic
import unittest
from unittest.mock import patch

from minidora.HDS運用.製品 import HDS製品ミニドラ
from minidora.製品版.api import APIHandler, CloudRun許可値, HTTPBasic許可値


class CloudRun入口試験(unittest.TestCase):
    def setUp(self):
        self.環境 = patch.dict(os.environ, {"MINIDORA_HTTP_LOG": "0"})
        self.環境.start()
        self.サーバ = ThreadingHTTPServer(("127.0.0.1", 0), APIHandler)
        self.サーバ.app = HDS製品ミニドラ()
        self.サーバ.CloudRun境界 = True
        パスワード = secrets.token_urlsafe(32)
        self.認証 = "Basic " + base64.b64encode(("judge:" + パスワード).encode()).decode()
        self.サーバ.HTTPBasic許可値 = HTTPBasic許可値({"MINIDORA_HTTP_BASIC_USERNAME": "judge",
            "MINIDORA_HTTP_BASIC_PASSWORD_SHA256": hashlib.sha256(パスワード.encode()).hexdigest()}, 必須=True)
        self.サーバ.許可ホスト = frozenset({"minidora.example.test"})
        self.サーバ.許可生成元 = frozenset({"https://minidora.example.test"})
        self.サーバ.受付ロック, self.サーバ.受付時刻 = Lock(), deque()
        self.糸 = Thread(target=self.サーバ.serve_forever, kwargs={"poll_interval": 0.01}, daemon=True)
        self.糸.start()

    def tearDown(self):
        self.サーバ.shutdown()
        self.サーバ.server_close()
        self.糸.join(5)
        self.環境.stop()

    def HTTPを呼ぶ(self, 方法="GET", 道="/health", 内容=None, **追加ヘッダ):
        接続 = HTTPConnection("127.0.0.1", self.サーバ.server_port, timeout=60)
        ヘッダ = {"Host": "minidora.example.test", "Authorization": self.認証, **追加ヘッダ}
        if 内容 is not None:
            内容 = json.dumps(内容).encode("utf-8")
            ヘッダ.setdefault("Content-Type", "application/json")
        try:
            接続.request(方法, 道, 内容, ヘッダ)
            応答 = 接続.getresponse()
            return 応答.status, dict(応答.getheaders()), 応答.read()
        finally:
            接続.close()

    def test_実測Hostと同一生成元だけが通る(self):
        self.assertEqual(self.HTTPを呼ぶ()[0], 200)
        self.assertEqual(self.HTTPを呼ぶ(Host="evil.example.test")[0], 403)
        self.assertEqual(self.HTTPを呼ぶ(Origin="https://evil.example.test")[0], 403)
        self.assertEqual(self.HTTPを呼ぶ(Origin="null")[0], 403)
        応答 = self.HTTPを呼ぶ(Origin="https://minidora.example.test")
        self.assertEqual(応答[0], 200)
        self.assertNotIn("Access-Control-Allow-Origin", 応答[1])

    def test_重複Hostを拒否する(self):
        接続 = HTTPConnection("127.0.0.1", self.サーバ.server_port, timeout=60)
        try:
            接続.putrequest("GET", "/health", skip_host=True)
            接続.putheader("Host", "minidora.example.test")
            接続.putheader("Host", "evil.example.test")
            接続.endheaders()
            応答 = 接続.getresponse()
            self.assertEqual(応答.status, 403)
            応答.read()
        finally:
            接続.close()

    def test_不正JSON形と入力上限を拒否する(self):
        for 内容 in ([], {"message": 7}, {"message": "2+3", "session_id": {}},
                    {"message": "2+3", "session_id": "a" * 129}, {"message": "a" * 8193}):
            with self.subTest(内容=内容):
                self.assertEqual(self.HTTPを呼ぶ("POST", "/api/chat", 内容)[0], 400)
        self.assertEqual(self.HTTPを呼ぶ("POST", "/api/chat", {"message": "2+3"},
                                       **{"Content-Type": "text/plain"})[0], 415)

    def test_拒否応答後も未読の大きい本文を安全に破棄する(self):
        本文 = b"x" * 1_048_576
        with patch.object(self.サーバ.app, "応答") as 実行:
            with socket.create_connection(("127.0.0.1", self.サーバ.server_port), timeout=5) as 接続:
                ヘッダ = ("POST /api/chat HTTP/1.1\r\nHost: minidora.example.test\r\n"
                         "Authorization: " + self.認証 + "\r\n"
                         "Content-Type: application/json\r\nContent-Length: "
                         + str(len(本文)) + "\r\n\r\n").encode("ascii")
                接続.sendall(ヘッダ + 本文[:1])
                接続.sendall(本文[1:])
                with HTTPResponse(接続) as 応答:
                    応答.begin()
                    self.assertEqual(応答.status, 413)
                    self.assertEqual(json.loads(応答.read())["error"], "invalid_body_size")
            実行.assert_not_called()

    def test_空TransferEncodingも拒否し製品を実行しない(self):
        with patch.object(self.サーバ.app, "応答") as 実行:
            self.assertEqual(self.HTTPを呼ぶ("POST", "/api/chat", {"message": "2+3"},
                                           **{"Transfer-Encoding": ""})[0], 400)
            実行.assert_not_called()

    def test_応答と監査を既存入口から取得する(self):
        応答 = self.HTTPを呼ぶ("POST", "/api/chat", {"message": "2+3", "session_id": "検査"})
        self.assertEqual(応答[0], 200)
        本文 = json.loads(応答[2])
        self.assertEqual(本文["status"], "APPROVE")
        追跡 = self.HTTPを呼ぶ(道="/api/trace/" + 本文["追跡_id"])
        self.assertTrue(json.loads(追跡[2])["valid"])

    def test_Cloudは旧監査の原値を返さず原記録も変換しない(self):
        旧 = self.サーバ.app.監査台帳.開始("user@example.test", "旧境界").確定("旧原値", "SUSPEND")
        応答 = self.HTTPを呼ぶ(道="/api/trace/" + 旧.追跡ID)
        self.assertEqual(応答[0], 409)
        self.assertNotIn(b"user@example.test", 応答[2])
        self.assertEqual(self.サーバ.app.監査台帳.取得(旧.追跡ID).入力文, "user@example.test")
        self.assertTrue(self.サーバ.app.監査台帳.検証(旧.追跡ID))

    def test_要求頻度を制限して期限後に解除する(self):
        self.サーバ.受付時刻.extend([monotonic()] * 30)
        self.assertEqual(self.HTTPを呼ぶ("POST", "/api/chat", {"message": "2+3"})[0], 429)
        self.サーバ.受付時刻 = deque([monotonic() - 61] * 30)
        self.assertEqual(self.HTTPを呼ぶ("POST", "/api/chat", {"message": "2+3"})[0], 200)

    def test_設定未確定とワイルドカードを起動時に拒否する(self):
        with patch.dict(os.environ, {"MINIDORA_CLOUD_HOSTS": "", "MINIDORA_CLOUD_ORIGINS": ""}):
            with self.assertRaises(ValueError): CloudRun許可値()
        for ホスト, 生成元 in (("*", "https://*"), ("test:443", "https://test:443"),
                            ("test", "http://test"), ("test", "https://test/"),
                            ("test", "https://evil.test")):
            with patch.dict(os.environ, {"MINIDORA_CLOUD_HOSTS": ホスト, "MINIDORA_CLOUD_ORIGINS": 生成元}):
                with self.assertRaises(ValueError): CloudRun許可値()
        with patch.dict(os.environ, {"MINIDORA_CLOUD_HOSTS": "bootstrap.invalid", "MINIDORA_CLOUD_ORIGINS": "https://bootstrap.invalid"}):
            self.assertEqual(CloudRun許可値()[0], frozenset({"bootstrap.invalid"}))

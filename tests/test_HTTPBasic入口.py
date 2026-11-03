"""審査用Basic認証を実HTTPで確認する。Googleへの通信・取得は行わない。"""
import base64
from http.client import HTTPConnection, HTTPResponse
import json
import os
import socket
import unittest
from unittest.mock import patch

from minidora.HDS運用.製品 import HDS製品ミニドラ
from minidora.製品版.ADK接続 import ADK製品ミニドラ
from minidora.製品版.api import HTTPBasic許可値, serve
import test_CloudRun入口 as Cloud入口


class HTTPBasic入口試験(unittest.TestCase):
    setUp = Cloud入口.CloudRun入口試験.setUp
    tearDown = Cloud入口.CloudRun入口試験.tearDown
    HTTPを呼ぶ = Cloud入口.CloudRun入口試験.HTTPを呼ぶ

    def test_全画面とAPIとhealthが未認証ではchallengeを返す(self):
        ケース = [("GET", 道, None) for 道 in ("/", "/static/app.js", "/health", "/api/capabilities",
                    "/api/trace/" + "a" * 32, "/missing")]
        ケース += [("POST", "/api/chat", {"message": "2+3"}), ("OPTIONS", "/api/chat", None),
                  ("HEAD", "/", None), ("PUT", "/api/chat", {}), ("DELETE", "/api/trace/" + "a" * 32, None)]
        with patch.object(self.サーバ.app, "応答") as 応答, patch.object(self.サーバ.app, "能力一覧") as 能力:
            for 方法, 道, 内容 in ケース:
                with self.subTest(方法=方法, 道=道):
                    status, headers, body = self.HTTPを呼ぶ(方法, 道, 内容, Authorization="")
                    self.assertEqual(status, 401)
                    self.assertEqual(headers["WWW-Authenticate"], 'Basic realm="MINIDORA Judge", charset="UTF-8"')
                    self.assertEqual(headers["Cache-Control"], "no-store")
                    self.assertNotIn("Access-Control-Allow-Origin", headers)
            応答.assert_not_called()
            能力.assert_not_called()
        self.assertEqual(len(self.サーバ.受付時刻), 0)

    def test_不正認証と重複ヘッダを拒否する(self):
        for ヘッダ in ("Bearer invalid", "Basic !!!", "Basic " + base64.b64encode(b"judge:wrong").decode(),
                        "Basic " + base64.b64encode(b"judge").decode(), "Basic " + base64.b64encode(b"judge:").decode(),
                        "Basic " + "a" * 1024):
            with self.subTest(形=ヘッダ[:5]):
                self.assertEqual(self.HTTPを呼ぶ(Authorization=ヘッダ)[0], 401)
        接続 = HTTPConnection("127.0.0.1", self.サーバ.server_port, timeout=10)
        try:
            接続.putrequest("GET", "/health", skip_host=True)
            接続.putheader("Host", "minidora.example.test")
            接続.putheader("Authorization", self.認証)
            接続.putheader("Authorization", self.認証)
            接続.endheaders()
            応答 = 接続.getresponse()
            self.assertEqual(応答.status, 401)
            応答.read()
        finally:
            接続.close()

    def test_未認証の大きなPOSTを既存破棄境界で閉じHDSを実行しない(self):
        本文 = b"x" * 1_048_576
        with patch.object(self.サーバ.app, "応答") as 実行:
            with socket.create_connection(("127.0.0.1", self.サーバ.server_port), timeout=5) as 接続:
                ヘッダ = ("POST /api/chat HTTP/1.1\r\nHost: minidora.example.test\r\n"
                         "Content-Type: application/json\r\nContent-Length: " + str(len(本文)) + "\r\n\r\n").encode("ascii")
                接続.sendall(ヘッダ + 本文[:1])
                接続.sendall(本文[1:])
                with HTTPResponse(接続) as 応答:
                    応答.begin()
                    self.assertEqual(応答.status, 401)
                    self.assertEqual(json.loads(応答.read())["error"], "authentication_required")
            実行.assert_not_called()

    def test_正常認証で画面とAPIが通り他Originは拒否する(self):
        for 道 in ("/", "/static/app.js", "/health", "/api/capabilities"):
            with self.subTest(道=道):
                self.assertEqual(self.HTTPを呼ぶ(道=道)[0], 200)
        self.assertEqual(self.HTTPを呼ぶ(Origin="https://evil.example.test")[0], 403)
        self.assertEqual(self.HTTPを呼ぶ(Host="evil.example.test")[0], 403)

    def test_正常認証で既存ADKからHDSを一度だけ駆動し監査を保持する(self):
        hds = HDS製品ミニドラ(基本チャット=True, 公開取得方針="off")
        self.サーバ.app = ADK製品ミニドラ(hds)
        with patch.object(hds, "応答", wraps=hds.応答) as 実駆動:
            status, _, raw = self.HTTPを呼ぶ("POST", "/api/chat",
                {"message": "2+3", "session_id": "judge-auth-local", "external_read": False})
            self.assertEqual(status, 200)
            応答 = json.loads(raw)
            self.assertEqual(応答["status"], "APPROVE")
            self.assertEqual(実駆動.call_count, 1)
            self.assertIn("5", 応答["response"])
            self.assertTrue(応答["metadata"]["ADK"]["通過"])
            self.assertEqual(応答["metadata"]["ADK"]["HDS呼出数"], 1)
        status, _, raw = self.HTTPを呼ぶ(道="/api/trace/" + 応答["追跡_id"])
        self.assertEqual(status, 200)
        self.assertTrue(json.loads(raw)["valid"])

    def test_Cloud設定欠落をlisten前に拒否しローカル無設定を維持する(self):
        self.assertIsNone(HTTPBasic許可値({}))
        不正値 = ({}, {"MINIDORA_HTTP_BASIC_USERNAME": "judge"},
                  {"MINIDORA_HTTP_BASIC_PASSWORD_SHA256": "a" * 64},
                  {"MINIDORA_HTTP_BASIC_USERNAME": "", "MINIDORA_HTTP_BASIC_PASSWORD_SHA256": "a" * 64},
                  {"MINIDORA_HTTP_BASIC_USERNAME": "judge", "MINIDORA_HTTP_BASIC_PASSWORD_SHA256": ""},
                  {"MINIDORA_HTTP_BASIC_USERNAME": "a:b", "MINIDORA_HTTP_BASIC_PASSWORD_SHA256": "a" * 64},
                  {"MINIDORA_HTTP_BASIC_USERNAME": "judge", "MINIDORA_HTTP_BASIC_PASSWORD_SHA256": "A" * 64})
        for 設定 in 不正値:
            with self.subTest(鍵=list(設定)):
                with self.assertRaises(ValueError): HTTPBasic許可値(設定, 必須=True)
        設定 = {"MINIDORA_CLOUD_HOSTS": "minidora.example.test", "MINIDORA_CLOUD_ORIGINS": "https://minidora.example.test"}
        with patch.dict(os.environ, 設定, clear=True), patch("minidora.製品版.api.ThreadingHTTPServer") as listen:
            with self.assertRaises(ValueError): serve(self.サーバ.app, CloudRun境界=True)
            listen.assert_not_called()
        self.サーバ.CloudRun境界 = False
        self.サーバ.同一生成元限定 = True
        self.サーバ.HTTPBasic許可値 = None
        self.assertEqual(self.HTTPを呼ぶ(Host=f"127.0.0.1:{self.サーバ.server_port}", Authorization="")[0], 200)
        self.assertEqual(self.HTTPを呼ぶ(Host="evil.example.test", Authorization="")[0], 403)


if __name__ == "__main__":
    unittest.main()

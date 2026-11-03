"""固定SDKとローカルTCPでADC境界を検査。Google実機の受入ではない。"""
from contextlib import contextmanager
from http.client import HTTPConnection, HTTPSConnection
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from importlib.metadata import PackageNotFoundError, version
import json
from pathlib import Path
import socket
from tempfile import TemporaryDirectory
from threading import Thread
import unittest
from unittest.mock import MagicMock, patch

from minidora.取得実行制御 import 公開取得上限, 共有取得制御, 取得資源不足
from minidora.製品版.Google検索 import GoogleCloud検索供給器
from Google検索予算fixture import 局所送信予算

try:
    SDK導入済み = version("google-auth") == "2.61.0"
except PackageNotFoundError:
    SDK導入済み = False
if SDK導入済み:
    from minidora.製品版.Google認証 import GoogleADC認証器, Google認証失敗, _当回認証Request


_PROJECT = "minidora-507702"
_SA = "minidora-local-test@minidora-507702.iam.gserviceaccount.com"
_トークン = "LOCAL_ADC_TEST_TOKEN"
_URL = ("https://discoveryengine.googleapis.com/v1/projects/" + _PROJECT
        + "/locations/global/collections/default_collection/engines/local-test"
        + "/servingConfigs/default_search:search")


class _サーバ(BaseHTTPRequestHandler):
    def _処理(self):
        body = self.rfile.read(int(self.headers.get("Content-Length", "0")))
        self.server.calls.append((self.command, self.path, dict(self.headers), body))
        headers = {}
        if self.path == "/token":
            応答値 = {"access_token": _トークン, "expires_in": 3600, "token_type": "Bearer"}
            if self.server.mode == "oauthinvalid":
                応答値 = {"access_token": "MALFORMED_PRIVATE_RESPONSE", "expires_in": True}
        elif "/computeMetadata/" in self.path:
            headers["Metadata-Flavor"] = "Google" if self.server.mode != "flavor" else "Wrong"
            if "/default/token?" in self.path:
                応答値 = {"access_token": _トークン, "expires_in": 3600, "token_type": "Bearer"}
            else:
                応答値 = {"email": _SA if self.server.mode != "account" else "other@example.test"}
        elif self.path.endswith("/allowedLocations"):
            応答値 = {"locations": ["global"], "encodedLocations": "0x0"}
            if self.server.mode == "rabinvalid":
                応答値 = {"private": "MALFORMED_PRIVATE_RESPONSE"}
        else:
            応答値 = {"results": [{"document": {"derivedStructData": {
                "title": "ローカル検査資料", "link": "https://source.example.test/body"}}}]}
        raw = json.dumps(応答値).encode()
        self.server.rawSize += len(raw)
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(raw)))
        for key, value in headers.items():
            self.send_header(key, value)
        self.end_headers()
        self.wfile.write(raw)

    do_GET = _処理
    do_POST = _処理

    def log_message(self, *_):
        pass


@unittest.skipUnless(SDK導入済み, "cloud追加依存のgoogle-auth固定版が必要")
class Google認証契約試験(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.server = ThreadingHTTPServer(("127.0.0.1", 0), _サーバ)
        cls.thread = Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()
        cls.server.server_close()
        cls.thread.join(timeout=2)

    def setUp(self):
        self.server.calls, self.server.rawSize, self.server.mode = [], 0, "ok"

    def shared(self, **values):
        return 共有取得制御(公開取得上限(最小通信間隔秒=0, **values))

    @contextmanager
    def transport(self):
        connect = socket.create_connection
        http_request = HTTPConnection.request
        ctx = MagicMock()
        ctx.wrap_socket.side_effect = lambda sock, server_hostname: sock
        addresses = []
        def local_connection(address, timeout):
            addresses.append(address)
            self.assertIn(address, (("8.8.8.8", 443), ("169.254.169.254", 80)))
            return connect(self.server.server_address, timeout=timeout)
        def request(conn, method, target, body=None, headers=None, *, encode_chunked=False):
            return http_request(conn, method, target, body=body,
                headers={**(headers or {}), "Host": "127.0.0.1"}, encode_chunked=encode_chunked)
        with patch("minidora.公開本文取得._公開アドレス", return_value="8.8.8.8"), \
             patch("minidora.公開本文取得.socket.create_connection", side_effect=local_connection), \
             patch("minidora.公開本文取得.ssl.create_default_context", return_value=ctx), \
             patch.object(HTTPSConnection, "request", request), \
             patch.object(HTTPConnection, "request", request):
            yield addresses, ctx

    @contextmanager
    def local_user(self):
        with TemporaryDirectory() as directory:
            path = Path(directory) / "adc.json"
            path.write_text(json.dumps({"type": "authorized_user", "client_id": "LOCAL_CLIENT",
                "client_secret": "LOCAL_SECRET", "refresh_token": "LOCAL_REFRESH",
                "quota_project_id": _PROJECT, "token_uri": "https://evil.example.test/token",
                "token": "DO_NOT_USE_SAVED_TOKEN"}), encoding="utf-8")
            yield GoogleADC認証器.ローカル利用者ファイル(path, _PROJECT)

    def test_利用者ADC更新から検索まで共有し秘密を来歴へ入れない(self):
        shared = self.shared()
        with self.local_user() as auth, self.transport() as (addresses, TLS):
            provider = GoogleCloud検索供給器(_PROJECT, "local-test", auth,
                送信予算=局所送信予算(auth, "local-test"),
                許可ホスト=("source.example.test",)).制御を束縛(shared)
            refs, 来歴 = provider.検索来歴付き("試験資料")
        self.assertEqual(len(refs), 1)
        self.assertEqual([x[:2] for x in self.server.calls], [("POST", "/token"),
            ("POST", "/v1/projects/minidora-507702/locations/global/collections/default_collection/engines/local-test/servingConfigs/default_search:search")])
        self.assertEqual(shared.記録()["使用"]["HTTP開始数"], 2)
        self.assertEqual(shared.記録()["使用"]["総バイト数"], self.server.rawSize)
        self.assertEqual(self.server.calls[1][2]["authorization"], "Bearer " + _トークン)
        self.assertEqual([c.kwargs["server_hostname"] for c in TLS.wrap_socket.call_args_list],
                         ["oauth2.googleapis.com", "discoveryengine.googleapis.com"])
        serialized = json.dumps(来歴, ensure_ascii=False)
        for secret in (_トークン, "LOCAL_SECRET", "LOCAL_REFRESH", "DO_NOT_USE_SAVED_TOKEN"):
            self.assertNotIn(secret, serialized)

    def test_実行SAのmetadataと境界照会を当回で完了する(self):
        shared = self.shared()
        auth = GoogleADC認証器.CloudRun(_PROJECT, _SA)
        with patch("google.auth._regional_access_boundary_utils._RegionalAccessBoundaryRefreshManager.start_refresh",
                   side_effect=AssertionError("背景処理禁止")), self.transport() as (addresses, _):
            headers = auth.認証する(_URL, 取得制御=shared)
            self.assertEqual(len(self.server.calls), 3)
            second = self.shared()
            cached = auth.認証する(_URL, 取得制御=second)
        self.assertEqual(addresses, [("169.254.169.254", 80),
                                    ("169.254.169.254", 80), ("8.8.8.8", 443)])
        self.assertEqual(headers["authorization"], "Bearer " + _トークン)
        self.assertEqual(headers["x-allowed-locations"], "0x0")
        self.assertEqual(cached, headers)
        self.assertEqual(second.記録()["使用"]["HTTP開始数"], 0)
        self.assertEqual(shared.記録()["使用"]["総バイト数"], self.server.rawSize)
        self.assertTrue(self.server.calls[0][1].startswith("/computeMetadata/v1/instance/service-accounts/" + _SA + "/?recursive=true"))
        self.assertEqual(self.server.calls[2][1], "/v1/projects/-/serviceAccounts/" + _SA + "/allowedLocations")

    def test_SDKに捕捉された境界照会の資源不足を再通知し検索しない(self):
        shared = self.shared(最大HTTP開始数=2)
        auth = GoogleADC認証器.CloudRun(_PROJECT, _SA)
        provider = GoogleCloud検索供給器(_PROJECT, "local-test", auth,
            送信予算=局所送信予算(auth, "local-test"),
            許可ホスト=("source.example.test",)).制御を束縛(shared)
        with self.transport(), self.assertRaises(取得資源不足):
            provider.検索来歴付き("試験資料")
        self.assertEqual(len(self.server.calls), 2)
        self.assertTrue(all("/computeMetadata/" in row[1] for row in self.server.calls))

    def test_保存認証は固定objectのGETと世代CAS_POSTだけを扱う(self):
        from minidora.製品版.検索送信予算 import 固定保存先, 検索送信予算停止
        target = 固定保存先("minidora-local-budget", "budget/search.json")
        with self.local_user() as auth:
            scoped = auth.固定保存先(target.バケット, target.オブジェクト)
            with patch("socket.create_connection") as connect:
                for url, method in ((target.メタデータURL(), "POST"),
                        (target.更新URL("100").replace("Match=100", "Match=0"), "POST"),
                        (target.メタデータURL().replace("search.json", "other.json"), "GET"),
                        (target.本文URL("100") + "&generation=100", "GET")):
                    with self.subTest(url=url), self.assertRaises(検索送信予算停止):
                        scoped.認証する(url, method=method, 取得制御=self.shared())
                with self.assertRaises(Google認証失敗):
                    auth.認証する(target.メタデータURL(), 取得制御=self.shared())
                connect.assert_not_called()
            with self.transport():
                first = self.shared()
                header = scoped.認証する(target.メタデータURL(), method="GET", 取得制御=first)
                second = self.shared()
                upload_header = scoped.認証する(target.更新URL("100"), method="POST", 取得制御=second)
            self.assertEqual(header["authorization"], "Bearer " + _トークン)
            self.assertEqual(upload_header, header)
            self.assertEqual(first.記録()["使用"]["HTTP開始数"], 1)
            self.assertEqual(second.記録()["使用"]["HTTP開始数"], 0)
            self.assertEqual([row[:2] for row in self.server.calls], [("POST", "/token")])

    def test_停止と禁止要求ではソケットを開かない(self):
        for url, method, headers, body in (
            ("https://evil.example.test/token", "POST", {}, b"secret"),
            ("http://metadata.google.internal/computeMetadata/v1/project/project-id", "GET", {"metadata-flavor": "Google"}, None),
            ("http://metadata.google.internal:80/computeMetadata/v1/instance/service-accounts/" + _SA + "/?recursive=true", "GET", {"metadata-flavor": "Google"}, None),
            ("https://oauth2.googleapis.com/token", "POST", {"Host": "evil.example.test"}, b"secret")):
            with self.subTest(url=url), patch("socket.create_connection") as connect:
                request = _当回認証Request(self.shared(), _SA)
                with self.assertRaises(Exception):
                    request(url, method=method, headers=headers, body=body)
                with self.assertRaises(Google認証失敗):
                    request.失敗を通知()
                connect.assert_not_called()
        stopped = 共有取得制御(公開取得上限(), 停止要求=lambda: True)
        auth = GoogleADC認証器.CloudRun(_PROJECT, _SA)
        with patch("socket.create_connection") as connect, self.assertRaises(取得資源不足):
            auth.認証する(_URL, 取得制御=stopped)
        connect.assert_not_called()

    def test_metadataの応答識別と実行アカウントを検査する(self):
        for mode in ("flavor", "account"):
            with self.subTest(mode=mode), self.transport():
                self.server.calls, self.server.mode = [], mode
                auth = GoogleADC認証器.CloudRun(_PROJECT, _SA)
                with self.assertRaises(Google認証失敗):
                    auth.認証する(_URL, 取得制御=self.shared())
                self.assertEqual(len(self.server.calls), 1)

    def test_不明ADC型と別プロジェクトとSDK版を自動採用しない(self):
        with TemporaryDirectory() as directory, patch("socket.create_connection") as connect:
            path = Path(directory) / "adc.json"
            for info in ({"type": "service_account"}, {"type": "external_account"},
                         {"type": "authorized_user", "quota_project_id": "other-project"}):
                path.write_text(json.dumps(info), encoding="utf-8")
                with self.assertRaises(Google認証失敗):
                    GoogleADC認証器.ローカル利用者ファイル(path, _PROJECT)
            with patch("google.auth.__version__", "0.0.0"), self.assertRaises(Google認証失敗):
                GoogleADC認証器.CloudRun(_PROJECT, _SA)
            connect.assert_not_called()

    def test_不正な200認証応答をSDKのrawログへ渡さない(self):
        for mode in ("oauthinvalid", "rabinvalid"):
            self.server.calls, self.server.mode = [], mode
            with self.subTest(mode=mode), self.local_user() as local, self.transport(), \
                    patch("google.oauth2._client._LOGGER.error") as error_log:
                auth = local if mode == "oauthinvalid" else GoogleADC認証器.CloudRun(_PROJECT, _SA)
                with self.assertRaises(Google認証失敗) as caught:
                    auth.認証する(_URL, 取得制御=self.shared())
                self.assertNotIn("MALFORMED_PRIVATE_RESPONSE", str(caught.exception))
                error_log.assert_not_called()
                self.assertFalse(any("default_search:search" in row[1] for row in self.server.calls))

    def test_ADC更新の受信枠不足でも検索ソケットを開かない(self):
        shared = self.shared(最大総バイト数=10)
        with self.local_user() as auth, self.transport() as (addresses, TLS):
            provider = GoogleCloud検索供給器(_PROJECT, "local-test", auth,
                送信予算=局所送信予算(auth, "local-test"),
                許可ホスト=("source.example.test",)).制御を束縛(shared)
            with self.assertRaises(取得資源不足):
                provider.検索来歴付き("試験資料")
        self.assertEqual(len(self.server.calls), 1)
        self.assertEqual([c.kwargs["server_hostname"] for c in TLS.wrap_socket.call_args_list],
                         ["oauth2.googleapis.com"])
        self.assertEqual(shared.記録()["使用"]["HTTP開始数"], 1)


if __name__ == "__main__":
    unittest.main()

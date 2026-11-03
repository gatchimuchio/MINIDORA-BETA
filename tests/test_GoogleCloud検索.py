"""認証POSTの局所TCP契約。Google実検索・課金・一般Web品質の受入ではない。"""
from contextlib import contextmanager
from http.client import HTTPSConnection
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import socket
from threading import Thread
import unittest
from unittest.mock import MagicMock, patch

from minidora.取得実行制御 import 公開取得上限, 共有取得制御, 取得資源不足
from minidora.知識取得 import 知識取得要求
from minidora.製品版.Google検索 import GoogleCloud検索供給器, GoogleCloud知識取得器
from Google検索予算fixture import 局所送信予算


class _認証:
    def __init__(self):
        self.calls = []
        self.headers = {"authorization": "Bearer LOCAL_TEST_TOKEN"}

    def 認証する(self, url, *, 取得制御):
        self.calls.append((url, 取得制御))
        return self.headers.copy()


class _検索サーバ(BaseHTTPRequestHandler):
    def do_POST(self):
        body = self.rfile.read(int(self.headers["Content-Length"]))
        self.server.calls.append((self.command, self.path, dict(self.headers), body))
        mode = self.server.mode
        if mode in ("redirect", "rate", "denied"):
            self.send_response({"redirect": 302, "rate": 429, "denied": 401}[mode])
            if mode == "redirect":
                self.send_header("Location", "https://other.test/private")
            if mode == "rate":
                self.send_header("Retry-After", "600")
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        def row(url, title="取得題名"):
            return {"document": {"derivedStructData": {"title": title, "link": url,
                    "snippets": [{"snippet": "生成抜粋を使わない"}]}}}
        rows = [row("https://source.example.test/body")]
        if mode == "filter":
            rows += [row("https://source.example.test/body", "別題名"),
                     row("https://source.example.test.evil.test/body"),
                     row("http://source.example.test/body"),
                     {"document": {"derivedStructData": {"title": 7, "link": "https://source.example.test/a"}}}]
        raw = json.dumps({"results": rows, "summary": {"summaryText": "秘密生成回答"},
                          "nextPageToken": "自動ページ送り禁止"}, ensure_ascii=False).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)

    def do_GET(self):
        self.server.calls.append((self.command, self.path, dict(self.headers), b""))
        raw = "電圧は120 V。".encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/plain; charset=utf-8")
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)

    def log_message(self, *_):
        pass


class Google検索契約試験(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.server = ThreadingHTTPServer(("127.0.0.1", 0), _検索サーバ)
        cls.thread = Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()
        cls.server.server_close()
        cls.thread.join(timeout=2)

    def setUp(self):
        self.server.calls = []
        self.server.mode = "ok"
        self.auth = _認証()
        self.送信予算 = 局所送信予算(self.auth)

    def provider(self):
        return GoogleCloud検索供給器("minidora-507702", "minidora-public-web", self.auth,
            送信予算=self.送信予算, 許可ホスト=("source.example.test",)).制御を束縛(
                共有取得制御(公開取得上限(最小通信間隔秒=0)))

    @contextmanager
    def transport(self):
        original = socket.create_connection
        original_request = HTTPSConnection.request
        ctx = MagicMock()
        ctx.wrap_socket.side_effect = lambda sock, server_hostname: sock
        def connect(address, timeout):
            self.assertEqual(address, ("8.8.8.8", 443))
            return original(self.server.server_address, timeout=timeout)
        def request(conn, method, target, body=None, headers=None, *, encode_chunked=False):
            # ローカルfixtureだけHostを補正。検査済み接続先・本番host・SNIは別に観測する。
            return original_request(conn, method, target, body=body,
                                    headers={**(headers or {}), "Host": "127.0.0.1"},
                                    encode_chunked=encode_chunked)
        with patch("minidora.公開本文取得._公開アドレス", return_value="8.8.8.8") as DNS, \
             patch("minidora.公開本文取得.socket.create_connection", side_effect=connect), \
             patch("minidora.公開本文取得.ssl.create_default_context", return_value=ctx), \
             patch.object(HTTPSConnection, "request", request):
            yield DNS, ctx

    def test_通信先と明示認証を推測しない(self):
        for project, app, auth, hosts in (("../secret", "app", self.auth, ("source.example.test",)),
                ("minidora-507702", "a/b", self.auth, ("source.example.test",)),
                ("minidora-507702", "app", None, ("source.example.test",)),
                ("minidora-507702", "app", self.auth, ("*.example.test",)),
                ("minidora-507702", "app", self.auth, ())):
            with self.subTest(project=project, app=app, hosts=hosts), self.assertRaises((ValueError, TypeError)):
                GoogleCloud検索供給器(project, app, auth, 送信予算=self.送信予算, 許可ホスト=hosts)
        self.assertEqual(self.auth.calls, [])

    def test_通常POSTの候補だけ返し秘密と生成本文を保存しない(self):
        shared = 共有取得制御(公開取得上限(最小通信間隔秒=0))
        provider = self.provider().制御を束縛(shared)
        with self.transport() as (DNS, TLS):
            refs, 来歴 = provider.検索来歴付き("試験機器", 6)
        self.assertEqual(len(self.server.calls), 1)
        method, path, headers, body = self.server.calls[0]
        self.assertEqual(method, "POST")
        self.assertEqual(path, "/v1/projects/minidora-507702/locations/global/collections/default_collection/engines/minidora-public-web/servingConfigs/default_search:search")
        self.assertEqual(json.loads(body), {"query": "試験機器", "pageSize": 6})
        self.assertEqual(headers["authorization"], "Bearer LOCAL_TEST_TOKEN")
        self.assertIs(self.auth.calls[0][1], shared)
        DNS.assert_called_once_with("discoveryengine.googleapis.com")
        self.assertEqual(TLS.wrap_socket.call_args.kwargs["server_hostname"], "discoveryengine.googleapis.com")
        self.assertEqual(len(refs), 1)
        self.assertEqual(refs[0].本文, "")
        self.assertEqual(refs[0].URL, "https://source.example.test/body")
        encoded = json.dumps(来歴, ensure_ascii=False)
        for secret in ("LOCAL_TEST_TOKEN", "authorization", "秘密生成回答", "生成抜粋", "次の質問"):
            self.assertNotIn(secret, encoded)
        self.assertEqual(shared.記録()["使用"]["HTTP開始数"], 1)
        self.assertEqual(shared.記録()["使用"]["総バイト数"], 来歴[0]["通信"]["転送バイト数"])

    def test_許可ホスト外と不正候補と重複を本文へ渡さない(self):
        self.server.mode = "filter"
        with self.transport():
            refs, 来歴 = self.provider().検索来歴付き("試験機器", 6)
        self.assertEqual(len(refs), 1)
        self.assertEqual(来歴[0]["除外"], {"形式": 1, "URL": 1, "ホスト": 1, "重複": 1})

    def test_Bearer付きPOSTは転送も認証再送もしない(self):
        for mode in ("redirect", "denied"):
            with self.subTest(mode=mode):
                self.server.mode = mode
                self.server.calls = []
                with self.transport():
                    refs, 来歴 = self.provider().検索来歴付き("試験機器", 6)
                self.assertEqual(refs, ())
                self.assertEqual(来歴[0]["状態"], "失敗")
                self.assertIn("結果未確認", 来歴[0]["検索課金"])
                self.assertEqual(len(self.server.calls), 1)

    def test_429待機は次当回でも実POSTを抑止する(self):
        self.server.mode = "rate"
        original = self.provider()
        with self.transport():
            for _ in range(2):
                shared = 共有取得制御(公開取得上限(最小通信間隔秒=0))
                refs, 来歴 = original.制御を束縛(shared).検索来歴付き("試験機器", 6)
                self.assertEqual(refs, ())
                self.assertEqual(来歴[0]["種類"], "JSON待機要求")
        self.assertEqual(len(self.server.calls), 1)
        self.assertEqual(len(self.auth.calls), 1)

    def test_停止と不正検索語は認証や通信を始めない(self):
        provider = self.provider()
        with self.transport() as (DNS, _):
            for query, limit in (("", 1), ("秘密\n内容", 1), ("a" * 513, 1), ("試験", True), ("試験", 11)):
                with self.subTest(query=query, limit=limit), self.assertRaises(ValueError):
                    provider.検索来歴付き(query, limit)
            shared = 共有取得制御(公開取得上限(), 停止要求=lambda: True)
            with self.assertRaises(取得資源不足):
                provider.制御を束縛(shared).検索来歴付き("試験機器", 6)
        DNS.assert_not_called()
        self.assertEqual(self.auth.calls, [])
        self.assertEqual(self.server.calls, [])

    def test_任意ヘッダーをAPIへ送信せず秘密を診断に含めない(self):
        self.auth.headers["Host"] = "secret.example"
        with self.transport():
            refs, 来歴 = self.provider().検索来歴付き("試験機器", 6)
        self.assertEqual(refs, ())
        self.assertEqual(来歴[0]["種類"], "ValueError")
        self.assertEqual(self.server.calls, [])
        self.assertNotIn("secret.example", json.dumps(来歴))

    def test_検索URLから独立本文を取得し認証を本文hostへ渡さない(self):
        provider = GoogleCloud知識取得器("minidora-507702", "minidora-public-web", self.auth,
                                        送信予算=self.送信予算, 許可ホスト=("source.example.test",))
        shared = 共有取得制御(公開取得上限(最小通信間隔秒=0))
        with self.transport():
            結果 = provider.制御を束縛(shared).実行(
                知識取得要求("試験機器", ("電圧",)), 外部読取許可=True)
        self.assertTrue(結果.成立)
        self.assertEqual(結果.本文, "電圧は120 V。")
        self.assertEqual([c[0] for c in self.server.calls], ["POST", "GET"])
        self.assertNotIn("authorization", {k.lower(): v for k, v in self.server.calls[1][2].items()})
        used = shared.記録()["使用"]
        self.assertEqual((used["検索回数"], used["候補数"], used["本文取得数"], used["HTTP開始数"]), (1, 1, 1, 2))
        self.assertEqual(shared.記録()["制御未対応"], [])

    def test_POST後の受信枠停止でも課金未確認を保ち再送しない(self):
        provider = GoogleCloud知識取得器("minidora-507702", "minidora-public-web", self.auth,
                                        送信予算=self.送信予算, 許可ホスト=("source.example.test",))
        shared = 共有取得制御(公開取得上限(最小通信間隔秒=0, 最大総バイト数=1))
        with self.transport():
            結果 = provider.制御を束縛(shared).実行(
                知識取得要求("試験機器", ("電圧",)), 外部読取許可=True)
        self.assertFalse(結果.成立)
        self.assertEqual(結果.保留理由, "取得資源:総バイト数上限")
        self.assertIn({"作用": "外部検索課金", "状態": "結果未確認"}, 結果.データ["試行"])
        self.assertEqual(len(self.server.calls), 1)
        self.assertEqual(shared.記録()["使用"]["HTTP開始数"], 1)


if __name__ == "__main__":
    unittest.main()

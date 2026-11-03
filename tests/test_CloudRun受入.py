"""受入CLIの認証配送・独立監査契約を、通信せずに検査する。製品品質の評価ではない。"""
from base64 import b64encode
from contextlib import redirect_stdout, redirect_stderr
from copy import deepcopy
from hashlib import sha256
from importlib.util import module_from_spec, spec_from_file_location
from io import StringIO
import json
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch
from urllib.parse import urlsplit


_SPEC = spec_from_file_location("cloud_acceptance", Path(__file__).resolve().parents[1] / "tools" / "CloudRun受入.py")
CLI = module_from_spec(_SPEC)
_SPEC.loader.exec_module(CLI)


def _hash(value):
    return sha256(json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


def _trace(version, message="入力", session="session", response="確認待ち", identifier="a" * 32):
    event = {"番号": 1, "段階": "処理", "モジュール": "試験", "版": "v1", "入力": message,
             "出力": response, "根拠": [], "前ハッシュ": "0" * 64}
    event["ハッシュ"] = _hash({"spec": version, "追跡": identifier, "n": 1, "stage": event["段階"],
                           "モジュール": event["モジュール"], "version": event["版"], "input": message,
                           "output": response, "証拠": [], "prev": "0" * 64})
    record = {"仕様版": version, "追跡ID": identifier, "セッションID": session, "開始時刻": "2026-10-13T00:00:00+00:00",
              "入力文": message, "経路": "HDS通常運用", "イベント": [event], "最終応答": response,
              "状態": "SUSPEND", "前応答ハッシュ": ""}
    record["ルートハッシュ"] = _hash({"spec": version, "追跡": identifier, "session": session,
                                  "started": record["開始時刻"], "input": message, "経路": record["経路"],
                                  "response": response, "status": "SUSPEND", "prev_response": "",
                                  "last_event": event["ハッシュ"]})
    return record


class _Response:
    def __init__(self, status, body):
        self.status = status
        self.headers = {"Content-Type": "application/json" if isinstance(body, dict) else "text/html"}
        self.body = (json.dumps(body).encode() if isinstance(body, dict) else body.encode())
    def __enter__(self): return self
    def __exit__(self, *args): return False
    def read(self, limit): return self.body[:limit]


class _Transport:
    """送信要求をメモリだけに保持。正常認証でも意味未成立を返しゲートのFAILを確認する。"""
    def __init__(self, basic, token, public, leak=None):
        self.basic, self.token, self.public, self.leak = basic, token, public, leak
        self.requests, self.traces = [], {}
    def open(self, request, timeout):
        headers = {key.lower(): value for key, value in request.header_items()}
        self.requests.append((urlsplit(request.full_url).path, headers))
        if not self.public and headers.get("x-serverless-authorization") != "Bearer " + self.token:
            return _Response(403, {})
        if headers.get("authorization") != self.basic:
            return _Response(401, {})
        path = urlsplit(request.full_url).path
        if self.leak is not None:
            return _Response(200, {"leak": self.leak})
        if path == "/": return _Response(200, "<html></html>")
        if path == "/health": return _Response(200, {"ok": True})
        if path == "/api/capabilities": return _Response(200, {"capabilities": ["試験"]})
        if path.startswith("/api/trace/"):
            return _Response(200, {"valid": True, "追跡": self.traces[path.rsplit("/", 1)[1]]})
        if headers.get("origin") != "https://test.run.app": return _Response(403, {})
        try:
            data = json.loads(request.data)
            if not isinstance(data, dict) or not data.get("message"): return _Response(400, {})
        except (ValueError, TypeError): return _Response(400, {})
        identifier = format(len(self.traces) + 1, "032x")
        record = _trace(CLI.安全監査仕様版, data["message"], data.get("session_id", ""), identifier=identifier)
        self.traces[identifier] = record
        return _Response(200, {"session_id": record["セッションID"], "status": "SUSPEND", "response": record["最終応答"],
                              "経路": record["経路"], "追跡_id": identifier, "追跡_hash": record["ルートハッシュ"],
                              "metadata": {"ADK": {"ライブラリ": "google-adk", "版": "2.11.0",
                                  "実行方式": "Workflow/FunctionNode", "通過": True, "HDS呼出数": 1,
                                  "実行ID": "mock-execution", "追跡ID": identifier}}})


class 受入契約試験(unittest.TestCase):
    def test_v1とv2の独立ハッシュ(self):
        for version in (CLI.監査仕様版, CLI.安全監査仕様版):
            with self.subTest(version=version):
                record = _trace(version)
                before = deepcopy(record)
                self.assertTrue(CLI.監査を検証(record))
                self.assertEqual(record, before)
                record["最終応答"] = "改変"
                self.assertFalse(CLI.監査を検証(record))

    def test_版偽装と不正イベントを拒否(self):
        record = _trace(CLI.安全監査仕様版)
        for name, value in (("仕様版", CLI.監査仕様版), ("仕様版", "unknown"), ("イベント", [])):
            broken = deepcopy(record); broken[name] = value
            self.assertFalse(CLI.監査を検証(broken))
        record["イベント"][0]["番号"] = True
        self.assertFalse(CLI.監査を検証(record))
        self.assertFalse(CLI.監査を検証({}))

    def test_BasicとIAMの独立配送(self):
        self.assertEqual(CLI.認証ヘッダ("Basic test", "a.b.c"),
                         {"Authorization": "Basic test", "X-Serverless-Authorization": "Bearer a.b.c"})
        self.assertEqual(CLI.認証ヘッダ("Basic test", ""), {"Authorization": "Basic test"})
        self.assertEqual(CLI.認証ヘッダ("", "a.b.c"), {"Authorization": "Bearer a.b.c"})
        self.assertEqual(CLI.認証ヘッダ("Basic test", "a.b.c", 認証=False), {})
        self.assertEqual(CLI.認証ヘッダ("Basic test", "a.b.c", Basic状態="omit"),
                         {"X-Serverless-Authorization": "Bearer a.b.c"})
        self.assertNotEqual(CLI.認証ヘッダ("Basic test", "", Basic状態="wrong")["Authorization"], "Basic test")

    def test_私有資格形式と不正資格の非表示(self):
        with TemporaryDirectory() as directory:
            path = Path(directory) / "credentials.json"
            path.write_text(json.dumps({"username": "judge", "password": "secret-sentinel-123", "created_at": "2026-10-13"}))
            header, password = CLI.基本認証を読む(path)
            self.assertEqual(header, "Basic " + b64encode(b"judge:secret-sentinel-123").decode())
            self.assertEqual(password, "secret-sentinel-123")
            path.write_text('{"username":"bad:name","password":"secret-sentinel-123"}')
            with self.assertRaises(ValueError) as caught: CLI.基本認証を読む(path)
            self.assertNotIn("secret-sentinel-123", str(caught.exception))

    def _run(self, public, *, leak=None, token=True):
        with TemporaryDirectory() as directory:
            directory = Path(directory)
            password, oidc = "secret-sentinel-123", "header.payload.signature"
            basic = "Basic " + b64encode(("judge:" + password).encode()).decode()
            creds, deployment, output = (directory / name for name in ("private.json", "deployment.json", "output.json"))
            creds.write_text(json.dumps({"username": "judge", "password": password}), encoding="utf-8")
            deployment.write_text(json.dumps({"サービス": "test", "リージョン": "asia-northeast1", "リビジョン": "test-r1",
                "URL": "https://test.run.app", "実装SHA": "a" * 40, "認証必須": True, "利用者認証": "HTTP Basic",
                "審査公開": public, "CloudIAM本人認証必須": not public}), encoding="utf-8")
            tokenfile = directory / "token.txt"
            tokenfile.write_text(oidc, encoding="utf-8")
            argv = ["CloudRun受入.py", "--URL", "https://test.run.app", "--環境", "CloudRun", "--配備記録", str(deployment),
                    "--basic-credentials-file", str(creds), "--出力", str(output)]
            if token and not public: argv += ["--token-file", str(tokenfile)]
            transport = _Transport(basic, oidc, public, leak=leak)
            stdout, stderr = StringIO(), StringIO()
            with patch.object(CLI.sys, "argv", argv), patch.object(CLI, "build_opener", return_value=transport), \
                    patch.dict(CLI.os.environ, {}, clear=True), redirect_stdout(stdout), redirect_stderr(stderr):
                try: code = CLI.main()
                except SystemExit as caught: code = caught.code
            text = output.read_text(encoding="utf-8") if output.exists() else ""
            for secret in (password, oidc, basic, basic.removeprefix("Basic ")):
                self.assertNotIn(secret, text + stdout.getvalue() + stderr.getvalue())
            return code, json.loads(text) if text else None, transport.requests

    def test_公開Basicだけで配送し意味FAILを維持(self):
        code, report, requests = self._run(True)
        self.assertEqual(code, 1)
        self.assertFalse(report["受入成立"])
        checks = {row["項目"]: row["成立"] for row in report["検査"]}
        self.assertFalse(checks["ADKからHDS算術"])
        self.assertFalse(checks["連続依頼の再表現"])
        self.assertTrue(checks["Basic未指定拒否"])
        self.assertTrue(checks["Basic誤資格拒否"])
        self.assertFalse(any("x-serverless-authorization" in headers for _, headers in requests))
        self.assertEqual(report["認証方式"], "HTTP Basic")

    def test_非公開はBasicとIAMを別headerへ(self):
        code, report, requests = self._run(False)
        self.assertEqual(code, 1)
        self.assertEqual(report["認証方式"], "HTTP Basic+Cloud Run IAM")
        self.assertTrue(all(headers.get("x-serverless-authorization", "").startswith("Bearer ")
                            for _, headers in requests if headers.get("authorization", "").startswith("Basic ")))
        checks = {row["項目"]: row["成立"] for row in report["検査"]}
        self.assertTrue(checks["Basic未指定拒否"])
        self.assertTrue(checks["Basic誤資格拒否"])

    def test_非公開のIAM欠落は通信前拒否(self):
        code, report, requests = self._run(False, token=False)
        self.assertEqual(code, 2)
        self.assertIsNone(report)
        self.assertEqual(requests, [])

    def test_資格が応答へ反射されても保存しない(self):
        code, report, _ = self._run(True, leak="secret-sentinel-123")
        self.assertEqual(code, 1)
        self.assertFalse(report["受入成立"])
        checks = {row["項目"]: row["成立"] for row in report["検査"]}
        self.assertFalse(checks["Web UI"])
        self.assertFalse(checks["health"])

    def test_旧IAM配備は維持しBasic新境界の矛盾を拒否(self):
        with TemporaryDirectory() as directory:
            path = Path(directory) / "deployment.json"
            value = {"サービス": "test", "リージョン": "asia-northeast1", "リビジョン": "r1",
                     "URL": "https://test.run.app", "実装SHA": "a" * 40, "認証必須": True}
            path.write_text(json.dumps(value), encoding="utf-8")
            self.assertEqual(CLI.配備を読む(path, value["URL"]), value)
            value.update({"利用者認証": "HTTP Basic", "審査公開": True, "CloudIAM本人認証必須": True})
            path.write_text(json.dumps(value), encoding="utf-8")
            with self.assertRaises(ValueError): CLI.配備を読む(path, value["URL"], 基本認証=True)
            value["CloudIAM本人認証必須"] = False
            path.write_text(json.dumps(value), encoding="utf-8")
            with self.assertRaises(ValueError): CLI.配備を読む(path, value["URL"])
            self.assertEqual(CLI.配備を読む(path, value["URL"], 基本認証=True), value)


if __name__ == "__main__":
    unittest.main()

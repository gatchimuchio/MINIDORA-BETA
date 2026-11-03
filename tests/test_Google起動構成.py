"""固定設定と既存HDSへの接続を検査。Cloud Run実機の受入ではない。"""
from contextlib import redirect_stderr
from importlib.metadata import PackageNotFoundError, version
from io import StringIO
import os
import unittest
from unittest.mock import patch

from minidora.製品版.Google起動構成 import Google固定設定を読む, GoogleCloud取得器を構成, Google起動設定不正
from minidora.製品版.__main__ import main

try:
    SDK導入済み = version("google-auth") == "2.61.0"
except PackageNotFoundError:
    SDK導入済み = False


def _設定():
    # 環境変数の外部固定名。内部の意味鍵とは別に扱う。
    名称列 = (
        "MINIDORA_GOOGLE_PROJECT_ID", "MINIDORA_GOOGLE_SEARCH_APP_ID",
        "MINIDORA_GOOGLE_RUNTIME_SERVICE_ACCOUNT", "MINIDORA_GOOGLE_PUBLIC_HOSTS",
        "MINIDORA_GOOGLE_BUDGET_ID", "MINIDORA_GOOGLE_BUDGET_BUCKET",
        "MINIDORA_GOOGLE_BUDGET_OBJECT", "MINIDORA_GOOGLE_BUDGET_START_UTC",
        "MINIDORA_GOOGLE_BUDGET_END_UTC", "MINIDORA_GOOGLE_BUDGET_LIMIT")
    値列 = ("minidora-507702", "local-only",
           "minidora-local-test@minidora-507702.iam.gserviceaccount.com", "source.example.test",
           "local-only", "minidora-local-test-budget", "budget/requests.json",
           "2026-10-13T00:00:00Z", "2026-12-02T00:00:00Z", "1000")
    return dict(zip(名称列, 値列))


class Google起動構成契約試験(unittest.TestCase):
    def test_固定設定は同じ対象を束縛し元の環境変更を持ち越さない(self):
        設定 = _設定()
        予算, ホスト, SA = Google固定設定を読む(設定)
        self.assertEqual(予算.プロジェクトID, "minidora-507702")
        self.assertEqual(予算.アプリID, "local-only")
        self.assertEqual(ホスト, ("source.example.test",))
        self.assertEqual(SA, "minidora-local-test@minidora-507702.iam.gserviceaccount.com")
        設定["MINIDORA_GOOGLE_BUDGET_LIMIT"] = "1"
        self.assertEqual(予算.累積上限, 1000)
        self.assertNotEqual(予算.設定SHA256, Google固定設定を読む(設定)[0].設定SHA256)

    def test_欠損と不正な日時整数SAを固定理由で拒否する(self):
        for 名称 in _設定():
            設定 = _設定()
            del 設定[名称]
            with self.subTest(欠損=名称), self.assertRaises(Google起動設定不正):
                Google固定設定を読む(設定)
        for 名称, 値 in (
            ("MINIDORA_GOOGLE_BUDGET_START_UTC", "2026-10-13T00:00:00+00:00"),
            ("MINIDORA_GOOGLE_BUDGET_END_UTC", "2026-02-30T00:00:00Z"),
            ("MINIDORA_GOOGLE_BUDGET_END_UTC", "2026-10-12T00:00:00Z"),
            ("MINIDORA_GOOGLE_BUDGET_LIMIT", "010"),
            ("MINIDORA_GOOGLE_BUDGET_LIMIT", "1001"),
            ("MINIDORA_GOOGLE_PROJECT_ID", " PRIVATE_SECRET_PROJECT "),
            ("MINIDORA_GOOGLE_RUNTIME_SERVICE_ACCOUNT", "secret@other-project.iam.gserviceaccount.com"),
            ("MINIDORA_GOOGLE_PUBLIC_HOSTS", "source.example.test, other.example.test")):
            設定 = _設定()
            設定[名称] = 値
            with self.subTest(不正=名称), self.assertRaises(Google起動設定不正) as 停止:
                Google固定設定を読む(設定)
            self.assertNotIn(値, str(停止.exception))

    @unittest.skipUnless(SDK導入済み, "固定版Google認証SDKが必要")
    def test_構成時には通信せず同じ認証と予算を取得器へ接続する(self):
        from minidora.製品版.Google認証 import GoogleADC認証器
        from minidora.製品版.Google検索 import GoogleCloud知識取得器
        from minidora.知識取得 import 知識取得器
        with patch("socket.socket", side_effect=AssertionError("構成時の通信")), \
                patch("socket.getaddrinfo", side_effect=AssertionError("構成時のDNS通信")), \
                patch.object(GoogleADC認証器, "CloudRun", wraps=GoogleADC認証器.CloudRun) as 認証生成, \
                patch("minidora.製品版.Google検索.GoogleCloud知識取得器", wraps=GoogleCloud知識取得器) as 取得生成:
            取得器 = GoogleCloud取得器を構成(_設定())
        self.assertIsInstance(取得器, 知識取得器)
        認証生成.assert_called_once_with("minidora-507702", "minidora-local-test@minidora-507702.iam.gserviceaccount.com")
        self.assertEqual(取得生成.call_args.args[:2], ("minidora-507702", "local-only"))
        self.assertEqual(取得生成.call_args.kwargs["送信予算"].設定.設定SHA256,
                         Google固定設定を読む(_設定())[0].設定SHA256)

    @unittest.skipUnless(SDK導入済み, "固定版Google認証SDKが必要")
    def test_公開ホストの不正とSDK自由文は値を漏らさず停止する(self):
        from minidora.製品版.Google認証 import GoogleADC認証器
        for 値 in ("source.example.test,source.example.test", "*.example.test", "localhost"):
            設定 = _設定()
            設定["MINIDORA_GOOGLE_PUBLIC_HOSTS"] = 値
            with self.assertRaises(Google起動設定不正) as 停止:
                GoogleCloud取得器を構成(設定)
            self.assertNotIn(値, str(停止.exception))
        with patch.object(GoogleADC認証器, "CloudRun", side_effect=ValueError("PRIVATE_SDK_SECRET")), \
                self.assertRaises(Google起動設定不正) as 停止:
            GoogleCloud取得器を構成(_設定())
        self.assertNotIn("PRIVATE_SDK_SECRET", str(停止.exception))

    def test_新Cloud基本チャットだけが構成し同じHDSへ注入する(self):
        for 引数 in (("--HDS", "--基本チャット", "--serve"),
                     ("--HDS", "--ADK", "--CloudRun", "--serve"),
                     ("--HDS", "--ADK", "--CloudRun", "--基本チャット", "--serve")):
            設定 = _設定()
            設定["MINIDORA_RETRIEVAL_POLICY"] = "off"
            取得器 = object()
            with self.subTest(入口=引数), patch.dict(os.environ, 設定, clear=True), \
                    patch("sys.argv", ["minidora", *引数]), \
                    patch("minidora.製品版.Google起動構成.GoogleCloud取得器を構成", return_value=取得器) as 構成, \
                    patch("minidora.HDS運用.製品.HDS製品ミニドラ") as HDS生成, \
                    patch("minidora.製品版.ADK接続.ADK製品ミニドラ") as ADK生成, \
                    patch("minidora.製品版.__main__.serve") as 配信:
                self.assertEqual(main(), 0)
            Google対象 = "--CloudRun" in 引数 and "--基本チャット" in 引数
            self.assertEqual(構成.call_count, int(Google対象))
            self.assertIs(HDS生成.call_args.kwargs["取得器"], 取得器 if Google対象 else None)
            self.assertEqual(配信.call_count, 1)
            if "--ADK" in 引数:
                ADK生成.assert_called_once_with(HDS生成.return_value)
                self.assertIs(配信.call_args.args[0], ADK生成.return_value)

    def test_新Cloudの設定欠損でHDSと配信を起動しない(self):
        with patch.dict(os.environ, {}, clear=True), \
                patch("sys.argv", ["minidora", "--HDS", "--ADK", "--CloudRun", "--基本チャット", "--serve"]), \
                patch("minidora.HDS運用.製品.HDS製品ミニドラ") as HDS生成, \
                patch("minidora.製品版.__main__.serve") as 配信, \
                redirect_stderr(StringIO()), self.assertRaises(SystemExit) as 停止:
            main()
        self.assertEqual(停止.exception.code, 2)
        self.assertEqual(HDS生成.call_count, 0)
        self.assertEqual(配信.call_count, 0)


if __name__ == "__main__":
    unittest.main()

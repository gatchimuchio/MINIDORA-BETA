"""実ADKランナーとHDSの会話・採否・監査の互換性を検査する。"""
import asyncio
from importlib.metadata import PackageNotFoundError, version
import unittest
from unittest.mock import patch

from minidora.HDS運用.製品 import HDS製品ミニドラ
from minidora.製品版.ADK接続 import ADK固定版, ADK製品ミニドラ


try:
    ADK導入済み = version("google-adk") == ADK固定版
except PackageNotFoundError:
    ADK導入済み = False


@unittest.skipUnless(ADK導入済み, "cloud追加依存のgoogle-adk固定版が必要")
class ADK接続試験(unittest.TestCase):
    def test_実ランナーの通過証拠と既存監査を返す(self):
        製品 = HDS製品ミニドラ(手順形成=False)
        接続 = ADK製品ミニドラ(製品)
        with patch.object(製品, "応答", wraps=製品.応答) as 呼出:
            結果 = 接続.応答("2+3", セッションID="甲")
        self.assertEqual(呼出.call_count, 1)
        呼出.assert_called_once_with("2+3", セッションID="甲")
        self.assertEqual(結果.状態, "APPROVE")
        self.assertEqual(結果.経路, "HDS通常運用")
        self.assertEqual(接続.能力一覧(), 製品.能力一覧())
        self.assertIs(接続.監査台帳, 製品.監査台帳)
        self.assertTrue(接続.監査台帳.検証(結果.追跡ID))
        証拠 = 結果.メタデータ["ADK"]
        self.assertEqual(証拠["版"], ADK固定版)
        self.assertEqual(証拠["実行方式"], "Workflow/FunctionNode")
        self.assertTrue(証拠["通過"])
        self.assertTrue(証拠["実行ID"])
        self.assertEqual(証拠["HDS呼出数"], 1)
        self.assertEqual(証拠["追跡ID"], 結果.追跡ID)
        self.assertIn("HDS", 結果.メタデータ)

    def 同じ応答を検査(self, 直接, 接続結果):
        for 属性 in ("セッションID", "本文", "状態", "経路", "参照", "能力"):
            self.assertEqual(getattr(直接, 属性), getattr(接続結果, 属性), 属性)
        self.assertEqual(直接.メタデータ["HDS終端"], 接続結果.メタデータ["HDS終端"])

    def test_直接実行と連続依頼と条件訂正と保留が同じ(self):
        直接 = HDS製品ミニドラ(手順形成=False)
        経由元 = HDS製品ミニドラ(手順形成=False)
        接続 = ADK製品ミニドラ(経由元)
        入力列 = (
            "資料「A」を登録:単価は12円/個。数量は3個。費用は単価と数量の積。",
            "資料「A」の費用を計算して",
            "資料「A」の数量を6個に変更して",
            "短く説明して",
            "未対応の能力を発明して",
        )
        直前ハッシュ = ""
        for 入力文 in 入力列:
            with self.subTest(入力=入力文):
                前 = 直接.応答(入力文, セッションID="会話")
                後 = 接続.応答(入力文, セッションID="会話")
                self.同じ応答を検査(前, 後)
                self.assertEqual(直接.セッション("会話").状態(), 経由元.セッション("会話").状態())
                記録 = 接続.監査台帳.取得(後.追跡ID)
                self.assertEqual(記録.前応答ハッシュ, 直前ハッシュ)
                self.assertEqual(記録.ルートハッシュ, 後.監査ハッシュ)
                self.assertTrue(接続.監査台帳.検証(後.追跡ID))
                直前ハッシュ = 後.監査ハッシュ
        self.assertEqual(後.状態, "SUSPEND")

    def test_異なるHDS会話を混ぜない(self):
        接続 = ADK製品ミニドラ(HDS製品ミニドラ(手順形成=False))
        self.assertEqual(接続.応答("2+3", セッションID="甲").状態, "APPROVE")
        self.assertEqual(接続.応答("詳しく説明して", セッションID="乙").状態, "SUSPEND")
        自会話 = 接続.応答("詳しく説明して", セッションID="甲")
        self.assertEqual(自会話.状態, "APPROVE")
        self.assertIn("5", 自会話.本文)

    def test_ADKセッションを要求ごとに削除しHDS状態は保持(self):
        from google.adk.sessions import InMemorySessionService
        元削除 = InMemorySessionService.delete_session
        消去記録 = []

        async def 消去を検査(状態庫, **引数):
            await 元削除(状態庫, **引数)
            残存 = await 状態庫.get_session(**引数)
            self.assertIsNone(残存)
            消去記録.append(引数["session_id"])

        製品 = HDS製品ミニドラ(手順形成=False)
        接続 = ADK製品ミニドラ(製品)
        with patch.object(InMemorySessionService, "delete_session", 消去を検査):
            接続.応答("2+3", セッションID="甲")
            結果 = 接続.応答("詳しく説明して", セッションID="甲")
        self.assertEqual(結果.状態, "APPROVE")
        self.assertEqual(len(set(消去記録)), 2)
        self.assertNotIn("甲", 消去記録)
        self.assertGreater(製品.セッション("甲").状態()["発話数"], 0)

    def test_HDS例外を握り潰さず再試行しない(self):
        from google.adk.sessions import InMemorySessionService
        製品 = HDS製品ミニドラ(手順形成=False)
        接続 = ADK製品ミニドラ(製品)
        元削除 = InMemorySessionService.delete_session
        消去記録 = []

        async def 消去を検査(状態庫, **引数):
            await 元削除(状態庫, **引数)
            消去記録.append(引数["session_id"])

        with patch.object(InMemorySessionService, "delete_session", 消去を検査):
            with patch.object(製品, "応答", side_effect=ValueError("HDS接続試験の停止")) as 呼出:
                with self.assertRaisesRegex(ValueError, "HDS接続試験の停止"):
                    接続.応答("2+3")
                self.assertEqual(呼出.call_count, 1)
        self.assertEqual(len(消去記録), 1)

    def test_非同期入口でも同じHDSを駆動する(self):
        接続 = ADK製品ミニドラ(HDS製品ミニドラ(手順形成=False))

        async def 検査():
            結果 = await 接続.応答非同期("3*7")
            self.assertEqual(結果.状態, "APPROVE")
            self.assertIn("21", 結果.本文)
            with self.assertRaisesRegex(RuntimeError, "応答非同期"):
                接続.応答("2+3")

        asyncio.run(検査())

    def test_未確認のADK版で実行しない(self):
        with patch("minidora.製品版.ADK接続.version", return_value="2.0.0"):
            with self.assertRaisesRegex(RuntimeError, ADK固定版):
                ADK製品ミニドラ(HDS製品ミニドラ())

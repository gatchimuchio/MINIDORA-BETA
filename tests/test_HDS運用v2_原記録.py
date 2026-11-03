"""資料・成果・原文選択の依存と原子的な採用を検査する。"""
from copy import deepcopy
import json
import unittest
from unittest.mock import patch
from minidora.HDS運用 import HDS運用セッション
from minidora.HDS運用.値 import 封緘
from minidora.長文脈管理 import 文脈選択要求
from minidora.長文脈接続 import 長文脈要求資料
from minidora.能力合成 import 合成計画, 合成工程, 素材参照
from minidora.製品版.型 import 能力結果


class 原記録運用試験(unittest.TestCase):
    def setUp(self):
        self.会話 = HDS運用セッション("原記録試験", 手順形成=False)

    def 成立(self, 結果):
        self.assertTrue(結果.成立, (結果.本文, 結果.理由))
        return 結果

    def test_資料登録を同じHDS循環で自動同期(self):
        結果 = self.成立(self.会話.資料を登録("A", "数値は123です。"))
        名前 = self.会話.状態()["原記録資料"]["A"]
        記録 = self.会話.目録.原記録庫.原記録(名前)
        self.assertTrue(記録["現行"])
        self.assertEqual(記録["内容"]["本文"], "数値は123です。")
        self.assertIn("運用/原記録同期", [行["作用ID"] for 行 in 結果.追跡["作用"]])

    def test_資料更新で旧原文を残して失効(self):
        self.会話.資料を登録("A", "旧原文")
        旧 = self.会話._原記録.資料対応["A"]
        self.成立(self.会話.資料を登録("A", "新原文", 更新=True))
        self.assertFalse(self.会話.目録.原記録庫.原記録(旧)["現行"])
        self.assertEqual(self.会話.目録.原記録庫.原記録(旧)["内容"]["本文"], "旧原文")

    def test_同じ版への再更新は原記録を重複登録しない(self):
        self.会話.資料を登録("A", "原文")
        起点 = self.会話.目録.原記録庫.起点().記録ハッシュ
        self.成立(self.会話.資料を登録("A", "原文", 更新=True))
        self.assertEqual(起点, self.会話.目録.原記録庫.起点().記録ハッシュ)

    def test_AからBからAへの復帰を別改訂として保持(self):
        self.会話.資料を登録("A", "甲")
        旧 = self.会話._原記録.資料対応["A"]
        self.会話.資料を登録("A", "乙", 更新=True)
        self.成立(self.会話.資料を登録("A", "甲", 更新=True))
        self.assertNotEqual(旧, self.会話._原記録.資料対応["A"])
        self.assertFalse(self.会話.目録.原記録庫.原記録(旧)["現行"])

    def test_原文再参照が長文脈能力へ到達(self):
        self.会話.資料を登録("A", "売上は731です。費用は75です。")
        結果 = self.成立(self.会話.応答("資料「A」の原文を再参照して"))
        self.assertIn("731", 結果.本文)
        self.assertIn("長文脈選択", [行["能力"] for 行 in 結果.追跡["能力試行"]])
        self.assertEqual(set(self.会話._前回依存), {"A"})

    def test_必須原文が予算を超えれば切断せず保留(self):
        self.会話.資料を登録("A", "長い原文" * 30)
        結果 = self.会話.文脈を取得(必須資料=("A",), 最大バイト数=100)
        self.assertFalse(結果.成立)
        self.assertIn("予算", 結果.本文)

    def test_検索は一致する原記録を取得(self):
        self.会話.資料を登録("A", "独立語甲")
        self.会話.資料を登録("B", "独立語乙")
        結果 = self.成立(self.会話.応答("記憶から「独立語甲」を探して"))
        self.assertIn("独立語甲", 結果.本文)
        self.assertNotIn("独立語乙", 結果.本文)

    def test_未知資料を参照しない(self):
        self.assertFalse(self.会話.応答("資料「存在しない」の原文を再参照して").成立)

    def test_復元後に旧所有者の選択を再利用しない(self):
        self.会話.資料を登録("A", "原文")
        選択 = self.会話.目録.原記録庫.選択()
        復元 = HDS運用セッション.復元(self.会話.保存())
        self.assertFalse(復元.目録.原記録庫.選択を確認(選択))
        self.成立(復元.応答("資料「A」の原文を再参照して"))

    def test_保持庫の参照を変えても新しい庫へ能力が接続される(self):
        self.会話.資料を登録("A", "原文")
        self.会話.資料を登録("B", "追加")
        庫 = self.会話.目録.原記録庫
        材料 = 長文脈要求資料(庫, 文脈選択要求((self.会話._原記録.資料対応["A"],), (), 0))
        計画 = 合成計画((合成工程("選択", ("長文脈選択",), "指示", (素材参照("入力", "選択"),)),), ("選択",))
        self.成立(self.会話.合成を実行("原文を選択", 計画, {"指示": 能力結果(True, "読む"), "選択": 材料}))

    def test_原記録保存失敗で資料だけを先に採用しない(self):
        self.会話.資料を登録("A", "前")
        前 = deepcopy(self.会話._資料)
        from minidora.HDS運用.原記録 import 運用原記録
        with patch.object(運用原記録, "同期候補", side_effect=ValueError("試験容量不足")):
            結果 = self.会話.資料を登録("A", "後", 更新=True)
        self.assertFalse(結果.成立)
        self.assertEqual(self.会話._資料, 前)

    def test_保存対応表を別資料へ変えると拒否(self):
        self.会話.資料を登録("A", "甲")
        self.会話.資料を登録("B", "乙")
        raw = json.loads(self.会話.保存())["内容"]
        raw["原記録"]["資料対応"]["A"] = raw["原記録"]["資料対応"]["B"]
        with self.assertRaises(ValueError):
            HDS運用セッション.復元(json.dumps(封緘(raw), ensure_ascii=False))

    def test_明示初期化は知識と原記録と手順も消去(self):
        self.会話.資料を登録("A", "甲", 種類="知識")
        self.成立(self.会話.応答("/初期化"))
        self.assertEqual(self.会話.状態()["原記録資料"], {})
        self.assertEqual(self.会話.状態()["知識資産数"], 0)
        self.assertEqual(self.会話.状態()["形成手順数"], 0)

    def test_失効した原資料の成果を文脈から現行利用しない(self):
        self.会話.資料を登録("A", "数値は123です。")
        self.成立(self.会話.応答("資料「A」の原文を再参照して"))
        履歴 = self.会話._原記録.保存()["庫"]["履歴"]
        成果ID = 履歴[-1]["追加"][0]["識別子"]
        self.会話.資料を登録("A", "数値は789です。", 更新=True)
        self.assertFalse(self.会話.目録.原記録庫.原記録(成果ID)["現行"])

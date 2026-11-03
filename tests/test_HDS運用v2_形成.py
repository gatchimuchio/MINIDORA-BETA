"""手順の形成・実再現・現行資料での再利用を別々に検証する。"""
from copy import deepcopy
from dataclasses import replace
import json
import unittest
from unittest.mock import patch
from minidora.HDS運用 import HDS運用セッション
from minidora.HDS運用.値 import 封緘, 指紋
from minidora.HDS運用.手順形成 import 目的鍵, 手順を束縛
from minidora.能力合成 import 合成計画, 合成工程, 登録能力
from minidora.製品版.型 import 能力結果


class 形成運用試験(unittest.TestCase):
    def 成立(self, 結果):
        self.assertTrue(結果.成立, (結果.本文, 結果.理由))
        return 結果

    def test_同一HDSで純粋工程を再実行して形成(self):
        会話 = HDS運用セッション()
        結果 = self.成立(会話.応答("2+3"))
        self.assertEqual(結果.追跡["手順形成"]["再現工程数"], 3)
        self.assertEqual(会話.状態()["形成手順数"], 1)
        self.assertEqual(結果.追跡["作用"][-1]["作用ID"], "内的/目的検証")
        形成 = next(iter(会話._形成手順.values()))
        self.assertEqual(形成["検証"], "再実行一致")
        self.assertNotIn("回答", 形成)
        self.assertNotIn("元結果", 形成)

    def test_形成済手順は次回の現行計画へ照合して使う(self):
        会話 = HDS運用セッション()
        self.成立(会話.応答("8*9"))
        結果 = self.成立(会話.応答("再計算して"))
        self.assertIn("72", 結果.本文)
        self.assertTrue(結果.追跡["手順形成"]["手順再利用"])
        self.assertEqual(結果.追跡["手順形成"]["再現工程数"], 0)
        self.assertTrue(結果.追跡["能力試行"])

    def test_手順再利用でも変更後の資料から値を計算(self):
        会話 = HDS運用セッション(再利用=False)
        for 名前, 値 in (("A",75),("B",60)):
            会話.資料を登録(名前,json.dumps({"売上":値,"単位":"円"},ensure_ascii=False))
        self.成立(会話.応答("この2つの売上を比較して"))
        会話.資料を登録("A",'{"売上":90,"単位":"円"}',更新=True)
        結果 = self.成立(会話.応答("再計算して"))
        self.assertIn("-30円", 結果.本文)
        self.assertNotIn("75円", 結果.本文)
        self.assertTrue(結果.追跡["手順形成"]["手順再利用"])
        self.assertFalse(any(行["再利用"] for 行 in 結果.追跡["能力試行"]))

    def test_再表現を新しい手順へ誤形成しない(self):
        会話 = HDS運用セッション()
        self.成立(会話.応答("2+3"))
        結果 = self.成立(会話.応答("詳しく説明して"))
        self.assertEqual(結果.追跡["手順形成"]["再現工程数"], 0)
        self.assertEqual(会話.状態()["形成手順数"], 1)

    def test_形成を無効にしても同じ結果(self):
        会話 = HDS運用セッション(手順形成=False)
        結果 = self.成立(会話.応答("2+3"))
        self.assertIn("5", 結果.本文)
        self.assertEqual(会話.状態()["形成手順数"], 0)
        self.assertEqual(結果.追跡["手順形成"]["再現工程数"], 0)

    def test_再現不一致の部品を形成へ採用しない(self):
        会話 = HDS運用セッション(再利用=False)
        部品 = 会話.目録.取得("記号演算").モジュール
        元 = 部品.実行
        回数 = [0]
        def 実行(文脈):
            回数[0] += 1
            結果 = 元(文脈)
            return replace(結果, 本文=結果.本文 + "再現差") if 回数[0] > 1 else 結果
        with patch.object(部品, "実行", side_effect=実行):
            結果 = self.成立(会話.応答("2+3"))
        self.assertIn("5", 結果.本文)
        self.assertEqual(会話.状態()["形成手順数"], 0)
        self.assertIn("不一致", 結果.追跡["手順形成"]["状態"])

    def test_未契約の追加能力は再現しない(self):
        class 追加:
            名前="追加検査"
            版="v1"
            優先度=0
            回数=0
            def 判定(self,文脈): return 1.0
            def 実行(self,文脈):
                self.回数 += 1
                return 能力結果(True,"追加成果")
        部品=追加()
        会話=HDS運用セッション(追加能力=(登録能力(部品),))
        計画=合成計画((合成工程("s",(部品.名前,),"指示"),),("s",))
        self.成立(会話.合成を実行("実行する",計画,{"指示":能力結果(True,"指示")}))
        self.assertEqual(部品.回数,1)
        self.assertEqual(会話.状態()["形成手順数"],0)

    def test_手順は保存復元しても実行経路を通る(self):
        会話=HDS運用セッション()
        self.成立(会話.応答("3*7"))
        復元=HDS運用セッション.復元(会話.保存())
        結果=self.成立(復元.応答("再計算して"))
        self.assertIn("21",結果.本文)
        self.assertTrue(結果.追跡["手順形成"]["手順再利用"])
        self.assertFalse(any(行["再利用"] for 行 in 結果.追跡["能力試行"]))

    def test_保存手順の内部ハッシュ不一致を拒否(self):
        会話=HDS運用セッション()
        self.成立(会話.応答("2+3"))
        値=json.loads(会話.保存())["内容"]
        next(iter(値["形成手順"].values()))["検証"]="未検証"
        with self.assertRaises(ValueError):
            HDS運用セッション.復元(json.dumps(封緘(値),ensure_ascii=False))

    def test_別目的に既存手順を流用しない(self):
        会話=HDS運用セッション()
        self.成立(会話.応答("2+3"))
        結果=self.成立(会話.応答("2*3"))
        self.assertIn("6",結果.本文)
        self.assertFalse(結果.追跡["手順形成"]["手順再利用"])

    def test_形成の資源が足りないときは回答だけを成立(self):
        会話=HDS運用セッション(最大作用回数=12)
        結果=self.成立(会話.応答("2+3"))
        self.assertEqual(会話.状態()["形成手順数"],0)
        self.assertIn("予算",結果.追跡["手順形成"]["状態"])

    def test_成立していない応答を形成しない(self):
        会話=HDS運用セッション()
        結果=会話.応答("未知の自由発明を完成せよ")
        self.assertFalse(結果.成立)
        self.assertEqual(会話.状態()["形成手順数"],0)

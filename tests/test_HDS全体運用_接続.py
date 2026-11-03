"""外部のネットワークだけ人工供給し、取得・計画・検証は実部品を使う。"""
from copy import deepcopy
import json
import unittest
from minidora.HDS運用 import HDS運用セッション
from minidora.HDS運用.工程 import 工程供給
from minidora.HDS運用.値 import 指紋
from minidora.製品版.型 import 能力結果, 参照資料
from minidora.能力合成 import 合成計画, 合成工程, 素材参照, 登録能力
from minidora.会話意味 import 意味目的
from minidora.役割計画 import 役割作用
from minidora.知識取得 import 知識取得器
from minidora.公開本文取得 import 本文を復号

class _検索試験器:
    def __init__(self, 初回空=False):
        self.照会 = []
        self.初回空 = 初回空
    def 検索(self, query, limit=5):
        self.照会.append(query)
        if self.初回空 and not query.endswith("数値"):
            return ()
        return (参照資料("候補", "人工試験本文", "試験", "https://example.test/doc", 本文="電圧は999V"),)

class _本文試験器:
    def __init__(self):
        self.照会 = []
    def 取得(self, url):
        self.照会.append(url)
        return 本文を復号(url, (url,), {"content-type":"text/plain; charset=utf-8"},
                          "機器Aの電圧は120Vです。".encode())

class _追加能力:
    名前 = "試験追加能力"
    版 = "試験-v1"
    優先度 = 0
    def __init__(self, 判定値=1.0, 不成立=False):
        self.判定値, self.不成立, self.回数 = 判定値, 不成立, 0
    def 判定(self, 文脈):
        return self.判定値
    def 実行(self, 文脈):
        self.回数 += 1
        # 参照を故意に返さなくても、運用系は読取資料の依存を保持する。
        return 能力結果(not self.不成立, "検査された中間成果", 保留理由="試験未成立" if self.不成立 else "")

class 作用接続試験(unittest.TestCase):
    def 検索会話(self, 初回空=False, 許可=True):
        self.検索 = _検索試験器(初回空); self.本文 = _本文試験器()
        return HDS運用セッション(外部読取許可=許可, 取得器=知識取得器(self.検索, self.本文))

    def test_取得全文を使いスニペットを計算根拠にしない(self):
        会話 = self.検索会話()
        結果 = 会話.応答("機器Aの電圧をVで調べて", 外部読取許可=True)
        self.assertTrue(結果.成立, 結果.本文)
        self.assertIn("120V", 結果.本文); self.assertNotIn("999", 結果.本文)
        self.assertTrue(self.本文.照会)

    def test_不足時の再取得もHDSの回復作用として実行(self):
        会話 = self.検索会話(初回空=True)
        結果 = 会話.応答("機器Aの電圧をVで調べて", 外部読取許可=True)
        self.assertTrue(結果.成立, 結果.本文)
        self.assertEqual(self.検索.照会, ["機器A 電圧", "機器A 電圧 V 数値"])
        self.assertTrue(any(行["作用ID"].startswith("運用/回復/") for 行 in 結果.追跡["作用"]))

    def test_個別要求の許可なしでは外部読取しない(self):
        会話 = self.検索会話()
        self.assertFalse(会話.応答("機器Aの電圧をVで調べて").成立)
        self.assertEqual(self.検索.照会, [])

    def test_実行環境の許可なしでは要求側でも昇格できない(self):
        会話 = self.検索会話(許可=False)
        self.assertFalse(会話.応答("機器Aの電圧をVで調べて", 外部読取許可=True).成立)
        self.assertEqual(self.検索.照会, [])

    def test_外部取得は新要求で再利用せず説明だけなら再取得しない(self):
        会話 = self.検索会話()
        self.assertTrue(会話.応答("機器Aの電圧をVで調べて", 外部読取許可=True).成立)
        回数 = len(self.検索.照会)
        self.assertTrue(会話.応答("詳しく説明して").成立)
        self.assertEqual(len(self.検索.照会), 回数)
        self.assertTrue(会話.応答("機器Aの電圧をVで調べて", 外部読取許可=True).成立)
        self.assertGreater(len(self.検索.照会), 回数)

    def test_科学部品を通常依頼から説明へ接続(self):
        会話 = HDS運用セッション()
        値 = {"問題": "Consider a Hamiltonian operator H = epsilon sigma.n, where n is a unit vector and sigma are Pauli spin matrices. What are the eigenvalues of the Hamiltonian operator?",
              "選択肢": ["+epsilon,-epsilon", "+epsilon*hbar/2,-epsilon*hbar/2", "+hbar/2,-hbar/2", "+1,-1"]}
        self.assertTrue(会話.資料を登録("問題", json.dumps(値)).成立)
        結果 = 会話.応答("資料「問題」の問題を解いて")
        self.assertTrue(結果.成立, 結果.本文)
        self.assertIn("+epsilon,-epsilon", 結果.本文)
        self.assertIn("科学専門作用", [行["能力"] for 行 in 結果.追跡["能力試行"]])
        self.assertTrue(会話.応答("詳しく説明して").成立)

    def test_科学入力にgoldを混ぜたら実行へ渡さない(self):
        会話 = HDS運用セッション()
        会話.資料を登録("Q", json.dumps({"問題":"question", "選択肢":["A","B"], "gold":0}))
        結果 = 会話.応答("資料「Q」の問題を解いて")
        self.assertFalse(結果.成立)

    def test_追加能力と意味契約をコード変更なしで利用(self):
        能力 = _追加能力()
        契約 = 役割作用("追加作用", 能力.名前, "追加成果",
            lambda p: (("資料", 意味目的("原資料", {"資料":p["資料"]})),),
            lambda p: {}, lambda p: True)
        会話 = HDS運用セッション(追加能力=(登録能力(能力),), 追加役割作用=(契約,))
        会話.資料を登録("A", "試験原資料")
        結果 = 会話.役割目的を実行(意味目的("追加成果", {"資料":"A"}), 原文="追加能力の目的を実行する")
        self.assertTrue(結果.成立, 結果.本文)
        self.assertEqual(能力.回数, 1)
        標準 = {行["名前"]: 行 for 行 in HDS運用セッション().能力一覧()}
        拡張 = {行["名前"]: 行 for 行 in 会話.能力一覧()}
        self.assertEqual(len(会話.能力一覧()), len(標準) + 1)
        self.assertEqual(set(拡張), set(標準) | {能力.名前})
        self.assertEqual({名: 拡張[名] for 名 in 標準}, 標準)
        self.assertEqual(会話._前回依存.keys(), {"A"})
        会話.資料を登録("A", "改訂資料", 更新=True)
        self.assertFalse(会話.状態()["前回有効"])

    def test_追加能力は既定では純粋結果として再利用しない(self):
        能力 = _追加能力()
        会話 = HDS運用セッション(追加能力=(登録能力(能力),))
        計画 = 合成計画((合成工程("単体", (能力.名前,), "指示"),), ("単体",))
        for _ in range(2):
            結果 = 会話.合成を実行("明示計画を実行する", 計画, {"指示":能力結果(True,"試験")})
            self.assertTrue(結果.成立, 結果.本文)
        self.assertEqual(能力.回数, 2)

    def test_候補の不成立から次の候補へ移る(self):
        前 = _追加能力(不成立=True); 前.名前 = "試験前候補"
        後 = _追加能力(); 後.名前 = "試験後候補"
        会話 = HDS運用セッション(追加能力=(登録能力(前), 登録能力(後)))
        計画 = 合成計画((合成工程("単体", (前.名前,後.名前), "指示"),), ("単体",))
        結果 = 会話.合成を実行("候補を実行する", 計画, {"指示":能力結果(True,"試験")})
        self.assertTrue(結果.成立, 結果.本文)
        self.assertEqual((前.回数,後.回数),(1,1))
        self.assertEqual([行["能力"] for 行 in 結果.追跡["能力試行"]], [前.名前,後.名前])

    def test_判定NaNを最大適用として採用しない(self):
        能力 = _追加能力(判定値=float('nan'))
        会話 = HDS運用セッション(追加能力=(登録能力(能力),))
        計画 = 合成計画((合成工程("単体", (能力.名前,), "指示"),), ("単体",))
        結果 = 会話.合成を実行("明示計画を実行する", 計画, {"指示":能力結果(True,"試験")})
        self.assertFalse(結果.成立)
        self.assertEqual(結果.状態,"FAIL")
        self.assertEqual(能力.回数,0)

    def test_運用途中の能力版変更を拒否する(self):
        会話 = HDS運用セッション()
        会話.目録.取得("科学専門作用").モジュール.版 = "試験改変版"
        結果 = 会話.応答("2+3")
        self.assertFalse(結果.成立)
        self.assertIn("版", 結果.本文)

    def test_参照を返さない能力の再表現も資料依存を維持(self):
        能力 = _追加能力()
        契約 = 役割作用("追加作用",能力.名前,"追加成果",
            lambda p: (("資料",意味目的("原資料",{"資料":p["資料"]})),),lambda p:{},lambda p:True)
        会話 = HDS運用セッション(追加能力=(登録能力(能力),),追加役割作用=(契約,))
        会話.資料を登録("A","原資料")
        self.assertTrue(会話.役割目的を実行(意味目的("追加成果",{"資料":"A"}),原文="実行する").成立)
        self.assertTrue(会話.応答("詳しく説明して").成立)
        self.assertIn("A", 会話._前回依存)
        会話.資料を登録("A","更新資料",更新=True)
        self.assertFalse(会話.応答("詳しく説明して").成立)

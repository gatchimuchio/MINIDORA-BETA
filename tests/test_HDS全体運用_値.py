"""有限交換形式は能力専用のtuple/list差を保持し、不正位置や非有限値を拒否する。"""
from copy import deepcopy
from dataclasses import replace
import unittest
from minidora.HDS運用.値 import 結果を保存, 結果を復元, 指紋, 正準, 計画を保存, 計画を復元, 封緘, 開封
from minidora.製品版.型 import 能力結果, 参照資料
from minidora.採否 import 実行状態
from minidora.能力合成 import 合成計画, 合成工程, 素材参照

class 交換形式試験(unittest.TestCase):
    def test_専用データの配列と組を往復保持(self):
        元 = 能力結果(True, "本文", 根拠=("由来",),
            参照=(参照資料("識別", "題", "提供", 本文="原文"),),
            データ={"組": (1, {"入子": (2, 3)}, [4, 5]), "配列": [6, (7, 8)]})
        復元 = 結果を復元(正準(結果を保存(元)))
        self.assertEqual(結果を保存(元), 結果を保存(復元))
        self.assertEqual(元.データ, 復元.データ)
        self.assertIs(type(復元.データ["組"]), tuple)
        self.assertIs(type(復元.データ["配列"]), list)

    def test_非採用状態を往復保持(self):
        for 状態 in (実行状態.保留, 実行状態.非適用, 実行状態.失敗):
            with self.subTest(状態=状態):
                元 = 能力結果(False, "診断", 保留理由="未確定", 採否状態=状態)
                self.assertEqual(結果を復元(結果を保存(元)), 元)

    def test_復元後の変更で保存資料が変わらない(self):
        元 = 結果を保存(能力結果(True, "", データ={"配列": [1, 2]}))
        前 = 指紋(元)
        結果を復元(元).データ["配列"].append(9)
        self.assertEqual(指紋(元), 前)

    def test_成立と採否の矛盾を拒否(self):
        with self.assertRaises(ValueError):
            結果を保存(能力結果(True, "", 採否状態=実行状態.保留))

    def test_非有限値を拒否(self):
        for 値 in (float('nan'), float('inf'), float('-inf')):
            with self.subTest(値=値), self.assertRaises(ValueError):
                結果を保存(能力結果(True, "", データ={"値": 値}))

    def test_組位置の負数範囲外欠落と型偽装を拒否(self):
        元 = 結果を保存(能力結果(True, "", データ={"列": [1, 2]}))
        for 経路 in ([], ["データ", "列", -1], ["データ", "列", 99], ["不在"],
                   ["データ", "列", True], ["データ", "列", "0"], "文字列"):
            with self.subTest(経路=経路), self.assertRaises(ValueError):
                記録 = deepcopy(元); 記録["組位置"] = [経路]; 結果を復元(記録)

    def test_同じ組位置の重複を拒否(self):
        元 = 結果を保存(能力結果(True, "", データ={"列": (1, 2)}))
        元["組位置"].append(["データ", "列"])
        with self.assertRaises(ValueError):
            結果を復元(元)

    def test_未知欄を拒否(self):
        元 = 結果を保存(能力結果(True, "")); 元["命令"] = "実行"
        with self.assertRaises(ValueError):
            結果を復元(元)

    def test_計画の往復は工程と参照を保持(self):
        元 = 合成計画((合成工程("前", ("能力",), "指示", (素材参照("入力", "A"),)),
                      合成工程("後", ("能力",), "指示", (素材参照("工程", "前"),))), ("後",))
        self.assertEqual(計画を復元(計画を保存(元)), 元)

    def test_封緘内容改変を拒否(self):
        元 = 封緘({"値": 1}); 元["内容"]["値"] = 2
        with self.assertRaises(ValueError):
            開封(元)

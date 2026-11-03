from __future__ import annotations

import unittest

from minidora.HDS構文化器_v1 import 公開HDSコンパイラ
from minidora.HDS参照 import HDS参照問合せ候補, HDS参照縮退問合せ候補
from minidora.HDS観測計画 import HDS参照観測要求群


class HDS構文化器構造V11試験(unittest.TestCase):
    def setUp(self) -> None:
        self.構文化器 = 公開HDSコンパイラ()

    def test_状態遷移を関係図として保持する(self) -> None:
        結果 = self.構文化器.詳細コンパイル(
            "初期状態S0から、条件CならS1へ遷移し、失敗時はrollbackしてS0へ戻す。"
        )
        names = {node.名称 for node in 結果.状態遷移.ノード}
        self.assertIn("S0", names)
        self.assertIn("S1", names)
        forward = [edge for edge in 結果.状態遷移.遷移 if edge.始点 == "S0" and edge.終点 == "S1"]
        self.assertTrue(forward)
        self.assertTrue(any("C" in " ".join(edge.条件) for edge in forward))
        rollback = [edge for edge in 結果.状態遷移.遷移 if edge.rollback先 == "S0"]
        self.assertTrue(rollback)
        self.assertTrue(any(edge.可逆 is True for edge in rollback))

    def test_暗黙知を表面ラベルでなく構造Recordへ分ける(self) -> None:
        結果 = self.構文化器.詳細コンパイル(
            "AIとは人工知能を指す。データは固定と仮定する。この条件下のみ有効であり、結果には不確実性がある。"
        )
        kinds = {record.種別 for record in 結果.暗黙知構造}
        for expected in ("定義", "前提", "射程", "不確実性"):
            self.assertIn(expected, kinds)
        definition = next(record for record in 結果.暗黙知構造 if record.種別 == "定義")
        self.assertEqual(definition.主語, "AI")
        self.assertIn("人工知能", definition.内容)

    def test_FailureSignatureからChecklistへ接続する(self) -> None:
        結果 = self.構文化器.詳細コンパイル("AIが世界を変える。")
        signatures = [sig for sig in 結果.失敗署名候補 if sig.失敗分類 == "coordinate_unfixed"]
        self.assertTrue(signatures)
        signature_id = signatures[0].署名ID
        linked = [item for item in 結果.チェックリスト if item.失敗署名参照 == signature_id]
        self.assertTrue(linked)
        self.assertTrue(any("G00" in item.関門対応 for item in linked))

    def test_不可能性要求をG03と監査R_probeへ落とす(self) -> None:
        結果 = self.構文化器.詳細コンパイル("この構成は実現可能である。")
        checks = [item for item in 結果.チェックリスト if "G03" in item.関門対応]
        self.assertTrue(checks)
        self.assertTrue(any("不可能性証拠" in item.必要証拠 for item in checks))
        self.assertTrue(結果.監査参照候補)
        self.assertTrue(any("G03" in 候補.関門対応 for 候補 in 結果.監査参照候補))

    def test_監査R_probeは主検索へ混入せず縮退時だけ利用する(self) -> None:
        結果 = self.構文化器.詳細コンパイル("Which mechanism could make this possible?")
        requests = HDS参照観測要求群(結果.IR)
        audit = [x for x in requests if x.ID.startswith("監査表層:")]
        self.assertTrue(audit)
        self.assertTrue(all(x.段階 == "fallback" for x in audit))
        primary = tuple(x for x in requests if x.段階 == "primary")
        self.assertFalse(any(x.ID.startswith("監査表層:") for x in primary))

    def test_CognitiveWorld差分は旧世界を消さず再解釈要求を出す(self) -> None:
        first = self.構文化器.詳細コンパイル("AIが市場を変える。")
        second = self.構文化器.詳細コンパイル("AIが制度を変える。", HDS履歴=(first.IR,))
        diff = second.認知世界差分
        self.assertTrue(diff.旧世界保持)
        self.assertEqual(diff.前回世界参照, first.認知世界差分.現行世界参照)
        self.assertNotEqual(diff.前回世界参照, diff.現行世界参照)
        self.assertTrue(diff.追加座標 or diff.消失座標 or diff.変更関係)
        self.assertTrue(diff.再解釈要求)

    def test_監査probeと帰還metaはK用の意味座標へ昇格しない(self) -> None:
        結果 = self.構文化器.詳細コンパイル("この構成は実現可能である。")
        kinds = {str(coord.種別) for coord in 結果.IR.座標}
        self.assertIn("監査.R_query", kinds)
        self.assertIn("帰還.現行CognitiveWorld", kinds)

    def test_v1_1機能契約は現行v1_3でも維持する(self) -> None:
        self.assertEqual(self.構文化器.構造版, "v1.3")
        結果 = self.構文化器.詳細コンパイル("AからBへ遷移する。")
        self.assertTrue(結果.状態遷移.遷移)
        self.assertTrue(結果.失敗署名候補 or 結果.監査項目)


if __name__ == "__main__":
    unittest.main()

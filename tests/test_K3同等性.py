import json
import unittest

from minidora.K3評価 import K3同等性評価を実行
from minidora.実行系 import ミニドラ


class K3機能相当試験(unittest.TestCase):
    def test_K3公開構造47項目を全通過する(self):
        結果 = K3同等性評価を実行()
        print("K3_EQUIVALENCE_JSON=" + json.dumps({
            "status": 結果["status"],
            "pass_count": 結果["pass_count"],
            "total_count": 結果["total_count"],
            "failed_tests": [item["name"] for item in 結果["failed_tests"]],
            "実行系": 結果["実行系"],
            "fit_metrics": 結果["fit_metrics"],
        }, ensure_ascii=True, default=str))
        self.assertEqual(結果["total_count"], 47)
        self.assertEqual(結果["status"], "PASS", 結果["failed_tests"])
        self.assertEqual(結果["pass_count"], 47)

    def test_MINIDORA本体からK3能力核へ到達できる(self):
        body = ミニドラ()
        body.K3知識投入([
            "Kimi K3 uses KDA.",
            "KDA performs selective temporal update.",
            "Alice is parent of Bob.",
            "Bob is parent of Carol.",
        ])
        direct = body.K3実行("What does Kimi K3 use?", "low")
        multihop = body.K3実行("What capability does Kimi K3 have?", "max")
        grandparent = body.K3実行("Who is the grandparent of Carol?", "max")
        self.assertEqual(direct.answer, "kda")
        self.assertEqual(multihop.answer, "selective temporal update")
        self.assertEqual(grandparent.answer, "alice")


if __name__ == "__main__":
    unittest.main()

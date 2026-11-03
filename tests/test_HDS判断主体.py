from __future__ import annotations

import inspect
import unittest

from minidora.hds判断主体 import HDS判断主体, MINIDORA出力, MINIDORA出力化
from minidora.模型 import 成立差, 模型検査点, 模型結果, 模型統計, 文脈付き言語状態, 内部言語状態, 関係寄与


def 模型結果を作る(*, ref_winner="A", ref_ties=(), ref_a=2, ref_b=0, total_a=2, total_b=0):
    ctx=文脈付き言語状態(内部言語状態("q","自然言語:en",frozenset()))
    diffs=(
        成立差("A",total_a,(関係寄与("参照関係寄与",ref_a),) if ref_a else ()),
        成立差("B",total_b,(関係寄与("参照関係寄与",ref_b),) if ref_b else ()),
    )
    return 模型結果(
        ctx,diffs,"A" if total_a>total_b else None,(),
        (模型検査点("一次能力作用",(("A",total_a),("B",total_b))),),
        模型統計(終端遍歴数=1),ref_winner,tuple(ref_ties),
    )


class HDS判断主体試験(unittest.TestCase):
    def test_後段HDSの入力APIはMINIDORA出力1個だけ(self):
        params=tuple(inspect.signature(HDS判断主体.判断).parameters)
        self.assertEqual(params,("self","出力"))

    def test_MINIDORAの一意な正出力だけをAPPROVEする(self):
        output=MINIDORA出力化(模型結果を作る())
        decision=HDS判断主体().判断(output)
        self.assertEqual(decision.状態,"APPROVE")
        self.assertEqual(decision.選択候補ID,"A")
        self.assertEqual(decision.外部出力状態,"OUTPUT")
        self.assertEqual(decision.運用状態,"COMMIT")
        self.assertIn("HDS_OUTPUT_APPROVED",decision.理由)

    def test_MINIDORAが出力しなければHOLDして沈黙する(self):
        output=MINIDORA出力化(模型結果を作る(ref_winner=None,ref_ties=("A","B"),ref_a=0,ref_b=0,total_a=0,total_b=0))
        decision=HDS判断主体().判断(output)
        self.assertEqual(decision.状態,"HOLD")
        self.assertIsNone(decision.選択候補ID)
        self.assertEqual(decision.外部出力状態,"SILENT")
        self.assertIn("SILENT",decision.理由)
        self.assertIn("NO_FEEDBACK_LOOP",decision.理由)

    def test_不整合なMINIDORA出力はREJECTして沈黙する(self):
        output=MINIDORA出力(
            状態="OUTPUT",候補ID="A",
            候補差=(("A",2),("B",3)),
            参照候補差=(("A",0),("B",2)),
            参照同率候補ID=(),検査点数=1,再作用回数=0,終端遍歴数=1,
        )
        decision=HDS判断主体().判断(output)
        self.assertEqual(decision.状態,"REJECT")
        self.assertIsNone(decision.選択候補ID)
        self.assertEqual(decision.外部出力状態,"SILENT")
        self.assertIn("NO_FEEDBACK_LOOP",decision.理由)

    def test_一般表層winnerは正式MINIDORA出力を上書きしない(self):
        結果=模型結果を作る(ref_winner="B",ref_a=0,ref_b=2,total_a=10,total_b=2)
        output=MINIDORA出力化(結果)
        self.assertEqual(結果.最有力候補ID,"A")
        self.assertEqual(output.候補ID,"B")
        decision=HDS判断主体().判断(output)
        self.assertEqual((decision.状態,decision.選択候補ID),("APPROVE","B"))

    def test_HDS判断はQuestion_資料_参照を受け取れない(self):
        sig=inspect.signature(HDS判断主体.判断)
        names=set(sig.parameters)
        self.assertTrue({"question_ir","候補群","参照群",'資料','参照'}.isdisjoint(names))

    def test_HDS判断結果に差し戻し状態を持たない(self):
        fields=set(HDS判断主体().判断(MINIDORA出力化(模型結果を作る())).__dataclass_fields__)
        self.assertTrue({"再試行","差し戻し","再検索","再計算"}.isdisjoint(fields))


if __name__ == "__main__":
    unittest.main()

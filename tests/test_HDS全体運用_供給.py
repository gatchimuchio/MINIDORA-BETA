"""動的な部品供給でも、選択・実行・最終採否は既存HDSの通常循環に残る。"""
from dataclasses import replace
import unittest
from minidora.HDS実行主体 import (
    HDS実行主体, HDS実行状態, HDS関数作用, HDS作用供給器,
    HDS作用結果, HDS作用状態, HDS終端,
)
from minidora.HDS駆動コア import HDS駆動コア
from minidora.統合駆動_v2.政策 import HDS運用政策


def 成立作用(名前="回答", 必要=(), 出力=("完了",), 呼出=None, 目的依存=()):
    def 実行(状態):
        if 呼出 is not None:
            呼出.append(名前)
        return HDS作用結果(HDS作用状態.成立, 追加状態=frozenset(出力))
    return HDS関数作用(名前, 実行, 入力状態=必要, 出力状態=出力, 目的依存=目的依存)


class 動的供給試験(unittest.TestCase):
    def 初期(self):
        return HDS実行状態(要求状態=frozenset({"完了"}))

    def 主体(self, 供給, **設定):
        return HDS実行主体((), 作用供給器=(HDS作用供給器("供給", 供給),),
                        政策=HDS運用政策(自動形成=False), **設定)

    def test_現状態から必要な作用が出現する(self):
        呼出 = []
        前 = 成立作用("前", 出力=("前完了",), 呼出=呼出, 目的依存=("状態:完了",))
        def 供給(状態):
            return (成立作用("後", 必要=("前完了",), 呼出=呼出),) if "前完了" in 状態.成立状態 else (前,)
        結果 = self.主体(供給).実行(self.初期())
        self.assertEqual(結果.終端, HDS終端.採用)
        self.assertEqual(呼出, ["前", "後"])
        self.assertEqual([行.作用ID for 行 in 結果.履歴], ["前", "後"])

    def test_登録順ではなく依存が実行順を決める(self):
        呼出 = []
        前 = 成立作用("前", 出力=("前完了",), 呼出=呼出)
        後 = 成立作用("後", 必要=("前完了",), 呼出=呼出)
        結果 = self.主体(lambda 状態: (後, 前)).実行(self.初期())
        self.assertEqual(結果.終端, HDS終端.採用)
        self.assertEqual(呼出, ["前", "後"])

    def test_供給器に選択させず無関係な候補を使わない(self):
        呼出 = []
        不要 = 成立作用("不要", 出力=("装飾",), 呼出=呼出)
        必要 = 成立作用("必要", 呼出=呼出)
        結果 = self.主体(lambda 状態: (不要, 必要)).実行(self.初期())
        self.assertEqual(結果.終端, HDS終端.採用)
        self.assertEqual(呼出, ["必要"])

    def test_局所成功だけでは全体採用しない(self):
        結果 = self.主体(lambda 状態: (成立作用(出力=("一部",)),)).実行(self.初期())
        self.assertEqual(結果.終端, HDS終端.保留)

    def test_同入力での無進展作用を繰り返さない(self):
        呼出 = []
        作用 = HDS関数作用("無進展", lambda 状態: 呼出.append(1) or HDS作用結果(HDS作用状態.保留), 目的依存=("状態:完了",))
        結果 = self.主体(lambda 状態: (作用,)).実行(self.初期())
        self.assertEqual(結果.終端, HDS終端.保留)
        self.assertEqual(len(呼出), 1)

    def test_供給器による状態の直接変更を拒否(self):
        初期 = replace(self.初期(), 成果=(("資料", {"値": 1}),))
        def 供給(状態):
            dict(状態.成果)["資料"]["値"] = 2
            return (成立作用(),)
        結果 = self.主体(供給).実行(初期)
        self.assertEqual(結果.終端, HDS終端.失敗)
        self.assertEqual(dict(初期.成果)["資料"]["値"], 1)
        self.assertEqual(len(結果.履歴), 0)

    def test_返却形式が配列なら拒否(self):
        結果 = self.主体(lambda 状態: [成立作用()]).実行(self.初期())
        self.assertEqual(結果.終端, HDS終端.失敗)

    def test_任意物体を作用とみなさない(self):
        結果 = self.主体(lambda 状態: (object(),)).実行(self.初期())
        self.assertEqual(結果.終端, HDS終端.失敗)

    def test_重複作用を拒否(self):
        結果 = self.主体(lambda 状態: (成立作用(), 成立作用())).実行(self.初期())
        self.assertEqual(結果.終端, HDS終端.失敗)

    def test_静的作用との衝突も拒否(self):
        作用 = 成立作用()
        主体 = HDS実行主体((作用,), 作用供給器=(HDS作用供給器("供給", lambda 状態: (作用,)),))
        self.assertEqual(主体.実行(self.初期()).終端, HDS終端.失敗)

    def test_内部予約名前空間を拒否(self):
        # 正式作用のIDを不正な供給器が変更するケース。
        作用 = 成立作用()
        作用.作用ID = "内的/目的検証"
        self.assertEqual(self.主体(lambda 状態: (作用,)).実行(self.初期()).終端, HDS終端.失敗)

    def test_供給数超過は一部だけ実行せず保留(self):
        呼出 = []
        主体 = HDS実行主体((), 作用供給器=(HDS作用供給器("供給", lambda 状態:
            tuple(成立作用(str(i), 呼出=呼出) for i in range(3))),),
            政策=HDS運用政策(最大内部生成=2))
        結果 = 主体.実行(self.初期())
        self.assertEqual(結果.終端, HDS終端.保留)
        self.assertFalse(呼出)

    def test_供給器例外は成功にしない(self):
        def 供給(状態):
            raise RuntimeError("試験用故障")
        self.assertEqual(self.主体(供給).実行(self.初期()).終端, HDS終端.失敗)

    def test_供給器の識別子重複を拒否(self):
        供給 = HDS作用供給器("同じ", lambda 状態: ())
        with self.assertRaises(ValueError):
            HDS実行主体((), 作用供給器=(供給, 供給))

    def test_開始前取消で能力を起動しない(self):
        呼出 = []
        結果 = self.主体(lambda 状態: (成立作用(呼出=呼出),), 停止要求=lambda: True).実行(self.初期())
        self.assertEqual(結果.終端, HDS終端.保留)
        self.assertEqual(len(結果.履歴), 0)
        self.assertFalse(呼出)

    def test_能力実行後の取消でも最終採用しない(self):
        呼出 = []
        結果 = self.主体(lambda 状態: (成立作用(呼出=呼出),), 停止要求=lambda: bool(呼出)).実行(self.初期())
        self.assertEqual(呼出, ["回答"])
        self.assertEqual(結果.終端, HDS終端.保留)

    def test_取消の真偽値を整数で偽装できない(self):
        結果 = self.主体(lambda 状態: (), 停止要求=lambda: 1).実行(self.初期())
        self.assertEqual(結果.終端, HDS終端.失敗)

    def test_公開コアから供給を注入できる(self):
        コア = HDS駆動コア(作用供給器=(HDS作用供給器("供給", lambda 状態: (成立作用(),)),))
        self.assertEqual(コア.実行("完了させる", 要求状態=("完了",)).終端, HDS終端.採用)

    def test_外部権限は供給後もHDSが検査する(self):
        呼出 = []
        作用 = HDS関数作用("外部", lambda 状態: 呼出.append(1) or
            HDS作用結果(HDS作用状態.成立, 追加状態=frozenset({"完了"})),
            出力状態=("完了",), 必要権限=("外部読取",))
        self.assertEqual(self.主体(lambda 状態: (作用,)).実行(self.初期()).終端, HDS終端.保留)
        self.assertFalse(呼出)

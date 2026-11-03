"""現行構文化の全文を保ち、構文化器自身の委譲と利用者の未解を混同しない。"""
from dataclasses import replace
import unittest
from minidora.HDS運用.構文化接続 import 運用構文化器, 局所接続を構成
from minidora.HDS運用 import HDS運用セッション
from minidora.HDS中間表現 import HDS座標, HDS関係, 値状態
from minidora.HDS目的射影 import _HDS照合

class 構文化責任接続試験(unittest.TestCase):
    def test_原構文化の監査情報を削除せず別ビューへ接続(self):
        接続 = 運用構文化器(); 局所 = 接続.コンパイル("2+3")
        self.assertGreater(len(接続.原IR.座標),len(局所.座標))
        self.assertTrue(接続.責任対応)
        self.assertTrue(all(not x["解消済み認定"] for x in 接続.責任対応))
        self.assertEqual(接続.原IR.原文,局所.原文)
        self.assertEqual(接続.原IR.残差,局所.残差)
        self.assertEqual(接続.原IR.関係,局所.関係)
        _HDS照合(局所,(),(),())

    def test_未解参照は消さず局所検査でも停止(self):
        接続 = 運用構文化器(); 局所 = 接続.コンパイル("それを計算して")
        self.assertTrue(局所.残差)
        self.assertEqual(局所.残差,接続.原IR.残差)
        with self.assertRaises(ValueError):
            _HDS照合(局所,(),(),())

    def test_未知の監査座標を一括免除しない(self):
        接続 = 運用構文化器(); 接続.コンパイル("2+3")
        原 = 接続.原IR
        追加 = HDS座標("archv1:999","監査.未知要求","特別な制限",値状態.留保,
                      由来="公開HDS 構文化器 構造 v1")
        局所,_ = 局所接続を構成(replace(原,座標=(*原.座標,追加)))
        self.assertIn(追加,局所.座標)
        with self.assertRaises(ValueError):
            _HDS照合(局所,(),(),())

    def test_構文化器以外の由来では委譲メタデータへ分類しない(self):
        接続 = 運用構文化器(); 接続.コンパイル("2+3")
        原 = 接続.原IR
        点 = next(x for x in 原.座標 if x.種別=="監査.構造")
        変更 = replace(点,由来="利用者資料")
        局所,_ = 局所接続を構成(replace(原,座標=tuple(変更 if x==点 else x for x in 原.座標)))
        self.assertIn(変更,局所.座標)

    def test_委譲以外の意味損失を消さない(self):
        接続 = 運用構文化器(); 接続.コンパイル("2+3")
        原 = 接続.原IR; 履歴=list(原.意味作用履歴)
        履歴[-1]=replace(履歴[-1],損失=(*履歴[-1].損失,"重要条件の未解釈"))
        局所,_=局所接続を構成(replace(原,意味作用履歴=tuple(履歴)))
        with self.assertRaises(ValueError):
            _HDS照合(局所,(),(),())

    def test_最終運用状態に現行の全文と責任対応が残る(self):
        結果=HDS運用セッション().応答("2+3")
        self.assertTrue(結果.成立,結果.本文)
        記録=dict(結果.HDS結果.状態.成果)["運用解釈"]["HDS"]
        self.assertGreater(len(記録["全文"]["座標"]),len(記録["局所作用ビュー"]["座標"]))
        self.assertTrue(記録["責任対応"])

    def test_実際の否定条件をメタデータとして免除しない(self):
        結果=HDS運用セッション().応答("2+3を計算しないで")
        self.assertFalse(結果.成立)
        self.assertFalse(結果.追跡["能力試行"])

import unittest
from minidora.統合駆動_v2.計画 import HDS作用仕様, 目的経路を構成, 作用列を構成

F = frozenset

class 計画契約試験(unittest.TestCase):
    def test_目的ラベルなしでも生産消費を接続する(self):
        specs = (
            HDS作用仕様('数量生成', 生成成果=('数量',)),
            HDS作用仕様('計算', 読取成果=('数量',), 生成成果=('計算値',)),
            HDS作用仕様('回答', 読取成果=('計算値',), 追加状態=F(('回答済',))),
        )
        path = 目的経路を構成(F(('状態:回答済',)), specs)
        self.assertEqual(path.関連作用ID, F(('数量生成', '計算', '回答')))
        self.assertEqual(path.利用先('成果:数量'), ('計算',))
        plan = 作用列を構成(F(), F(), F(('回答済',)), specs)
        self.assertEqual(plan.作用列, ('数量生成', '計算', '回答'))

    def test_中間ノードで明示依存を再訪する(self):
        specs = (
            HDS作用仕様('準備', 入力状態=F(('素材',)), 目的依存=('状態:中間',)),
            HDS作用仕様('素材作成', 追加状態=F(('素材',))),
            HDS作用仕様('回答', 入力状態=F(('中間',)), 追加状態=F(('回答済',))),
        )
        path = 目的経路を構成(F(('状態:回答済',)), specs)
        self.assertEqual(path.関連作用ID, F(('準備', '素材作成', '回答')))

    def test_未知を無関係と扱わない(self):
        specs = (HDS作用仕様('未記述'), HDS作用仕様('無関係', 追加状態=F(('別件',)), 契約完全=True))
        path = 目的経路を構成(F(('状態:回答済',)), specs)
        self.assertEqual(path.未構成作用, ('未記述',))
        self.assertEqual(path.無関係作用, ('無関係',))
        self.assertFalse(作用列を構成(F(), F(), F(('回答済',)), specs).成立)

    def test_欠落入力を宣言だけで真にしない(self):
        spec = HDS作用仕様('回答', 読取成果=('未取得',), 追加状態=F(('回答済',)))
        self.assertFalse(作用列を構成(F(), F(), F(('回答済',)), (spec,)).成立)
        self.assertEqual(作用列を構成(F(), F(), F(('回答済',)), (spec,), 可用ノード=F(('成果:未取得',))).作用列, ('回答',))

    def test_型付き中間物を事実扱いしない(self):
        specs = (HDS作用仕様('仮説形成', 生成ノード=('仮説:h',)),
                 HDS作用仕様('利用', 読取ノード=('仮説:h',), 追加状態=F(('回答済',))))
        plan = 作用列を構成(F(), F(), F(('回答済',)), specs)
        self.assertEqual(plan.作用列, ('仮説形成', '利用'))
        spec = HDS作用仕様('事実使用', 読取認識=('h',), 追加状態=F(('回答済',)))
        self.assertFalse(作用列を構成(F(), F(), F(('回答済',)), (spec,), 可用ノード=F(('仮説:h',))).成立)

    def test_循環を有限化し目的成立と誤認しない(self):
        specs = (HDS作用仕様('a', 読取成果=('b',), 生成成果=('a',)),
                 HDS作用仕様('b', 読取成果=('a',), 生成成果=('b',)),
                 HDS作用仕様('回答', 読取成果=('a',), 追加状態=F(('回答済',))))
        self.assertFalse(作用列を構成(F(), F(), F(('回答済',)), specs, 最大状態数=8).成立)

    def test_状態だけの旧計画を保持する(self):
        specs = (HDS作用仕様('前', 追加状態=F(('中間',))),
                 HDS作用仕様('後', 入力状態=F(('中間',)), 追加状態=F(('回答済',))))
        self.assertEqual(作用列を構成(F(), F(), F(('回答済',)), specs).作用列, ('前', '後'))

    def test_予算を迂回できない(self):
        spec = HDS作用仕様('回答', 追加状態=F(('回答済',)), 資源負荷=3)
        result = 作用列を構成(F(), F(), F(('回答済',)), (spec,), 最大資源=2)
        self.assertFalse(result.成立)
        self.assertTrue(result.打切り)

if __name__ == '__main__':
    unittest.main()

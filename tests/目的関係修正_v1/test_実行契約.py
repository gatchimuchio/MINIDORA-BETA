"""追加契約を実際の通常循環で検証する。正例と拒否例は同じ公開入口を使う。"""
from dataclasses import replace
import unittest
from minidora.HDS実行主体 import (
    HDS実行主体, HDS実行状態, HDS関数作用, HDS作用結果, HDS作用状態,
    HDS終端, HDS作用供給器,
)
from minidora.統合駆動_v2.政策 import HDS運用政策, 停止理由
from minidora.統合駆動_v2.計画 import HDS作用仕様, 作用列を構成
from minidora.統合駆動_v2.仮説 import HDS仮説
from minidora.統合駆動_v2.依存 import HDS依存辺
from minidora.統合駆動_v2.目的保持 import 目的契約署名


def 主体(群, **kw):
    return HDS実行主体(群, 最大作用回数=20,
        政策=kw.pop('政策', HDS運用政策(初期作用予算=1, 予算増分=1, 自動形成=False)), **kw)


def 問い():
    return HDS実行状態(目的=('数量から計算した値を回答する',), 要求状態=frozenset({'回答'}))


class 通常循環契約試験(unittest.TestCase):
    def test_成果だけの中間工程を軟予算1で順次消費する(self):
        a=HDS関数作用('数量取得', lambda s: HDS作用結果(HDS作用状態.成立, 成果=(('数量', 21),)),
                      生成成果=('数量',), 契約完全=True)
        b=HDS関数作用('計算', lambda s: HDS作用結果(HDS作用状態.成立, 成果=(('計算値', s.成果辞書()['数量']*2),)),
                      読取成果=('数量',), 生成成果=('計算値',), 契約完全=True)
        c=HDS関数作用('回答', lambda s: HDS作用結果(HDS作用状態.成立, 成果=(('回答文',str(s.成果辞書()['計算値'])),), 追加状態=frozenset({'回答'})),
                      読取成果=('計算値',), 出力状態=('回答',), 契約完全=True)
        result=主体((c,b,a)).実行(問い())
        self.assertEqual(result.終端,HDS終端.採用,result.阻害履歴)
        self.assertEqual([h.作用ID for h in result.履歴],['数量取得','計算','回答'])
        self.assertEqual(result.状態.成果辞書()['回答文'],'42')
        self.assertEqual(result.計装.予算拡張数,2)
        self.assertTrue(all(h.目的進展 for h in result.履歴))
        self.assertEqual(result.目的経路.利用先('成果:数量'),('計算',))
        self.assertIn(HDS依存辺('成果:数量','成果:計算値'), result.状態.依存)
        self.assertIn(HDS依存辺('成果:計算値','状態:回答'),result.状態.依存)
        self.assertTrue(all(h.目的契約==目的契約署名(問い()) for h in result.履歴))
        self.assertGreater(result.計装.機会時間ns,0)
        self.assertGreater(result.計装.作用時間ns,0)

    def test_生成すると宣言しただけでは後続を動かさない(self):
        called=[]
        a=HDS関数作用('空の生産者', lambda s: HDS作用結果(HDS作用状態.成立),生成成果=('数量',))
        b=HDS関数作用('消費者',lambda s:(called.append(1) or HDS作用結果(HDS作用状態.成立,追加状態=frozenset({'回答'}))),
                      読取成果=('数量',),出力状態=('回答',))
        result=主体((a,b)).実行(問い())
        self.assertEqual(result.終端,HDS終端.保留)
        self.assertEqual(called,[])
        self.assertEqual(result.計装.目的進展数,0)

    def test_仮説の生成は仮説の消費へ接続できる(self):
        a=HDS関数作用('仮説生成',lambda s:HDS作用結果(HDS作用状態.成立,仮説更新=(HDS仮説('h','検証前の説明候補'),)),生成ノード=('仮説:h',))
        b=HDS関数作用('仮説を提示',lambda s:HDS作用結果(HDS作用状態.成立,追加状態=frozenset({'回答'})),
                      読取ノード=('仮説:h',),出力状態=('回答',))
        result=主体((a,b)).実行(問い())
        self.assertEqual(result.終端,HDS終端.採用,result.阻害履歴)
        self.assertEqual(result.状態.認識,())
        self.assertIn(HDS依存辺('仮説:h','状態:回答'),result.状態.依存)

    def test_仮説生成を事実要求へ短絡しない(self):
        called=[]
        a=HDS関数作用('仮説生成',lambda s:HDS作用結果(HDS作用状態.成立,仮説更新=(HDS仮説('h','仮説'),)),生成ノード=('仮説:h',))
        b=HDS関数作用('事実消費',lambda s:(called.append(1) or HDS作用結果(HDS作用状態.成立,追加状態=frozenset({'回答'}))),
                      読取認識=('h',),出力状態=('回答',))
        result=主体((a,b)).実行(問い())
        self.assertEqual(result.終端,HDS終端.保留)
        self.assertEqual(called,[])
        self.assertEqual(result.計装.作用実行数,0)

    def test_完全に宣言済みの無関係作用は機会も呼ばない(self):
        called=[]
        a=HDS関数作用('無関係',lambda s:HDS作用結果(HDS作用状態.成立),
                      出力状態=('別件',),契約完全=True,機会判定=lambda s:(called.append(1) or True))
        result=主体((a,)).実行(問い())
        self.assertEqual(called,[])
        self.assertIn('無関係',result.目的経路.無関係作用)
        self.assertGreater(result.計装.構成前除外数,0)

    def test_未記述契約は無関係と確定しない(self):
        a=HDS関数作用('未記述',lambda s:HDS作用結果(HDS作用状態.成立))
        result=主体((a,)).実行(問い())
        self.assertIn('未記述',result.目的経路.未構成作用)
        self.assertNotIn('未記述',result.目的経路.無関係作用)
        self.assertEqual(result.計装.作用実行数,0)

    def test_動的供給から得た消費契約を先に完全生産契約へ接続する(self):
        a=HDS関数作用('生産',lambda s:HDS作用結果(HDS作用状態.成立,成果=(('中間値',7),)),生成成果=('中間値',),契約完全=True)
        b=HDS関数作用('消費',lambda s:HDS作用結果(HDS作用状態.成立,追加状態=frozenset({'回答'})),読取成果=('中間値',),出力状態=('回答',))
        source=HDS作用供給器('供給',lambda s:(b,))
        result=主体((a,),作用供給器=(source,)).実行(問い())
        self.assertEqual(result.終端,HDS終端.採用,result.阻害履歴)
        self.assertEqual([h.作用ID for h in result.履歴],['生産','消費'])

    def test_明示削除された状態を可用キャッシュで復活させない(self):
        remove=HDS作用仕様('削除',入力状態=frozenset({'元'}),削除状態=frozenset({'元'}),生成成果=('中間',))
        finish=HDS作用仕様('回答',入力状態=frozenset({'元'}),読取成果=('中間',),追加状態=frozenset({'回答'}))
        plan=作用列を構成({'元'},(),{'回答'},(remove,finish),可用ノード={'状態:元'})
        self.assertFalse(plan.成立)

    def test_機会検討にも有限予算が適用される(self):
        actors=tuple(HDS関数作用(str(i),lambda s:HDS作用結果(HDS作用状態.成立)) for i in range(5))
        result=主体(actors,政策=HDS運用政策(最大機会検討=2,自動形成=False)).実行(問い())
        self.assertEqual(result.停止種別,停止理由.予算枯渇)
        self.assertEqual(result.計装.機会検討数,2)
        self.assertEqual(result.計装.作用実行数,0)

    def test_計画側だけの入力条件も実行で照合する(self):
        called=[]
        spec=HDS作用仕様('回答',追加状態=frozenset({'回答'}),読取成果=('未取得',))
        a=HDS関数作用('回答',lambda s:(called.append(1) or HDS作用結果(HDS作用状態.成立,追加状態=frozenset({'回答'}))),計画仕様=spec)
        result=主体((a,)).実行(問い())
        self.assertEqual(result.終端,HDS終端.保留)
        self.assertEqual(called,[])

    def test_保存手順も成果生産工程を落とさない(self):
        from minidora.統合駆動_v2.形成 import HDS形成関係,形成手順を再利用
        a=HDS作用仕様('生産',生成成果=('数',))
        b=HDS作用仕様('消費',読取成果=('数',),追加状態=frozenset({'回答'}))
        memory=HDS形成関係('手順',frozenset(),frozenset({'回答'}),('生産','消費'),'文脈',('根拠',),検証契約='実測',作用契約=(('生産','v1'),('消費','v1')))
        plan=形成手順を再利用(memory,frozenset(),frozenset(),frozenset({'回答'}),(a,b),'文脈',10)
        self.assertEqual(plan.作用列,('生産','消費'))

    def test_再開の入力更新で目的正本を書き換えない(self):
        engine=主体(())
        initial=replace(問い(),主体状態=(('HDS目的正本',('元の依頼',)),))
        previous=engine.実行(initial)
        update=HDS作用結果(HDS作用状態.成立,主体状態差分=(('HDS目的正本',('別依頼',)),))
        with self.assertRaisesRegex(ValueError,'目的契約'):
            engine.再開(previous,update)
        self.assertEqual(previous.状態.主体辞書()['HDS目的正本'],('元の依頼',))


if __name__=='__main__':
    unittest.main()

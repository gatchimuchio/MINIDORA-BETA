"""未構成経路の調査許可と、実際の目的進展を分ける。"""
from dataclasses import replace
import unittest
from minidora.HDS実行主体 import (
    HDS実行主体,HDS実行状態,HDS関数作用,HDS作用結果,HDS作用状態,HDS作用供給器,HDS終端,
)
from minidora.統合駆動_v2.計画 import HDS探索契約
from minidora.統合駆動_v2.政策 import HDS運用政策,停止理由


def 契約(**kw):
    args=dict(ID='対応を調べる',不明点='計算に使う対象と倍率の対応が未観測',
              取得ノード=('成果:取得関係',),利用先=('成果:計算値',),最大試行=2,最大資源=2)
    args.update(kw)
    return HDS探索契約(**args)


def 入力():
    return HDS実行状態(目的=('数量から回答する',),要求状態=frozenset({'回答'}),
        成果=(('数量',21),),主体状態=(('探索位置',0),))


def 回答作用():
    return HDS関数作用('回答',lambda s:HDS作用結果(HDS作用状態.成立,
        追加状態=frozenset({'回答'}),成果=(('回答',str(s.成果辞書()['計算値'])),)),
        読取成果=('計算値',),出力状態=('回答',),契約完全=True)


def 実行器(actions,**kw):
    return HDS実行主体(actions,最大作用回数=20,
        政策=HDS運用政策(初期作用予算=1,予算増分=1,自動形成=False),**kw)


class 有限探索試験(unittest.TestCase):
    def test_取得した対応から新しい経路を構成して回答する(self):
        def 取得(s):
            pos=s.主体辞書()['探索位置']
            if pos==0:
                return HDS作用結果(HDS作用状態.成立,主体状態差分=(('探索位置',1),))
            return HDS作用結果(HDS作用状態.成立,成果=(('取得関係',('数量','2倍',2)),))
        probe=HDS関数作用('対応取得',取得,読取ノード=('主体:探索位置',),純粋作用=True,探索=契約())
        def supply(s):
            # 観測された対応を明示された既存演算へ束縛する。資料をコード実行しない。
            relation=s.成果辞書().get('取得関係')
            if relation!=('数量','2倍',2):
                return ()
            return (HDS関数作用('関係から構成した計算',lambda x:HDS作用結果(HDS作用状態.成立,
                成果=(('計算値',x.成果辞書()['数量']*x.成果辞書()['取得関係'][2]),)),
                読取成果=('数量','取得関係'),生成成果=('計算値',),契約完全=True),)
        engine=実行器((probe,回答作用()),作用供給器=(HDS作用供給器('対応の射影',supply),))
        result=engine.実行(入力())
        self.assertEqual(result.終端,HDS終端.採用,result.阻害履歴)
        self.assertEqual(result.状態.成果辞書()['回答'],'42')
        self.assertEqual([h.作用ID for h in result.履歴],['対応取得','対応取得','関係から構成した計算','回答'])
        self.assertFalse(result.履歴[0].目的進展)
        self.assertEqual(result.履歴[0].進展根拠,())
        self.assertIn('成果:取得関係',result.履歴[1].進展根拠)
        self.assertEqual(result.計装.探索実行数,2)
        self.assertEqual(result.計装.探索予算拡張数,1)
        self.assertTrue(all(x.探索契約ID=='対応を調べる' for x in result.履歴[:2]))

    def test_空振りの反復は宣言試行数で終わる(self):
        calls=[]
        probe=HDS関数作用('空振り',lambda s:(calls.append(1) or HDS作用結果(HDS作用状態.成立)),純粋作用=True,探索=契約())
        result=実行器((probe,回答作用())).実行(入力())
        self.assertEqual(result.終端,HDS終端.保留)
        self.assertEqual(len(calls),2)
        self.assertEqual(result.計装.目的進展数,0)
        self.assertEqual(result.計装.探索実行数,2)

    def test_再開しても探索消費をリセットしない(self):
        calls=[]
        probe=HDS関数作用('空振り',lambda s:(calls.append(1) or HDS作用結果(HDS作用状態.成立)),純粋作用=True,探索=契約())
        engine=実行器((probe,回答作用()))
        previous=engine.実行(入力())
        result=engine.再開(previous,HDS作用結果(HDS作用状態.成立,主体状態差分=(('外部情報','更新'),)))
        self.assertEqual(len(calls),2)
        self.assertEqual(result.終端,HDS終端.保留)
        self.assertEqual(result.計装.探索実行数,2)

    def test_試行数より先に契約資源上限が効く(self):
        calls=[]
        probe=HDS関数作用('調査',lambda s:(calls.append(1) or HDS作用結果(HDS作用状態.成立)),資源負荷=2,純粋作用=True,探索=契約(最大試行=5,最大資源=3))
        result=実行器((probe,回答作用())).実行(入力())
        self.assertEqual(len(calls),1)
        self.assertEqual(result.計装.消費資源,2)
        self.assertNotEqual(result.終端,HDS終端.採用)

    def test_作用IDを変えても同じ探索契約の上限を共有する(self):
        calls=[]
        def supply(s):
            name='調査'+str(s.主体辞書()['探索位置'])
            def act(state):
                calls.append(name)
                return HDS作用結果(HDS作用状態.成立,主体状態差分=(('探索位置',state.主体辞書()['探索位置']+1),))
            return (HDS関数作用(name,act,純粋作用=True,探索=契約()),)
        result=実行器((回答作用(),),作用供給器=(HDS作用供給器('動的調査',supply),)).実行(入力())
        self.assertEqual(calls,['調査0','調査1'])
        self.assertEqual(result.計装.探索実行数,2)

    def test_同じ契約IDで予算を無言拡大すると契約違反(self):
        def supply(s):
            pos=s.主体辞書()['探索位置']
            return (HDS関数作用('調査',lambda x:HDS作用結果(HDS作用状態.成立,
                主体状態差分=(('探索位置',pos+1),)),純粋作用=True,探索=契約(最大試行=2+pos)),)
        result=実行器((回答作用(),),作用供給器=(HDS作用供給器('動的調査',supply),)).実行(入力())
        self.assertEqual(result.終端,HDS終端.失敗)
        self.assertEqual(result.停止種別,停止理由.契約違反)
        self.assertEqual(result.計装.探索実行数,1)

    def test_目的への利用先が接続されない探索は実行しない(self):
        calls=[]
        probe=HDS関数作用('無関係な調査',lambda s:(calls.append(1) or HDS作用結果(HDS作用状態.成立)),
            純粋作用=True,探索=契約(利用先=('成果:別件',)))
        result=実行器((probe,回答作用())).実行(入力())
        self.assertEqual(calls,[])
        self.assertEqual(result.計装.探索実行数,0)

    def test_探索にも権限境界が適用される(self):
        calls=[]
        probe=HDS関数作用('調査',lambda s:(calls.append(1) or HDS作用結果(HDS作用状態.成立)),
            純粋作用=True,探索=契約(),必要権限=('未承認接続',))
        result=実行器((probe,回答作用())).実行(入力())
        self.assertEqual(calls,[])
        self.assertEqual(result.停止種別,停止理由.権限制約)

    def test_対象と利用先と有限上限を省略できない(self):
        for kw in ({'取得ノード':()},{'利用先':()},{'最大試行':0},{'最大資源':0},{'不明点':''}):
            with self.subTest(kw=kw),self.assertRaises((ValueError,TypeError)):
                契約(**kw)
        with self.assertRaisesRegex(ValueError,'純粋'):
            HDS関数作用('危険な再試行',lambda s:HDS作用結果(HDS作用状態.成立),探索=契約())


if __name__=='__main__':
    unittest.main()

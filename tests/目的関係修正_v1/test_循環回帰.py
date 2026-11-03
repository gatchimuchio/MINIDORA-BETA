"""元版と修正版に同じ既存入口・同じ引数で通す回帰シナリオ。"""
import unittest
from dataclasses import replace
from minidora.HDS実行主体 import (
    HDS実行主体, HDS実行状態, HDS関数作用, HDS作用結果, HDS作用状態,
    HDS作用供給器, HDS終端)
from minidora.統合駆動_v2.計画 import HDS作用仕様
from minidora.統合駆動_v2.政策 import HDS運用政策, 停止理由
from minidora.統合駆動_v2.検証 import HDS検証器
if __package__:
    from .test_目的証拠 import 仮説入力
else:
    from test_目的証拠 import 仮説入力


def policy(budget=16):
    return HDS運用政策(初期作用予算=budget, 予算増分=1, 自動形成=False)

def ready(name):
    return HDS作用結果(HDS作用状態.成立, 追加状態=frozenset({name}))


class 通常循環回帰試験(unittest.TestCase):
    def test_準備後回答を軟予算1でも失わない(self):
        first=HDS関数作用('準備',lambda s:ready('準備済'),出力状態=('準備済',))
        last=HDS関数作用('回答',lambda s:ready('回答済'),入力状態=('準備済',),出力状態=('回答済',))
        state=HDS実行状態(目的=('回答を形成する',),要求状態=frozenset({'回答済'}))
        r=HDS実行主体((first,last),政策=policy(1)).実行(state)
        self.assertEqual(r.終端,HDS終端.採用, r.理由)
        self.assertEqual(tuple(h.作用ID for h in r.履歴),('準備','回答'))
        self.assertTrue(r.履歴[0].目的進展)

    def test_中間の明示依存を失わない(self):
        first=HDS関数作用('前処理',lambda s:ready('中間'),計画仕様=HDS作用仕様('前処理'),目的依存=('状態:準備済',))
        last=HDS関数作用('回答',lambda s:ready('回答済'),入力状態=('準備済',),出力状態=('回答済',))
        middle=HDS関数作用('準備',lambda s:ready('準備済'),入力状態=('中間',),出力状態=('準備済',))
        r=HDS実行主体((first,middle,last),政策=policy()).実行(HDS実行状態(要求状態=frozenset({'回答済'})))
        self.assertEqual(r.終端,HDS終端.採用,r.理由)
        self.assertEqual([h.作用ID for h in r.履歴],['前処理','準備','回答'])

    def test_同じ目的ラベルで無限反復しない(self):
        called=[]
        def counter(s):
            called.append(1)
            return HDS作用結果(HDS作用状態.成立,主体状態差分=(('回数',len(called)),))
        action=HDS関数作用('内部更新',counter,目的依存=('状態:回答',),
                         意味入力署名=lambda s:str(s.主体辞書().get('回数',0)))
        r=HDS実行主体((action,),政策=policy(),最大作用回数=8).実行(HDS実行状態(要求状態=frozenset({'回答'})))
        self.assertNotEqual(r.終端,HDS終端.採用)
        self.assertEqual(len(called),1,'入力署名の変化だけでは再試行を認めない')
        self.assertFalse(any(h.目的進展 for h in r.履歴))

    def test_閉包済み目的を任意供給器が阻害しない(self):
        called=[]
        def broken(s):
            called.append(1)
            raise RuntimeError('任意供給器の故障')
        state=HDS実行状態(要求状態=frozenset({'回答'}),成立状態=frozenset({'回答'}))
        r=HDS実行主体((),作用供給器=(HDS作用供給器('任意',broken),),政策=policy()).実行(state)
        self.assertEqual(r.終端,HDS終端.採用)
        self.assertFalse(called)

    def test_最終検証失敗を採用しない(self):
        s=HDS実行状態(要求状態=frozenset({'回答'}),成立状態=frozenset({'回答'}))
        r=HDS実行主体((),最終検証器=(HDS検証器('値検証',lambda s,d:False),),政策=policy()).実行(s)
        self.assertNotEqual(r.終端,HDS終端.採用)
        self.assertIn('検証:値検証',r.状態.残差)

    def test_権限境界を緩和しない(self):
        called=[]
        a=HDS関数作用('回答',lambda s:(called.append(1) or ready('回答')),出力状態=('回答',),必要権限=('外部変更',))
        r=HDS実行主体((a,),政策=policy()).実行(HDS実行状態(要求状態=frozenset({'回答'})))
        self.assertNotEqual(r.終端,HDS終端.採用)
        self.assertFalse(called)

    def test_取消は回答準備済みでも優先する(self):
        r=HDS実行主体((),停止要求=lambda:True,政策=policy()).実行(HDS実行状態())
        self.assertEqual(r.停止種別,停止理由.明示停止)
        self.assertFalse(r.履歴)

    def test_通常循環で仮説を実体化する(self):
        state,rules,target=仮説入力()
        r=HDS実行主体((),関係規則=rules,政策=policy(),最大作用回数=12).実行(state)
        self.assertEqual(len(r.状態.仮説),2,r.理由)
        self.assertNotEqual(r.終端,HDS終端.採用)
        self.assertTrue(any(h.作用ID=='内的/関係仮説構成' for h in r.履歴))

    def test_読取成果欠落は署名失敗でなく保留にする(self):
        calls=[]
        def signer(s):
            calls.append(1)
            return str(s.成果辞書()['数量'])
        a=HDS関数作用('計算',lambda s:ready('回答'),出力状態=('回答',),読取成果=('数量',),意味入力署名=signer)
        r=HDS実行主体((a,),政策=policy()).実行(HDS実行状態(要求状態=frozenset({'回答'})))
        self.assertEqual(r.終端,HDS終端.保留)
        self.assertFalse(calls)

    def test_作用から目的原本を書き換えられない(self):
        a=HDS関数作用('改ざん',lambda s:HDS作用結果(HDS作用状態.成立,主体状態差分=(('HDS目的正本',('別目的',)),)),出力状態=('回答',))
        s=HDS実行状態(要求状態=frozenset({'回答'}),主体状態=(('HDS目的正本',('原要求',)),))
        r=HDS実行主体((a,),政策=policy()).実行(s)
        self.assertEqual(r.終端,HDS終端.失敗)
        self.assertEqual(r.状態.主体辞書()['HDS目的正本'],('原要求',))

    def test_利用不能成果では機会判定を呼ばない(self):
        calls=[]
        def predicate(s):
            calls.append(1)
            return s.成果辞書()['数量'] > 0
        a=HDS関数作用('計算',lambda s:ready('回答'),出力状態=('回答',),読取成果=('数量',),機会判定=predicate)
        r=HDS実行主体((a,),政策=policy()).実行(HDS実行状態(要求状態=frozenset({'回答'})))
        self.assertEqual(r.終端,HDS終端.保留)
        self.assertFalse(calls)

    def test_利用不能成果では入力署名を呼ばない(self):
        calls=[]
        def signer(s):
            calls.append(1)
            return str(s.成果辞書()['数量'])
        a=HDS関数作用('計算',lambda s:ready('回答'),出力状態=('回答',),読取成果=('数量',),入力署名=signer)
        r=HDS実行主体((a,),政策=policy()).実行(HDS実行状態(要求状態=frozenset({'回答'})))
        self.assertEqual(r.終端,HDS終端.保留)
        self.assertFalse(calls)

if __name__=='__main__': unittest.main()

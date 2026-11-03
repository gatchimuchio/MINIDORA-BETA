"""接続・再開・境界条件・有限生成の追加回帰。"""
from __future__ import annotations
from dataclasses import replace
import importlib.util
import json
from pathlib import Path
import random
import sys
import unittest
from minidora.HDS実行主体 import *
from minidora.HDS駆動コア import HDS駆動コア
from minidora.統合駆動_v2 import *
from minidora.統合駆動_v2.値 import 署名

F=frozenset


def 基礎状態():
    d=HDS資料('原典','1','値=4','人工試験','2026-09-17')
    x=HDS認識項目('x','対象','値',4,認識区分.確定,根拠=(d.出典(),),検証契約='入力検証/v1')
    return HDS実行状態(要求状態=F({'完了'}),認識=(x,),記憶=HDS記憶((d,)))


def 単純(ID='A',前=(),後=('完了',),版='v1'):
    return HDS関数作用(ID,lambda s:HDS作用結果(HDS作用状態.成立,追加状態=F(後)),
                        入力状態=前,出力状態=後,契約版=版)


class 接続追加試験(unittest.TestCase):
    def test_資料計算検証と同値根拠更新を実際に連結(self):
        path=Path(__file__).resolve().parents[1]/'tools/HDS内包統合_実演.py'
        spec=importlib.util.spec_from_file_location('minidora_demo_test',path)
        m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
        r=m.実演する()
        self.assertEqual(r['実値更新']['回答'],'積=27')
        self.assertEqual(r['実値更新']['呼出累計']['独立点検'],1)
        self.assertEqual(r['実値更新']['計装']['外部入力数'],2)
        self.assertEqual(r['実値更新']['計装']['作用実行数'],16)
        self.assertEqual(r['実値更新']['計装']['検証失敗数'],0)
        self.assertGreater(r['実値更新']['計装']['依存失効数'],0)
        self.assertNotEqual(r['初回']['状態署名'],r['同値根拠更新']['状態署名'])

    def test_保存復元で認識根拠と署名を保持(self):
        s=基礎状態(); t=復元する(保存する(s))
        self.assertEqual(t,s); self.assertEqual(t.状態署名,s.状態署名)

    def test_保存復元後も同一作用を反復しない(self):
        calls=[]; a=HDS関数作用('無進展',lambda s:calls.append(1) or HDS作用結果(HDS作用状態.保留),目的依存=('状態:完了',))
        実行主体=HDS実行主体((a,)); r=実行主体.実行(基礎状態()); t=実行主体.再開(復元する(保存する(r)))
        self.assertEqual(calls,[1]); self.assertEqual(len(r.履歴),len(t.履歴))

    def test_再開入力の前後差と挿入位置を記録(self):
        実行主体=HDS実行主体((単純(),)); r=実行主体.実行(基礎状態())
        t=実行主体.再開(r,HDS作用結果(HDS作用状態.成立,主体状態差分=(('補足','更新'),)))
        self.assertEqual(len(t.入力履歴),1); e=t.入力履歴[0]
        self.assertEqual(e.前状態署名,r.状態.状態署名); self.assertEqual(e.作用履歴位置,len(r.履歴)); self.assertTrue(e.状態差.変化有無)
        self.assertEqual(復元する(保存する(t)),t)

    def test_保存は任意objectと関数を受け付けない(self):
        for x in (object(),lambda:0,HDS検証器('v',lambda s,d:True)):
            with self.subTest(x=type(x)),self.assertRaises(TypeError):保存する(x)

    def test_復元は登録外の実行型を受け付けない(self):
        保存形式={'format':'MINIDORA-HDS-STATE-v2','内容':{'type':'os.system','fields':{}}}
        with self.assertRaises(ValueError):復元する(json.dumps(保存形式))

    def test_復元は重複鍵を受け付けない(self):
        with self.assertRaises(ValueError):復元する('{"format":"x","format":"y","内容":1}')

    def test_復元は過剰構造フィールドを拒否(self):
        d=json.loads(保存する(基礎状態())); d['内容']['fields']['秘密']='x'
        with self.assertRaises(ValueError):復元する(json.dumps(d))

    def test_保存と復元の容量上限を守る(self):
        for fun,args in ((保存する,('x'*300,)),(復元する,('x'*300,))):
            with self.subTest(fun=fun.__name__),self.assertRaises(ValueError):fun(*args,最大バイト=100)

    def test_保存の全組込み値種別を往復(self):
        x=({1:'一','1':None},(1,2.5,True),{'a','b'},F({'a'}),['x'],停止理由.証拠競合)
        self.assertEqual(復元する(保存する(x)),x)

    def test_無検証ラベルの確定を再観測要求へ戻せる(self):
        s=基礎状態(); s=replace(s,要求状態=F(),要求認識=F({'x'}),記憶=HDS記憶())
        obs=HDS観測器('再取得',lambda q,t:HDS観測値(4,s.認識[0].根拠,q.対象,q.関係,資料群=(基礎状態().記憶.正本[0],)),lambda q,v:True)
        r=HDS実行主体((),観測器=(obs,)).実行(s)
        self.assertEqual(r.終端,HDS終端.採用); self.assertEqual(r.計装.観測実行数,1)

    def test_最終検証成功済みを同じ契約で重複実行しない(self):
        calls=[]; v=HDS検証器('v',lambda s,d:calls.append(1) or True)
        実行主体=HDS実行主体((単純(),),最終検証器=(v,)); r=実行主体.実行(基礎状態()); t=実行主体.再開(復元する(保存する(r)))
        self.assertEqual(t.終端,HDS終端.採用); self.assertEqual(calls,[1])

    def test_最終検証契約変更では新しい検証を実行する(self):
        calls=[]; r=HDS実行主体((単純(),),最終検証器=(HDS検証器('v',lambda s,d:True,'1'),)).実行(基礎状態())
        t=HDS実行主体((単純(),),最終検証器=(HDS検証器('v',lambda s,d:calls.append(1) or False,'2'),)).再開(r)
        self.assertEqual(t.終端,HDS終端.保留); self.assertEqual(calls,[1])

    def test_最終検証例外でも実行した履歴を残す(self):
        def bad(s,d): raise ValueError('検証エラー')
        r=HDS実行主体((単純(),),最終検証器=(HDS検証器('v',bad),)).実行(基礎状態())
        self.assertEqual(r.終端,HDS終端.失敗); self.assertEqual(r.履歴[-1].作用ID,'内的/目的検証'); self.assertEqual(r.計装.作用実行数,len(r.履歴))

    def test_状態反映を拒否しても実行した回数を残す(self):
        a=HDS関数作用('不正証明',lambda s:HDS作用結果(HDS作用状態.成立,成果=(('x',5),),検証依存=(('認識:x','古い署名'),)),読取認識=('x',),目的依存=('状態:完了',))
        r=HDS実行主体((a,)).実行(基礎状態())
        self.assertEqual(r.終端,HDS終端.失敗); self.assertEqual(r.計装.作用実行数,1); self.assertNotIn('x',r.状態.成果辞書())

    def test_作用選択器が禁止作用を返しても実行しない(self):
        class 不正選択:
            def 選択(self,s,offers,history): return HDS作用機会('外部破壊','署名')
        r=HDS実行主体((),作用選択器=不正選択()).実行(基礎状態())
        self.assertEqual(r.終端,HDS終端.失敗); self.assertEqual(r.計装.作用実行数,0)

    def test_作用選択器は実状態を変更できない(self):
        class 不正選択:
            def 選択(self,s,offers,history): object.__setattr__(s,'主体状態',(('改変',True),)); return None
        s=基礎状態(); r=HDS実行主体((),作用選択器=不正選択()).実行(s)
        self.assertEqual(r.終端,HDS終端.失敗); self.assertEqual(r.状態,s)

    def test_権限停止を再開で迂回しない(self):
        a=HDS関数作用('write',lambda s:HDS作用結果(HDS作用状態.成立),必要権限=('write',))
        実行主体=HDS実行主体((a,)); r=実行主体.実行(基礎状態())
        self.assertEqual(r.停止種別,停止理由.権限制約)
        with self.assertRaises(ValueError):実行主体.再開(r)

    def test_小数やboolの資源量を整数へ無言変換しない(self):
        for n in (True,1.9,-1):
            with self.subTest(n=n),self.assertRaises(ValueError):HDS関数作用('x',lambda s:None,資源負荷=n)

    def test_依存グラフを順序と分岐数を変えて全下流検出する(self):
        rng=random.Random(20260917)
        for n in range(3,35):
            edges=[HDS依存辺('認識:'+str(i),'認識:'+str(j)) for i in range(n) for j in range(i+1,n) if rng.random()<0.13]
            root='認識:'+str(rng.randrange(n)); expected={root}
            while True:
                new=expected|{e.後続 for e in edges if e.前提 in expected}
                if new==expected: break
                expected=new
            a=set(下流集合({root},tuple(edges))); rng.shuffle(edges)
            self.assertEqual(a,expected-{root}); self.assertEqual(a,set(下流集合({root},tuple(edges))))

    def test_有界計画を様々な長さで実行する(self):
        for n in range(1,10):
            with self.subTest(n=n):
                acts=tuple(単純('段'+str(i),() if i==0 else (str(i-1),),(str(i),)) for i in range(n))
                r=HDS実行主体(acts,最大作用回数=20).実行(HDS実行状態(要求状態=F({str(n-1)})))
                self.assertEqual(r.終端,HDS終端.採用); self.assertEqual(len(r.履歴),n)

    def test_実行トレースから形成し再実行検証後に再利用(self):
        acts=(単純('A',(),('途中',)),単純('B',('途中',),('完了',))); specs=tuple(a.計画仕様 for a in acts)
        s=HDS実行状態(要求状態=F({'完了'}),主体状態=(('形成文脈署名','条件C'),)); 実行主体=HDS実行主体(acts)
        r=実行主体.実行(s); r2=実行主体.実行(s); e=実行結果から経験('経験1',s,r,'条件C',specs)
        rule=経験から形成(e); rule=再実行で検証(rule,実行結果から経験('経験2',s,r2,'条件C',specs),'再実行/v1')
        t=実行主体.実行(replace(s,形成関係=(rule,)))
        self.assertEqual(t.終端,HDS終端.採用); self.assertGreater(t.計装.形成再利用数,0); self.assertEqual(tuple(h.作用ID for h in t.履歴),('A','B')); self.assertLess(t.計装.探索状態数,r.計装.探索状態数)

    def test_形成手順の文脈や部品版が違えば再利用しない(self):
        rule=HDS形成関係('r',F(),F({'完了'}),('A',),'文脈',('由来',),検証契約='v',作用契約=(('A','1'),)); old=(単純('A',版='1').計画仕様,); new=(単純('A',版='2').計画仕様,)
        self.assertIsNone(形成手順を再利用(rule,F(),F(),F({'完了'}),old,'別文脈',10)); self.assertIsNone(形成手順を再利用(rule,F(),F(),F({'完了'}),new,'文脈',10))

    def test_形成検証の別手順成功を流用しない(self):
        e=HDS経験('e',F(),F({'完了'}),('A',),True,'署名','文脈'); rule=経験から形成(e)
        with self.assertRaises(ValueError):再実行で検証(rule,replace(e,作用列=('B',)),'検証')

    def test_監督モジュールをactive核からimportしない(self):
        root=Path(__file__).resolve().parents[1]/'src/minidora'; files=(root/'HDS実行主体.py',root/'HDS駆動コア.py',*(root/'統合駆動_v2').glob('*.py'))
        import ast
        for f in files:
            for node in ast.walk(ast.parse(f.read_text(encoding='utf-8'))):
                if isinstance(node,ast.ImportFrom):
                    self.assertNotIn('監督',node.module or ''); self.assertNotIn('介入制御',node.module or '')


if __name__=='__main__':unittest.main()

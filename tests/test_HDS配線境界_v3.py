"""領域横断、配線、保存と署名の境界を、公開入口も含めて再確認する。"""
from __future__ import annotations
import ast
from dataclasses import dataclass, replace
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import unittest
from minidora.HDS実行主体 import HDS実行主体, HDS実行状態, HDS終端, HDS作用結果, HDS作用状態
from minidora.統合駆動_v2 import HDS資料, HDS記憶, HDS未来状態, HDS失敗診断, HDS運用政策, 原資料を圧縮, 保存する, 復元する
from minidora.統合駆動_v2.値 import 署名

ROOT=Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('crossdomain_demo_v3',ROOT/'tools/HDS領域横断_実演.py')
demo=importlib.util.module_from_spec(spec);spec.loader.exec_module(demo)


class 領域横断試験(unittest.TestCase):
    def test_異なる三処理が同じ通常循環で完遂する(self):
        結果=demo.実演する()
        rows=結果['実演']
        self.assertTrue(all(x['終端']=='COMMIT' for x in rows))
        self.assertEqual(rows[0]['結果']['結果']['合計'],43)
        self.assertEqual(rows[1]['結果']['結果']['重複'],[['A','B']])
        self.assertEqual(rows[2]['結果']['結果']['共通'],['B'])
        for x in rows:
            seq=x['実行順']
            self.assertLess(seq.index('構造化'),seq.index('関係処理'))
            self.assertLess(seq.index('関係処理'),seq.index('報告形成'))
            self.assertEqual(x['呼出']['検証'],1)

    def test_新規の数値でも固定正解を返さない(self):
        for a,b,c,d in ((1,2,3,4),(7,13,9,5),(0,19,11,31)):
            with self.subTest(a=a,b=b):
                doc=HDS資料('入力','1',f'数量,単価\n{a},{b}\n{c},{d}\n','試験')
                sub,状態,_=demo.構成する(doc,'数量集計')
                r=sub.実行(状態)
                self.assertEqual(r.終端,HDS終端.採用)
                self.assertEqual(json.loads(r.状態.成果辞書()['報告'])['結果']['合計'],a*b+c*d)

    def test_資料差だけで必要下流が再評価され独立処理は再実行しない(self):
        doc=HDS資料('入力','1','数量,単価\n3,7\n','試験')
        sub,状態,calls=demo.構成する(doc,'数量集計')
        r=sub.実行(状態)
        d2=replace(doc,版='2')
        t=sub.再開(復元する(保存する(r)),HDS作用結果(HDS作用状態.成立,記憶更新=r.状態.記憶.更新((d2,))))
        self.assertEqual(t.終端,HDS終端.採用,t.理由)
        self.assertEqual(calls['独立'],1)
        self.assertEqual(calls['検証'],2)
        self.assertEqual(json.loads(t.状態.成果辞書()['報告'])['出典']['版'],'2')
        d3=replace(doc,版='3',本文='数量,単価\n3,9\n')
        u=sub.再開(t,HDS作用結果(HDS作用状態.成立,記憶更新=t.状態.記憶.更新((d3,))))
        self.assertEqual(u.終端,HDS終端.採用)
        self.assertEqual(json.loads(u.状態.成果辞書()['報告'])['結果']['合計'],27)
        self.assertEqual(calls['独立'],1)

    def test_必須列欠落を推測して採用しない(self):
        doc=HDS資料('入力','1','数量\n3\n','試験')
        sub,状態,calls=demo.構成する(doc,'数量集計')
        r=sub.実行(状態)
        self.assertNotEqual(r.終端,HDS終端.採用)
        self.assertEqual(calls['検証'],0)
        self.assertNotIn('報告',r.状態.成果辞書())

    def test_不正な時間範囲を成功へ変換しない(self):
        doc=HDS資料('入力','1','名称,開始,終了\nA,12,9\n','試験')
        sub,状態,_=demo.構成する(doc,'日程重複')
        r=sub.実行(状態)
        self.assertNotEqual(r.終端,HDS終端.採用)

    def test_入力状態と原資料を破壊しない(self):
        doc=HDS資料('入力','1','数量,単価\n3,7\n','試験')
        sub,状態,_=demo.構成する(doc,'数量集計')
        sig=状態.状態署名
        sub.実行(状態)
        self.assertEqual(状態.状態署名,sig)
        self.assertEqual(状態.記憶.正本[0],doc)


class 公開入口試験(unittest.TestCase):
    def run_code(self,code,seed='0'):
        env=dict(os.environ,PYTHONPATH=str(ROOT/'src'),PYTHONHASHSEED=seed,PYTHONIOENCODING='utf-8')
        return subprocess.run([sys.executable,'-c',code],env=env,check=True,capture_output=True,text=True,encoding="utf-8").stdout.strip()

    def test_公開型にアクセスしても旧監督や模型を起動しない(self):
        text=self.run_code('import sys;from minidora import HDS認識項目,HDS命題,HDS未来制約,HDS駆動コア;print([n for n in sys.modules if n.startswith("minidora.") and any(t in n for t in ("K3", "hds介入", "HDS監督", "模型_v05"))])')
        self.assertEqual(text,'[]')

    def test_plain_importは駆動コアすら読み込まない(self):
        text=self.run_code('import sys,minidora;print("minidora.HDS実行主体" in sys.modules)')
        self.assertEqual(text,'False')

    def test_同名旧作用記録の公開先を変更しない(self):
        import minidora
        self.assertEqual(minidora._公開経路['HDS作用記録'],('HDS構文化記録_v1_3','HDS作用記録'))

    def test_追加公開名の全数解決(self):
        text=self.run_code('import minidora;names=[k for k,v in minidora._公開経路.items() if v[0]=="統合駆動_v2"];assert all(getattr(minidora,k) is not None for k in names);print(len(names))')
        self.assertEqual(text,'34')

    def test_新循環に監督関数の直接依存がない(self):
        for path in (ROOT/'src/minidora/HDS実行主体.py',ROOT/'src/minidora/HDS駆動コア.py',ROOT/'src/minidora/統合駆動_v2/循環.py'):
            tree=ast.parse(path.read_text(encoding="utf-8"))
            modules=[x.module or '' for x in ast.walk(tree) if isinstance(x,ast.ImportFrom)]
            self.assertFalse(any('介入制御' in x or '監督選択' in x for x in modules))

    def test_正本のv3接続先が存在(self):
        self.assertTrue((ROOT/'設計/60_MINIDORA_HDS自律接続_v3.md').is_file())
        for name in ('HDS_MINIDORA_現行正本.md','src/README.md','設計/58_MINIDORA_HDS実行主体_v1.md'):
            self.assertIn('60_MINIDORA_HDS自律接続_v3.md',(ROOT/name).read_text(encoding="utf-8"))


class 署名保存境界試験(unittest.TestCase):
    def test_異なるハッシュ乱数種でも同一状態署名(self):
        code='from minidora.HDS実行主体 import HDS実行状態;print(HDS実行状態(成立状態=frozenset({"甲","乙","丙"}),成果=(("結果",{"b":2,"a":1}),)).状態署名)'
        test=公開入口試験()
        self.assertEqual(test.run_code(code,'0'),test.run_code(code,'2718'))

    def test_外部dataclassの構造的意味署名は保持(self):
        @dataclass(frozen=True)
        class 外部結果:
            状態:str
            値:tuple
        self.assertEqual(署名(外部結果('APPROVE',(1,2))),署名(外部結果('APPROVE',(1,2))))
        self.assertNotEqual(署名(外部結果('APPROVE',(1,2))),署名(外部結果('APPROVE',(1,3))))

    def test_任意objectをreprで無言受入しない(self):
        with self.assertRaises(TypeError):署名(object())

    def test_明示された外部型の署名契約を利用できる(self):
        class 外部型:
            def __init__(self,x):self.x=x
            def HDS署名値(self):return {'契約':'v1','値':self.x}
        self.assertEqual(署名(外部型(1)),署名(外部型(1)))
        self.assertNotEqual(署名(外部型(1)),署名(外部型(2)))

    def test_v2保存をv3意味で無言復元しない(self):
        payload=保存する(HDS実行状態())
        with self.assertRaisesRegex(ValueError,'保存形式'):
            復元する(payload.replace('MINIDORA-HDS-STATE-v3','MINIDORA-HDS-STATE-v2'))

    def test_偽の観測済み未来を復元で受け入れない(self):
        x=HDS未来状態('A',frozenset(),frozenset(),'v1')
        with self.assertRaises(ValueError):復元する(保存する(x).replace('条件付き予測','観測事実'))

    def test_圧縮上限に区切り文字も含める(self):
        d=HDS資料('doc','1','甲。'*400,'人工原文')
        c=原資料を圧縮(d,最大文字数=32)
        self.assertLessEqual(len(c.要約),32)

    def test_診断安全フラグの文字列を真扱いしない(self):
        with self.assertRaises(TypeError):HDS失敗診断('a','Exception','未知','入力',再実行安全='false')


class 多段署名反例試験(unittest.TestCase):
    def test_状態だけを読む作用も根拠更新で再開する(self):
        from minidora.HDS実行主体 import HDS関数作用
        from minidora.統合駆動_v2 import HDS認識項目, 認識区分
        d=HDS資料("証拠","1","値は同じ","入力")
        x=HDS認識項目("x","対象","属性",1,認識区分.確定,根拠=(d.出典(),),検証契約="照合")
        actions=(HDS関数作用("出発",lambda s:HDS作用結果(HDS作用状態.成立,追加状態=frozenset({"開始"})),読取認識=("x",),出力状態=("開始",)),
                 HDS関数作用("中間",lambda s:HDS作用結果(HDS作用状態.成立,追加状態=frozenset({"中間"})),入力状態=("開始",),出力状態=("中間",),入力署名=lambda s:"同一契約"),
                 HDS関数作用("終端",lambda s:HDS作用結果(HDS作用状態.成立,追加状態=frozenset({"完了"})),入力状態=("中間",),出力状態=("完了",),入力署名=lambda s:"同一契約"))
        コア=HDS実行主体(actions)
        r=コア.実行(HDS実行状態(要求状態=frozenset({"完了"}),認識=(x,),記憶=HDS記憶((d,))))
        d2=replace(d,版="2")
        t=コア.再開(r,HDS作用結果(HDS作用状態.成立,認識更新=(replace(x,根拠=(d2.出典(),)),),記憶更新=r.状態.記憶.更新((d2,))))
        self.assertEqual(t.終端,HDS終端.採用,t.理由)
        self.assertEqual([h.作用ID for h in t.履歴],["出発","中間","終端"]*2)

    def test_循環依存のノード署名は有限に計算される(self):
        from minidora.統合駆動_v2 import HDS依存辺
        s=HDS実行状態(成立状態=frozenset({"a","b"}),依存=(HDS依存辺("状態:a","状態:b"),HDS依存辺("状態:b","状態:a")))
        self.assertEqual(s.ノード署名("状態:a"),s.ノード署名("状態:a"))

    def test_無関係な成果変更は読取ノードの署名を変えない(self):
        s=HDS実行状態(成果=(("x",1),("unrelated",2)))
        t=replace(s,成果=(("x",1),("unrelated",3)))
        self.assertEqual(s.ノード署名("成果:x"),t.ノード署名("成果:x"))


if __name__=='__main__':unittest.main()

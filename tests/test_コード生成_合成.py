"""構造化仕様からの生成・既存合成・多段解決・独立したPython実行の対照。"""
from copy import deepcopy
from dataclasses import replace
from itertools import product
import json
from pathlib import Path
import runpy
import subprocess
import sys
import unittest

from minidora.コード能力 import コードを読む, コードを評価, コードを検証
from minidora.コード生成 import 関数を生成
from minidora.コード能力接続 import コード能力群, コード能力モジュール
from minidora.多段解決 import 多段解決器
from minidora.能力合成 import 能力合成器, 合成計画, 合成工程, 素材参照
from minidora.製品版.能力契約 import 能力文脈
from minidora.製品版.型 import 能力結果

ROOT=Path(__file__).resolve().parents[1]
DEMO=runpy.run_path(str(ROOT/'tools/コード能力デモ.py'))
仕様=DEMO['二乗和仕様']
例題=DEMO['問題を用意']


def 参照(name):
    return {'種別':'参照','名前':name}


def 定数(key):
    return {'種別':'定数参照','キー':key}


def 返す(expr):
    return {'名前':'処理','引数':['x'],'手順':[{'種別':'返却','式':expr}]}


class 生成契約試験(unittest.TestCase):
    def test_分岐反復算術からコードを生成(self):
        r=関数を生成(仕様(),{'初期値':0,'閾値':2})
        self.assertTrue(r.成立,r.保留理由)
        self.assertIn("if 数 > 定数['閾値']:",r.本文)
        self.assertTrue(コードを読む(r.本文).成立)
        value=コードを評価(r.本文,{'数列':[1,2,3,4],'定数':r.データ['定数']})
        self.assertEqual(value.データ['値'],25)

    def test_定数実値を生成コードに埋め込まない(self):
        a=関数を生成(仕様(),{'初期値':0,'閾値':2})
        b=関数を生成(仕様(),{'初期値':0,'閾値':731})
        self.assertEqual(a.本文,b.本文)
        self.assertEqual(a.データ['ソースSHA256'],b.データ['ソースSHA256'])
        self.assertNotEqual(a.データ['定数'],b.データ['定数'])
        self.assertNotIn('731',b.本文)

    def test_同じ生成コードを別の定数で再利用(self):
        r=関数を生成(仕様(),{'初期値':0,'閾値':2})
        a=コードを評価(r.本文,{'数列':[2,3,4],'定数':{'初期値':0,'閾値':2}})
        b=コードを評価(r.本文,{'数列':[2,3,4],'定数':{'初期値':0,'閾値':3}})
        self.assertEqual((a.データ['値'],b.データ['値']),(25,16))

    def test_設定入力に試験や期待値は渡さない(self):
        r=関数を生成(仕様(),{'初期値':0,'閾値':2})
        self.assertNotIn('期待値',r.本文)
        self.assertNotIn('試験',r.データ['仕様'])
        self.assertIn('入出力試験は別',r.データ['検証'])

    def test_生成した条件選択と添字(self):
        expr={'種別':'選択','条件':{'種別':'比較','演算':'超','左':参照('x'),'右':定数('境界')},'真':定数('上'),'偽':定数('下')}
        r=関数を生成(返す(expr),{'境界':1,'上':[3,4],'下':[1]})
        self.assertTrue(r.成立)
        self.assertEqual(コードを評価(r.本文,{'x':2,'定数':r.データ['定数']}).データ['値'],[3,4])
        r=関数を生成(返す({'種別':'添字','対象':参照('x'),'位置':定数('位置')}),{'位置':-1})
        self.assertEqual(コードを評価(r.本文,{'x':[3,7],'定数':r.データ['定数']}).データ['値'],7)

    def test_生成の組込呼出とリスト(self):
        expr={'種別':'呼出','関数':'合計','引数':[{'種別':'リスト','要素':[参照('x'),定数('値')]}]}
        r=関数を生成(返す(expr),{'値':3})
        self.assertEqual(コードを評価(r.本文,{'x':4,'定数':r.データ['定数']}).データ['値'],7)

    def test_未知操作を勝手に別実装へ置換しない(self):
        for expr in ({'種別':'LLMに委譲'},{'種別':'算術','演算':'未対応','左':参照('x'),'右':参照('x')},
                     {'種別':'呼出','関数':'ファイル削除','引数':[]}):
            self.assertFalse(関数を生成(返す(expr),{}).成立)

    def test_仕様の未知フィールドを無視しない(self):
        s=仕様();s['無視すべき']=True
        self.assertFalse(関数を生成(s,{'初期値':0,'閾値':2}).成立)
        s=仕様();s['手順'][0]['追加']=True
        self.assertFalse(関数を生成(s,{'初期値':0,'閾値':2}).成立)

    def test_必要定数がなければ埋めない(self):
        self.assertFalse(関数を生成(仕様(),{'初期値':0}).成立)

    def test_入力名の予約語重複不正文字(self):
        for name in ('def','定数','sum','__x','a b','a\nreturn 1'):
            s=仕様();s['引数']=[name]
            self.assertFalse(関数を生成(s,{'初期値':0,'閾値':2}).成立)
        s=仕様();s['引数']=['x','x']
        self.assertFalse(関数を生成(s,{'初期値':0,'閾値':2}).成立)

    def test_偽分岐の不正な空値も黙って受理しない(self):
        for bad in ({},None,False,''):
            s=仕様();s['手順'][1]['手順'][0]['偽']=bad
            self.assertFalse(関数を生成(s,{'初期値':0,'閾値':2}).成立)

    def test_元仕様と定数を変更しない(self):
        s,c=仕様(),{'初期値':0,'閾値':2};old=deepcopy((s,c))
        r=関数を生成(s,c)
        r.データ['仕様']['名前']='改変'
        r.データ['定数']['閾値']=999
        self.assertEqual((s,c),old)

    def test_循環や巨大仕様は不成立(self):
        n={'種別':'算術','演算':'加算','右':参照('x')};n['左']=n
        self.assertFalse(関数を生成(返す(n),{}).成立)
        s=仕様();s['手順']=s['手順']*200
        self.assertFalse(関数を生成(s,{'初期値':0,'閾値':2}).成立)

    def test_生成後も未束縛変数は試験で不成立(self):
        r=関数を生成(返す(参照('未定義')), {})
        self.assertTrue(r.成立)
        v=コードを評価(r.本文,{'x':1,'定数':{}})
        self.assertFalse(v.成立)
        self.assertEqual(v.データ['診断'],'NameError')

    def test_選別に使わない入力をPython本体と独立照合(self):
        r=関数を生成(仕様(),{'初期値':0,'閾値':2})
        # 自分で構成した既知の有限コードだけを試験用CPythonへ渡す。
        env={};exec(r.本文,env)
        count=0
        for threshold in (-1,2,5):
            for xs in product((-3,0,2,4,7),repeat=3):
                constants={'初期値':0,'閾値':threshold}
                結果=コードを評価(r.本文,{'数列':list(xs),'定数':constants})
                expected=sum(x*x for x in xs if x>threshold)
                self.assertTrue(結果.成立,結果.保留理由)
                self.assertEqual(結果.データ['値'],expected)
                self.assertEqual(env['条件付き二乗和'](list(xs),constants),expected)
                count+=1
        self.assertEqual(count,375)


class コード合成試験(unittest.TestCase):
    def test_生成から評価へ実値を渡す(self):
        資料={'仕様':能力結果(True,'',データ={'仕様':仕様(),'定数':{'初期値':0,'閾値':2}}),
              '指示':能力結果(True,'生成'),
              '引数':能力結果(True,'',データ={'引数':{'数列':[1,2,3,4],'定数':{'初期値':0,'閾値':2}}})}
        plan=合成計画((合成工程('生成',('コード生成',),'指示',(素材参照('入力','仕様'),)),
                       合成工程('評価',('コード評価',),'指示',(素材参照('工程','生成'),),'引数')),('評価',))
        r=能力合成器(コード能力群()).実行(plan,資料)
        self.assertTrue(r.成立,r.理由)
        self.assertEqual(r.出力[0][1].データ['値'],25)
        self.assertTrue(r.監査整合())

    def test_多段解決で境界バグ候補を退ける(self):
        p,資料=例題()
        r=多段解決器(コード能力群(),純粋作用確認=True).実行(p,資料)
        self.assertTrue(r.成立,r.理由)
        self.assertEqual([s['解法'] for s in r.採用経路],['境界を含まない候補'])
        self.assertTrue(any(e['作用']=='目的条件未達' and e['解法']=='境界を含む候補' for e in r.履歴))
        self.assertEqual(r.呼出数,4)

    def test_代替なしは試験条件を緩めない(self):
        p,資料=例題(False)
        r=多段解決器(コード能力群(),純粋作用確認=True).実行(p,資料)
        self.assertFalse(r.成立)
        self.assertEqual(r.出力,())

    def test_候補生成を既存検証が独立に監査(self):
        p,資料=例題()
        r=多段解決器(コード能力群(),純粋作用確認=True).実行(p,資料)
        code=r.出力[0][1]
        結果=コードを検証(code.本文,({'引数':{'数列':[-3,2,5,8],'定数':code.データ['定数']},'期待値':89},))
        self.assertTrue(結果.成立)

    def test_未知設定と任意の会話を実行しない(self):
        for モジュール in コード能力群():
            c=能力文脈('勝手にファイルを実行して','s')
            self.assertEqual(モジュール.モジュール.判定(c),0)
            self.assertFalse(モジュール.モジュール.実行(c).成立)
        with self.assertRaises(ValueError):
            コード能力モジュール('任意実行')

    def test_呼出者の試験データを候補書換えに使わない(self):
        p,資料=例題()
        original=deepcopy(資料)
        多段解決器(コード能力群(),純粋作用確認=True).実行(p,資料)
        self.assertEqual(original,資料)

    def test_コード読解も同じ契約で呼べる(self):
        p=合成計画((合成工程('a',('コード読解',),'i',(素材参照('入力','c'),)),),('a',))
        d={'i':能力結果(True,'読む'),'c':能力結果(True,'def f(x):\n return x+1')}
        r=能力合成器(コード能力群()).実行(p,d)
        self.assertTrue(r.成立)
        self.assertEqual(r.出力[0][1].データ['関数'],'f')

    def test_上流不成立ならコードを呼ばない(self):
        p=合成計画((合成工程('a',('コード読解',),'i',(素材参照('入力','c'),)),),('a',))
        d={'i':能力結果(True,'読む'),'c':能力結果(False,'def f():\n return 1')}
        r=能力合成器(コード能力群()).実行(p,d)
        self.assertFalse(r.成立)
        self.assertEqual(r.実行数,0)

    def test_独立CLIの成功と代替除去(self):
        for args in ([],['--代替なし']):
            r=subprocess.run([sys.executable,str(ROOT/'tools/コード能力デモ.py'),*args],capture_output=True,encoding='utf-8',timeout=15)
            self.assertEqual(r.returncode,0,r.stderr)
            d=json.loads(r.stdout)
            self.assertTrue(d['監査整合'])
            self.assertEqual(d['状態'],'保留' if args else '合格')

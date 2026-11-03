"""数量グラフと生成Pythonの独立実行、説明改変、数値変形対照。"""
from copy import deepcopy
from fractions import Fraction
import json
import random
import subprocess
import sys
import unittest
from minidora.HDS運用.数量構造 import 数量を構成,数量を評価
from minidora.HDS運用.数量生成 import 数量コード仕様,数量コード実行,数量を照合,数量を説明,数量回答を検査
from minidora.コード生成 import 関数を生成
from minidora.製品版.型 import 能力結果
from test_HDS運用v3_数量言語 import 要求


class 数量生成試験(unittest.TestCase):
    def 組(self, 本文='甲は12円/個。乙は3個。結果は甲と乙の積。'):
        式=数量を構成({'入力':本文},要求())
        計算=数量を評価(式)
        仕様=数量コード仕様(式)
        コード=関数を生成(**仕様)
        self.assertTrue(コード.成立,コード.保留理由)
        検算=数量コード実行(式,コード.本文)
        照合=数量を照合(式,計算,検算)
        return 式,計算,検算,照合

    def test_複数分母の単位を曖昧にしない(self):
        from minidora.HDS運用.数量生成 import 単位表記
        self.assertEqual(単位表記({"m":1,"kg":-1,"s":-1}),"m/(kg*s)")
        self.assertEqual(単位表記({"s":-2}),"1/s^2")
        self.assertEqual(単位表記({"s":-2,"m":1}),"m/s^2")

    def test_既存コード生成器へ立式結果を渡す(self):
        式,計算,検算,照合=self.組()
        self.assertTrue(照合['一致'])
        self.assertIn('定数[',検算['ソース'])
        self.assertNotIn('return 36',検算['ソース'])

    def test_実Pythonでも同じ値になる(self):
        for 文 in ('結果は0.1+0.2。','結果は(17-2)/3。','結果は-9/2。','甲は7円/個。乙は8個。結果は甲*乙。'):
            with self.subTest(文=文):
                式,計算,検算,照合=self.組(文)
                実行=検算['ソース']+'\nprint(数量を検算('+repr(検算['定数'])+'))\n'
                子=subprocess.run([sys.executable,'-I','-S','-c',実行],capture_output=True,text=True,timeout=5)
                self.assertEqual(子.returncode,0,子.stderr)
                数=json.loads(子.stdout)
                self.assertEqual(Fraction(*数[0]),Fraction(計算['結果'][0]['値']))

    def test_数値変形200組で独立期待値と照合(self):
        乱数=random.Random(91318)
        for 番号 in range(200):
            a,b,c,d=(乱数.randint(-30,30) for _ in range(4))
            d=d or 1
            文=f'甲は{a}。乙は{b}。丙は{c}。丁は{d}。結果は(甲*乙+丙)/丁。'
            with self.subTest(番号=番号):
                式,計算,検算,照合=self.組(文)
                self.assertEqual(Fraction(計算['結果'][0]['値']),Fraction(a*b+c,d))
                self.assertEqual(Fraction(*検算['評価']['値'][0]),Fraction(a*b+c,d))

    def test_値だけ変えても生成コードは同じ(self):
        前=self.組('甲は12。乙は3。結果は甲*乙。')[2]
        後=self.組('甲は15。乙は4。結果は甲*乙。')[2]
        self.assertEqual(前['ソース'],後['ソース'])
        self.assertNotEqual(前['定数'],後['定数'])
        self.assertNotEqual(前['評価']['値'],後['評価']['値'])

    def test_演算が違えばコードが変わる(self):
        self.assertNotEqual(self.組('結果は3+2。')[2]['ソース'],self.組('結果は3*2。')[2]['ソース'])

    def test_コード改変を採用しない(self):
        式,_,検算,_=self.組()
        with self.assertRaises(ValueError):数量コード実行(式,検算['ソース'].replace(' * ',' + '))

    def test_計算値改変を拒否(self):
        式,計算,検算,_=self.組();計算['結果'][0]['値']='999'
        with self.assertRaises(ValueError):数量を照合(式,計算,検算)

    def test_検算値改変を拒否(self):
        式,計算,検算,_=self.組();検算['評価']['値'][0][0]=999
        with self.assertRaises(ValueError):数量を照合(式,計算,検算)

    def test_定数改変を拒否(self):
        式,計算,検算,_=self.組();検算['定数']['項0分子']=999
        with self.assertRaises(ValueError):数量を照合(式,計算,検算)

    def test_説明を値と根拠から再構成(self):
        組=self.組();本文,資料=数量を説明(*組,表示={'詳細':True,'コード':True,'形式':'文章','読者':'技術者'})
        self.assertIn('36円',本文);self.assertIn('原文対応',本文);self.assertIn('```python',本文)
        self.assertTrue(数量回答を検査(能力結果(True,本文,データ=資料)))

    def test_説明本文や留保の改変を拒否(self):
        組=self.組();本文,資料=数量を説明(*組)
        self.assertFalse(数量回答を検査(能力結果(True,本文.replace('36円','99円'),データ=資料)))
        改=deepcopy(資料);改['本文']=本文.replace('36円','99円')
        self.assertFalse(数量回答を検査(能力結果(True,改['本文'],データ=改)))

    def test_短縮でも条件や単位を落とさない(self):
        式,計算,検算,照合=self.組()
        for 詳細 in (False,True):
            本文,_=数量を説明(式,計算,検算,照合,表示={'詳細':詳細,'コード':False,'形式':'表','読者':'一般'})
            self.assertIn('36円',本文);self.assertIn('現実の妥当性',本文)

    def test_巨大な共有式を指数的に文章展開しない(self):
        # 同一子の重複利用でも節点数・出力長は線形に留まる。
        文='項目0は1。'+''.join(f'項目{i}は項目{i-1}+項目{i-1}。' for i in range(1,9))+'結果は項目8。'
        組=self.組(文)
        本文,_=数量を説明(*組,表示={'詳細':True,'コード':False,'形式':'文章','読者':'一般'})
        self.assertLess(len(本文),3000)
        self.assertIn('256',本文)

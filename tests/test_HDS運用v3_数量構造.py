"""資料間の役割・不足・循環・単位・要求との対応を検査する。"""
from copy import deepcopy
from fractions import Fraction
import unittest
from minidora.HDS運用.数量構造 import 数量を構成,数量を評価,構造を検査,要求を検査
from test_HDS運用v3_数量言語 import 要求


class 数量構造試験(unittest.TestCase):
    def test_共通規則を複数資料へ束縛(self):
        資料={"A":"単価は12円/個。数量は3個。","B":"単価は7円/個。数量は4個。","規則":"費用は単価と数量の積。"}
        式=数量を構成(資料,要求("費用",['A','B'],['規則'],['計算','比較','合計','平均','最大','最小']))
        結果=数量を評価(式)
        self.assertEqual([x['値'] for x in 結果['結果']],['36','28'])
        self.assertEqual(結果['集計']['比較'][0]['差'],'-8')
        self.assertEqual({k:v for k,v in 結果['集計'].items() if k!='比較'}, {'合計':'64','平均':'32','最大':'36','最小':'28'})

    def test_連鎖依存を共有し記載順で結果を変えない(self):
        for 文 in ('結果は甲+甲。甲は乙*2。乙は3。','乙は3。甲は乙*2。結果は甲+甲。'):
            式=数量を構成({'入力':文},要求())
            self.assertEqual(数量を評価(式)['結果'][0]['値'],'12')
            self.assertEqual(len(式['節点']),4)

    def test_不足量を明示して止める(self):
        with self.assertRaisesRegex(ValueError,'確認待ち:数量不足:入力の乙'):
            数量を構成({'入力':'甲は2。結果は甲*乙。'},要求())

    def test_自己循環と相互循環を止める(self):
        for 文 in ('結果は結果+1。','結果は甲+1。甲は結果+1。'):
            with self.subTest(文=文),self.assertRaisesRegex(ValueError,'循環'):数量を構成({'入力':文},要求())

    def test_単位不一致の加減算(self):
        for 演算 in ('+','-'):
            with self.subTest(演算=演算),self.assertRaisesRegex(ValueError,'単位不一致'):
                数量を評価(数量を構成({'入力':f'結果は3円{演算}2個。'},要求()))

    def test_単位換算を勝手に補わない(self):
        with self.assertRaisesRegex(ValueError,'単位不一致'):
            数量を評価(数量を構成({'入力':'結果は1m+100cm。'},要求()))

    def test_ゼロ除算を止める(self):
        with self.assertRaisesRegex(ValueError,'ゼロ除算'):
            数量を評価(数量を構成({'入力':'結果は3/(2-2)。'},要求()))

    def test_比較単位も一致が必要(self):
        with self.assertRaisesRegex(ValueError,'単位が不一致'):
            数量を評価(数量を構成({'A':'結果は3円。','B':'結果は2個。'},要求(対象=['A','B'],操作=['計算','比較'])))

    def test_共通と個別の競合を無言上書きしない(self):
        with self.assertRaisesRegex(ValueError,'定義競合'):
            数量を構成({'A':'結果は3。','規則':'結果は4。'},要求(対象=['A'],規則=['規則']))

    def test_共通規則間の競合(self):
        with self.assertRaisesRegex(ValueError,'共通定義が競合'):
            数量を構成({'A':'甲は3。','R':'結果は甲+1。','S':'結果は甲+2。'},要求(対象=['A'],規則=['R','S']))

    def test_条件変更の原文と使用量を保持(self):
        q=要求();q['条件変更']=[{'資料':'入力','数量':'甲','値':'5個','原文':'甲は5個','範囲':[2,4]}]
        式=数量を構成({'入力':'甲は3個。結果は甲*2。'},q)
        self.assertEqual(数量を評価(式)['結果'][0]['値'],'10')
        self.assertEqual(式['資料']['入力'],'甲は3個。結果は甲*2。')
        self.assertTrue(any(x['資料'].startswith('条件変更:') for x in 式['束縛']))

    def test_不足量へ明示条件を補う(self):
        q=要求();q['条件変更']=[{'資料':'入力','数量':'甲','値':'5個','原文':'甲は5個','範囲':[2,4]}]
        式=数量を構成({'入力':'結果は甲*2。'},q)
        self.assertEqual(数量を評価(式)['結果'][0]['値'],'10')

    def test_無関係な条件を勝手に使わない(self):
        q=要求();q['条件変更']=[{'資料':'入力','数量':'乙','値':'5個','原文':'乙は5個','範囲':[2,4]}]
        with self.assertRaises(ValueError):数量を構成({'入力':'甲は3個。結果は甲*2。'},q)

    def test_寄与しない条件も捨てて通さない(self):
        q=要求();q['条件変更']=[{'資料':'入力','数量':'乙','値':'5個','原文':'乙は5個','範囲':[2,4]}]
        with self.assertRaisesRegex(ValueError,'寄与しない'):
            数量を構成({'入力':'甲は3個。乙は4個。結果は甲*2。'},q)

    def test_構造改変を再ハッシュに頼らず検出(self):
        式=数量を構成({'入力':'結果は3+2。'},要求())
        改=deepcopy(式);改['節点'][0]['値']='7'
        with self.assertRaises(ValueError):構造を検査(改)
        改=deepcopy(式);改['出力'][0]['節点']='項0'
        with self.assertRaises(ValueError):構造を検査(改)

    def test_未使用資料記載を履歴へ保持(self):
        式=数量を構成({'入力':'結果は3+2。時点は2025年。'},要求())
        self.assertEqual(式['未使用記載'][0]['数量'],'時点')

    def test_要求に未知鍵や重複を通さない(self):
        for 差 in ({'gold':1},{'対象':['入力','入力']},{'表示':{'詳細':True}},{'操作':['計算','送信']}):
            with self.subTest(差=差),self.assertRaises((ValueError,TypeError)):要求を検査(要求()|差)

    def test_巨大数値の演算を境界内で止める(self):
        文='甲は'+str(2**120)+'。結果は甲*甲*甲。'
        with self.assertRaisesRegex(ValueError,'数値上限'):数量を評価(数量を構成({'入力':文},要求()))

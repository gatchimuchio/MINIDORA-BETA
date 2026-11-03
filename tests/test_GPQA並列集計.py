from unittest import TestCase
from copy import deepcopy
from GPQA並列集計 import 集計
from GPQA実測管理 import 資料SHA256

def 群(score=40):
    cond={'資料集合CSV_SHA256':資料SHA256,'全問題数':198,'選択肢シャッフル種':0,
        '実行方式':'問題独立並列','問題間継続状態':False,'参照方式':'LIVE_ONLY',
        '固定参照資料許可':False,'採点結果の学習利用':False,'中核入口':'HDS駆動コア.選択実行',
        'リポジトリ版':'revision','OpenAlex有効':False,'EuropePMC有効':True,'Crossref有効':True,
        'Wikipedia言語群':['en'],'全数wall_clock上限分':90}
    rows=[{'番号':i,'問題束形成回数':1,'予測ラベル':'A','正解ラベル':'A' if i<score else 'B','正答':i<score,'回答済み':True,'終端':'COMMIT'}for i in range(198)]
    return [{'評価条件':{**cond,'選択番号群':list(range(i,min(198,i+5)))},'測定状態':'完了','実測':{'完走':True,'開始epoch':100,'終了epoch':200},'個票':rows[i:i+5]}for i in range(0,198,5)]
class 並列集計試験(TestCase):
    def 結果(self,g=None,now=300):return 集計(g if g is not None else 群(),版='revision',開始=100,締切=5500,現在=now)
    def test_40点以上の一意完走を採用(self):self.assertTrue(self.結果()['性能継承成立'])
    def test_39点は退行(self):self.assertFalse(self.結果(群(39))['性能継承成立'])
    def test_途中結果の合算を全数にしない(self):self.assertFalse(self.結果(群()[:-1])['性能継承成立'])
    def test_番号重複拒否(self):g=群();g[-1]['個票']=[g[0]['個票'][0]];self.assertFalse(self.結果(g)['性能継承成立'])
    def test_不正SHAを拒否(self):g=群();g[0]['評価条件']['資料集合CSV_SHA256']='wrong';self.assertFalse(self.結果(g)['性能継承成立'])
    def test_混合版を拒否(self):g=群();g[0]['評価条件']['リポジトリ版']='old';self.assertFalse(self.結果(g)['性能継承成立'])
    def test_時点不一致を拒否(self):g=群();g[0]['実測']['開始epoch']=200;self.assertFalse(self.結果(g)['性能継承成立'])
    def test_各shardだけ90分内でも全体超過を拒否(self):self.assertFalse(self.結果(now=5501)['性能継承成立'])
    def test_終了時刻超過を拒否(self):g=群();g[0]['実測']['終了epoch']=5501;self.assertFalse(self.結果(g)['性能継承成立'])
    def test_自己申告スコアを盲信しない(self):g=群(39);g[0]['指標']={'正答':198};self.assertFalse(self.結果(g)['性能継承成立'])
    def test_採点欄偽装を拒否(self):g=群();g[-1]['個票'][0]['正答']=True;self.assertFalse(self.結果(g)['性能継承成立'])
    def test_状態継承を混ぜない(self):g=群();g[0]['評価条件']['問題間継続状態']=True;self.assertFalse(self.結果(g)['性能継承成立'])
    def test_結果が揃っても異常終了は採用しない(self):g=群();g[0]['測定状態']='処理失敗';self.assertFalse(self.結果(g)['性能継承成立'])
    def test_参照条件を変更しない(self):g=群();g[0]['評価条件']['参照方式']='REPLAY';self.assertFalse(self.結果(g)['性能継承成立'])
    def test_一問一形成を検査(self):g=群();g[0]['個票'][0]['問題束形成回数']=2;self.assertFalse(self.結果(g)['性能継承成立'])
    def test_空の資料群で成功しない(self):self.assertFalse(self.結果([])['性能継承成立'])

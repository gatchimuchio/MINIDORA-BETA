"""URL移動なしの実Chromium描画試験。set_contentの人工HTMLを使い、通信試験とは分ける。"""
from copy import deepcopy
import json
import os
import unittest

from minidora.ブラウザ閲覧 import 描画を観測, 表示操作を行う, 表を配列, _要素
from minidora.構造化文書 import 構造化文書を読む
from minidora.構造化文書操作 import 文書を処理
from minidora.製品版.抽出 import 情報抽出モジュール
from ブラウザ試験素材 import 画面HTML


@unittest.skipUnless(os.environ.get('MINIDORA_BROWSER_DOM_TEST')=='1','実ブラウザ描画試験は明示した別構成で実行')
class 実ブラウザ描画試験(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        from playwright.sync_api import sync_playwright
        cls.driver=sync_playwright().start()
        cls.browser=cls.driver.chromium.launch(executable_path=os.environ.get('MINIDORA_BROWSER_EXECUTABLE'),headless=True)
    @classmethod
    def tearDownClass(cls):
        cls.browser.close();cls.driver.stop()
    def setUp(self):
        self.文脈=self.browser.new_context(offline=True,service_workers='block')
        self.page=self.文脈.new_page()
    def tearDown(self):
        self.文脈.close()
    def load(self,html=None):
        self.page.set_content(画面HTML() if html is None else html)
        return 描画を観測(self.page)
    def test_JSクリックで表が生成される(self):
        before=self.load();self.assertNotIn('table',[r['ID'] for r in before['要素']])
        表示操作を行う(self.page,before,'expand','詳細表示')
        after=描画を観測(self.page)
        self.assertNotEqual(before['観測SHA256'],after['観測SHA256'])
        self.assertEqual(表を配列(_要素(after,'table')),[['番号','値'],['001','731']])
    def test_動的表を既存JSON処理と抽出へ渡す(self):
        before=self.load();表示操作を行う(self.page,before,'expand','詳細表示')
        資料=表を配列(_要素(描画を観測(self.page),'table'))
        r=構造化文書を読む(json.dumps(資料,ensure_ascii=False),'JSON')
        r=文書を処理(r,'JSON選択',{'位置':'/1/1'})
        r=文書を処理(r,'値取出',{'型':'文字列'})
        self.assertTrue(r.成立,r.保留理由)
        self.assertEqual(情報抽出モジュール().実行('数字',r.本文).本文,'731')
    def test_値変更が実描画へ到達(self):
        for n in (0,222,10007):
            self.page.set_content(画面HTML(n));before=描画を観測(self.page)
            表示操作を行う(self.page,before,'expand','詳細表示')
            self.assertEqual(表を配列(_要素(描画を観測(self.page),'table'))[1][1],str(n))
    def test_隠れた本文を可視取得に混ぜない(self):
        r=self.load();self.assertNotIn('未表示999',r['本文']);self.assertNotIn('hidden',[x['ID'] for x in r['要素']])
    def test_フォーム送信をボタン表示と誤認しない(self):
        r=self.load('<form><button id="x">送信</button></form>')
        with self.assertRaises(ValueError):表示操作を行う(self.page,r,'x','送信')
    def test_無効ボタンと期待表示違いを拒否(self):
        r=self.load('<button id="x" type="button" disabled>表示</button>')
        with self.assertRaises(ValueError):表示操作を行う(self.page,r,'x','表示')
        r=self.load()
        with self.assertRaises(ValueError):表示操作を行う(self.page,r,'expand','違う文')
    def test_古い観測からのクリックを拒否(self):
        r=self.load();self.page.evaluate("document.getElementById('heading').textContent='変更'")
        with self.assertRaises(Exception):表示操作を行う(self.page,r,'expand','詳細表示')
        self.assertNotIn('table',[x['ID'] for x in 描画を観測(self.page)['要素']])
    def test_重複IDで最初のボタンを選ばない(self):
        r=self.load('<button id="x" type="button">甲</button><button id="x" type="button">甲</button>')
        with self.assertRaises(ValueError):表示操作を行う(self.page,r,'x','甲')
    def test_結合セルを誤った矩形表にしない(self):
        r=self.load('<table id="t"><tr><td colspan="2">値</td></tr></table>')
        with self.assertRaises(ValueError):表を配列(_要素(r,'t'))
    def test_遅延JSの描画を実観測(self):
        self.page.set_content('<div id="x">準備中</div><script>setTimeout(()=>document.getElementById("x").textContent="完了731",40)</script>')
        self.page.get_by_text('完了731',exact=True).wait_for()
        self.assertEqual(_要素(描画を観測(self.page),'x')['本文'],'完了731')
    def test_対象外の図を本文理解と呼ばない(self):
        r=self.load('<div id="x">文字</div><canvas></canvas><svg></svg>')
        self.assertEqual(r['除外']['図'],2)
    def test_文字列のIDをコードとして実行しない(self):
        r=self.load()
        with self.assertRaises(ValueError):_要素(r,"x');window.hacked=true;//")
        self.assertIsNone(self.page.evaluate('window.hacked'))

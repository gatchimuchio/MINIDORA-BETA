"""実ChromiumのURL移動・要求遮断・JS取得・合成接続。CIでは必須実行し、依存不足をskipしない。"""
from dataclasses import asdict, replace
import json
import os
import unittest

from minidora.ブラウザ閲覧 import ブラウザ閲覧器, ブラウザ要求, ブラウザ工程, ブラウザ記録整合
from minidora.ブラウザ接続 import ブラウザ閲覧モジュール
from minidora.構造化文書接続 import 構造化文書能力群
from minidora.能力合成_局所接続 import 局所能力群
from minidora.能力合成 import 能力合成器, 合成計画, 合成工程, 素材参照
from minidora.製品版.型 import 能力結果
from ブラウザ試験素材 import 起点, 詳細, 数値, 供給器, 表要求


@unittest.skipUnless(os.environ.get('MINIDORA_BROWSER_NAV_TEST')=='1','ページ移動はブラウザ導入済みCI構成で実行')
class 実ブラウザ移動試験(unittest.TestCase):
    def run_browser(self,request=None,provider=None):
        return ブラウザ閲覧器(実行ファイル=os.environ.get('MINIDORA_BROWSER_EXECUTABLE'),試験供給器=provider or 供給器()).実行(request or 表要求(),外部読取許可=True)
    def test_表示クリックから動的表取得(self):
        p=供給器();r=self.run_browser(provider=p)
        self.assertTrue(r.成立,r.保留理由);self.assertTrue(ブラウザ記録整合(r))
        self.assertEqual(json.loads(r.本文)[1],['001','731']);self.assertEqual(p.calls,[起点])
        self.assertEqual(r.データ['供給区分'],'試験供給')
    def test_リンク移動して次ページの実本文を取得(self):
        p=供給器();req=replace(表要求(),工程=(ブラウザ工程('リンク移動','next','次ページ'),ブラウザ工程('本文取得','answer')))
        r=self.run_browser(req,p)
        self.assertTrue(r.成立,r.保留理由);self.assertEqual(r.本文,'値は731です。')
        self.assertEqual(r.データ['最終観測']['URL'],詳細);self.assertEqual(p.calls,[起点,詳細])
    def test_未許可リンクは移動前に拒否(self):
        p=供給器();req=replace(表要求(),許可URL=(起点,),工程=(ブラウザ工程('リンク移動','next','次ページ'),ブラウザ工程('本文取得','answer')))
        r=self.run_browser(req,p);self.assertFalse(r.成立);self.assertEqual(p.calls,[起点]);self.assertEqual(r.本文,'')
    def test_JSのGET取得が描画へ反映(self):
        html=f'<html><head><meta charset="utf-8"></head><body><div id="x">準備中</div><script>fetch("{数値}").then(r=>r.json()).then(d=>document.getElementById("x").textContent="値"+d.value)</script></body></html>'
        p=供給器(initial=html);req=replace(表要求(),工程=(ブラウザ工程('表示待機','x','値731'),ブラウザ工程('本文取得','x')))
        r=self.run_browser(req,p);self.assertTrue(r.成立,r.保留理由);self.assertEqual(r.本文,'値731');self.assertIn(数値,p.calls)
    def test_POSTを供給せず全体保留(self):
        html='<button id="x" type="button" onclick="fetch(\''+数値+'\',{method:\'POST\',body:\'secret\'})">表示</button><p id="ok">完了</p>'
        p=供給器(initial=html);req=replace(表要求(),工程=(ブラウザ工程('表示操作','x','表示'),ブラウザ工程('本文取得','ok')))
        r=self.run_browser(req,p);self.assertFalse(r.成立);self.assertNotIn(数値,p.calls);self.assertEqual(r.本文,'')
    def test_未許可の追加資源を供給しない(self):
        html='<div id="x">準備中</div><script>fetch("/未知").catch(()=>document.getElementById("x").textContent="失敗")</script>'
        p=供給器(initial=html);req=replace(表要求(),工程=(ブラウザ工程('表示待機','x','失敗'),ブラウザ工程('本文取得','x')))
        r=self.run_browser(req,p);self.assertFalse(r.成立);self.assertEqual(p.calls,[起点])
    def test_存在しない対象の待機は有限(self):
        req=replace(表要求(),待機ミリ秒=100,工程=(ブラウザ工程('表示待機','missing'),ブラウザ工程('本文取得','missing')))
        r=self.run_browser(req);self.assertFalse(r.成立);self.assertEqual(r.本文,'')
    def test_操作を除くと表取得は成立しない(self):
        req=replace(表要求(),工程=(ブラウザ工程('表取得','table'),))
        self.assertFalse(self.run_browser(req).成立)
    def test_入出力の値変更と記録改変(self):
        r=self.run_browser(provider=供給器(222));self.assertTrue(r.成立,r.保留理由)
        self.assertEqual(json.loads(r.本文)[1][1],'222')
        self.assertFalse(ブラウザ記録整合(replace(r,本文='999')))
    def test_ブラウザ表から既存文書処理と数値抽出まで(self):
        engine=ブラウザ閲覧器(実行ファイル=os.environ.get('MINIDORA_BROWSER_EXECUTABLE'),試験供給器=供給器())
        runner=能力合成器((ブラウザ閲覧モジュール(engine,外部読取許可=True).登録(),*構造化文書能力群(),*局所能力群()))
        plan=合成計画((合成工程('閲覧',('ブラウザ閲覧',),'i',(素材参照('入力','r'),)),
            合成工程('読取',('文書読取',),'i',(素材参照('工程','閲覧'),),'形式'),
            合成工程('選択',('文書操作',),'i',(素材参照('工程','読取'),),'位置'),
            合成工程('値',('文書操作',),'i',(素材参照('工程','選択'),),'取出'),
            合成工程('抽出',('情報抽出',),'i',(素材参照('工程','値'),),'抽出設定')),('抽出',))
        資料={'r':能力結果(True,'',データ={'要求':asdict(表要求())}),'i':能力結果(True,'指定した処理'),
              '形式':能力結果(True,'',データ={'形式':'JSON'}),
              '位置':能力結果(True,'',データ={'操作':'JSON選択','設定':{'位置':'/1/1'}}),
              '取出':能力結果(True,'',データ={'操作':'値取出','設定':{'型':'文字列'}}),
              '抽出設定':能力結果(True,'',データ={'種別':'数字'})}
        r=runner.実行(plan,資料,外部読取許可=True)
        self.assertTrue(r.成立,r.理由);self.assertEqual(r.出力[0][1].本文,'731');self.assertTrue(r.監査整合())
    def test_ページ描画値を実HDSへ渡す(self):
        from minidora.文脈要求 import 文脈付き要求セッション
        req=replace(表要求(),工程=(ブラウザ工程('リンク移動','next','次ページ'),ブラウザ工程('本文取得','answer')))
        material=self.run_browser(req);self.assertTrue(material.成立,material.保留理由)
        r=文脈付き要求セッション('browser-hds').応答('本文から数字を抽出して',{'本文':material})
        self.assertTrue(r.成立,r.理由);self.assertEqual(r.出力[0][1].本文,'731')

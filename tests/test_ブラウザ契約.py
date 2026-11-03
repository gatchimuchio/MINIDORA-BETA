"""通信前の許可・要求・返却契約。外部サイトやブラウザを使わず検査する。"""
from copy import deepcopy
from dataclasses import asdict, replace
from unittest.mock import patch
import unittest

from minidora.ブラウザ通信 import (正規URL, ブラウザ資源, 公開資源供給器, ブラウザ通信境界)
from minidora.ブラウザ閲覧 import ブラウザ閲覧器, ブラウザ要求, ブラウザ工程, ブラウザ記録整合, 表を配列
from minidora.ブラウザ接続 import ブラウザ閲覧モジュール, ブラウザ要求を復元
from minidora.能力合成 import 能力合成器, 合成計画, 合成工程, 素材参照
from minidora.多段解決 import 多段解決器
from minidora.製品版.型 import 能力結果
from minidora.製品版.能力契約 import 能力文脈
from ブラウザ試験素材 import 起点, 詳細, 数値, 供給器, 表要求


class 閲覧要求試験(unittest.TestCase):
    def test_正常要求の往復(self):
        r=表要求(); r.検証(); self.assertEqual(ブラウザ要求を復元(asdict(r)),r)
    def test_不正URLと私設IPを拒否(self):
        for u in ('http://example.com/','https://127.0.0.1/','https://[::1]/','https://x@y/','https://example.com:81/', 'https://example.com/\n','file:///tmp/a','javascript:alert(1)','https://example.com/#a','https://example.com'):
            with self.subTest(u=u),self.assertRaises(ValueError): 正規URL(u)
    def test_未登録開始URLを拒否(self):
        with self.assertRaises(ValueError): replace(表要求(),開始URL='https://browser-test.example/unknown').検証()
    def test_異なる生成元と重複URLを拒否(self):
        for urls in ((起点,起点),(起点,'https://other.example/a'),[],()):
            with self.assertRaises(ValueError): ブラウザ通信境界(urls)
    def test_未知操作と送信を拒否(self):
        for op in ('入力','購入','フォーム送信','スクリプト評価'):
            with self.assertRaises(ValueError): replace(表要求(),工程=(ブラウザ工程(op,'x'),)).検証()
    def test_最後に取得を要求(self):
        with self.assertRaises(ValueError): replace(表要求(),工程=(ブラウザ工程('表示待機','x'),)).検証()
    def test_操作の期待表示を必須にする(self):
        for op in ('表示操作','リンク移動'):
            with self.assertRaises(ValueError): replace(表要求(),工程=(ブラウザ工程(op,'x'),ブラウザ工程('本文取得','x'))).検証()
    def test_不正型と予算と工程上限(self):
        for r in (replace(表要求(),待機ミリ秒=True),replace(表要求(),待機ミリ秒=10001),replace(表要求(),工程=[]),replace(表要求(),工程=表要求().工程*5)):
            with self.assertRaises(ValueError):r.検証()
    def test_余剰フィールドを捨てない(self):
        raw=asdict(表要求());raw['自動補完']=True
        with self.assertRaises(ValueError):ブラウザ要求を復元(raw)
    def test_未許可はブラウザ導入前に停止(self):
        r=ブラウザ閲覧器().実行(表要求())
        self.assertFalse(r.成立);self.assertEqual(r.本文,'')
    def test_停止と停止判定故障(self):
        for f in (lambda:True,lambda:'false',lambda:1/0):
            r=ブラウザ閲覧器().実行(表要求(),外部読取許可=True,停止要求=f)
            self.assertFalse(r.成立);self.assertEqual(r.データ['資源記録'],[])
    def test_通常会話は閲覧要求にしない(self):
        モジュール=ブラウザ閲覧モジュール(ブラウザ閲覧器());c=能力文脈('全部見て','s')
        self.assertEqual(モジュール.判定(c),0);self.assertFalse(モジュール.実行(c).成立)
    def test_外部読取登録を純粋探索へ混ぜない(self):
        reg=ブラウザ閲覧モジュール(ブラウザ閲覧器()).登録()
        self.assertTrue(reg.外部読取)
        with self.assertRaises(ValueError):多段解決器((reg,),純粋作用確認=True)
    def test_合成器の外部許可も必要(self):
        reg=ブラウザ閲覧モジュール(ブラウザ閲覧器(),外部読取許可=True).登録()
        p=合成計画((合成工程('閲覧',('ブラウザ閲覧',),'i',(素材参照('入力','r'),)),),('閲覧',))
        r=能力合成器((reg,)).実行(p,{'i':能力結果(True,'閲覧'),'r':能力結果(True,'',データ={'要求':asdict(表要求())})})
        self.assertFalse(r.成立);self.assertEqual(r.実行数,0)
    def test_通常結果を観測記録へ昇格しない(self):
        for r in (None,能力結果(True,'731'),能力結果(False,'')):
            self.assertFalse(ブラウザ記録整合(r))


class 閲覧通信試験(unittest.TestCase):
    def setUp(self):
        self.provider=供給器();self.n=ブラウザ通信境界((起点,詳細,数値),self.provider);self.n.予定文書=起点
    def test_許可された文書を新規取得(self):
        r=self.n.応答(起点,'GET','document',主文書=True)
        self.assertEqual(r.URL,起点);self.assertEqual(self.provider.calls,[起点]);self.assertEqual(self.n.履歴[0]['状態'],'供給')
    def test_許可されたJSON要求(self):
        self.assertIn(b'731',self.n.応答(数値,'GET','fetch').本体)
    def test_POSTと未許可GETを供給前に遮断(self):
        for url,method in ((起点,'POST'),(起点+'?secret=x','GET')):
            n=ブラウザ通信境界((起点,),self.provider)
            with self.assertRaises(ValueError):n.応答(url,method,'fetch')
        self.assertEqual(self.provider.calls,[])
    def test_子フレームと自動遷移を拒否(self):
        for url,main in ((起点,False),(詳細,True)):
            n=ブラウザ通信境界((起点,詳細),self.provider);n.予定文書=起点
            with self.assertRaises(ValueError):n.応答(url,'GET','document',主文書=main)
    def test_未知の要求種別を拒否(self):
        for kind in ('image','font','websocket','worker'):
            n=ブラウザ通信境界((起点,),self.provider)
            with self.assertRaises(ValueError):n.応答(起点,'GET',kind)
    def test_一度の拒否を別要求で隠さない(self):
        with self.assertRaises(ValueError):self.n.応答(起点,'POST','fetch')
        with self.assertRaises(ValueError):self.n.応答(起点,'GET','document',主文書=True)
        self.assertEqual(self.provider.calls,[])
    def test_非UTF8と不正MIMEと巨大応答(self):
        for mime,body in (('text/html',b'\xff'),('application/pdf',b'pdf'),('text/html; charset=cp932',b'x'),('text/html',b'x'*1000001)):
            n=ブラウザ通信境界((起点,),lambda u:ブラウザ資源(u,mime,body));n.予定文書=起点
            with self.assertRaises(ValueError):n.応答(起点,'GET','document',主文書=True)
    def test_供給器はURLをすり替えない(self):
        n=ブラウザ通信境界((起点,),lambda u:ブラウザ資源(詳細,'text/html',b'x'));n.予定文書=起点
        with self.assertRaises(ValueError):n.応答(起点,'GET','document',主文書=True)
    def test_資源数を勝手に剪定しない(self):
        for _ in range(64):self.n.応答(数値,'GET','fetch')
        with self.assertRaises(ValueError):self.n.応答(数値,'GET','fetch')
    def test_実取得適合器の既存HTTPS接続(self):
        with patch('minidora.ブラウザ通信.公開本文取得器._一回取得',return_value=(200,{'content-type':'text/html'},b'<p>x</p>')) as get:
            self.assertEqual(公開資源供給器()(起点).本体,b'<p>x</p>');get.assert_called_once_with(起点)
    def test_転送圧縮ダウンロードを追従しない(self):
        for status,headers in ((302,{'location':詳細}),(200,{'content-encoding':'gzip'}),(200,{'content-disposition':'attachment'})):
            with patch('minidora.ブラウザ通信.公開本文取得器._一回取得',return_value=(status,headers,b'a')):
                with self.assertRaises(ValueError):公開資源供給器()(起点)
    def test_供給器例外をURL外内容とともに漏らさない(self):
        def fail(url):raise RuntimeError('秘密本文')
        n=ブラウザ通信境界((起点,),fail);n.予定文書=起点
        with self.assertRaises(ValueError):n.応答(起点,'GET','document',主文書=True)
        self.assertNotIn('秘密本文',str(n.履歴));self.assertTrue(n.失敗)


class 表構造試験(unittest.TestCase):
    def test_結合と不揃いの表を拒否(self):
        cell={'本文':'1','行結合':1,'列結合':1,'見出し':False}
        for rows in ([[],[cell]],[[{**cell,'行結合':2}]],[[cell],[cell,cell]]):
            with self.assertRaises(ValueError):表を配列({'タグ':'table','表':rows})
    def test_先行零と文字列を保持(self):
        c=lambda s:{'本文':s,'行結合':1,'列結合':1,'見出し':False}
        self.assertEqual(表を配列({'タグ':'table','表':[[c('001'),c('731')]]}),[['001','731']])

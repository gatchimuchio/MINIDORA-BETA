"""横断読解を既存HDS・保存・依存失効・外部読取の境界で検査する。"""
from copy import deepcopy
from hashlib import sha256
from pathlib import Path
import gzip
import json
import unittest
from minidora.HDS運用 import HDS運用セッション, 旧保存を移行, 運用版
from minidora.HDS運用.値 import 開封, 封緘, 結果を復元
from minidora.HDS運用.関係内容 import 関係回答を検査
from test_HDS運用v7_資料横断 import 本文, 資料対, 質問
from test_HDS運用v5_運用接続 import 試験供給


class 横断運用試験(unittest.TestCase):
    def 会話(self, 資料=None, **設定):
        s = HDS運用セッション('横断運用試験', **{'手順形成': False, **設定})
        for 名, 値 in (資料 or 資料対()).items():
            self.assertTrue(s.資料を登録(名, 値.本文).成立)
        return s

    def 成功(self, s, 文=質問, **設定):
        r = s.応答(文, **設定)
        self.assertTrue(r.成立, r.本文)
        self.assertEqual(r.状態, 'COMMIT')
        return r

    def test_通常四工程が横断内容と原文を使う(self):
        r = self.成功(self.会話())
        self.assertEqual({行['能力'] for 行 in r.追跡['能力試行']},
                         {'資料関係読解', '関係内容構成', '文章作成', '関係文章照合'})
        # 能力試行は記録キー順。時系列はHDSの作用履歴で検査する。
        self.assertEqual([行['作用ID'].split('/')[-1] for 行 in r.追跡['作用']
                          if 行['作用ID'].startswith('運用能力/')],
                         ['資料関係読解', '関係内容構成', '文章作成', '関係文章照合'])
        self.assertIn('横断判定：支持', r.本文)
        self.assertIn('設備Aは有効である', r.本文)
        self.assertIn('装置Bが有効ならば装置Bは稼働する', r.本文)
        self.assertIn('世界の対象同一性', r.本文)
        self.assertTrue(関係回答を検査(r.結果))

    def test_保存復元再表現で宣言と実導出を保持(self):
        s = self.会話(); r = self.成功(s)
        old = deepcopy(r.結果.データ['構造'])
        t = HDS運用セッション.復元(s.保存())
        再 = self.成功(t, '文章にして')
        self.assertEqual(再.結果.データ['構造'], old)
        self.assertEqual({行['能力'] for 行 in 再.追跡['能力試行']}, {'関係文章再表現'})
        self.assertEqual(開封(json.loads(t.保存()))['前回目的']['原文'], 質問)

    def test_対象ID変更で旧結論を失効し再計算(self):
        s = self.会話(); self.成功(s)
        self.assertTrue(s.資料を登録('観測', 本文('設備A', '設備Aは有効である。', 識別子='別設備'), 更新=True).成立)
        self.assertFalse(s.状態()['前回有効']); self.assertFalse(s.応答('文章にして').成立)
        r = self.成功(s, '再計算して')
        self.assertEqual(r.結果.データ['構造']['横断']['判定']['判定'], '未確定')
        self.assertEqual(r.結果.データ['構造']['横断']['使用資料'], [])

    def test_時点変更で旧証拠を流用しない(self):
        s = self.会話(); self.成功(s)
        s.資料を登録('観測', 本文('設備A', '設備Aは有効である。', 時点='点検後'), 更新=True)
        r = self.成功(s, '再計算して')
        self.assertEqual(r.結果.データ['構造']['横断']['判定']['判定'], '未確定')
        self.assertIn('不一致:時点', r.本文)

    def test_別資料追加で指定範囲を勝手に広げない(self):
        s = self.会話(); r = self.成功(s)
        old = r.結果.データ['構造']['横断']
        s.資料を登録('第三資料', 本文('機器C', '機器Cは稼働しない。'))
        self.assertTrue(s.状態()['前回有効'])
        再 = self.成功(s, '再計算して')
        self.assertEqual(再.結果.データ['構造']['横断'], old)

    def test_未確定の説明完了を支持へ読み替えない(self):
        s = self.会話(資料対(時点='点検後')); r = self.成功(s)
        self.assertIn('横断判定：未確定', r.本文)
        self.assertNotIn('横断判定：支持', r.本文)

    def test_停止と予算超過で原依頼を成功扱いしない(self):
        s = self.会話()
        self.assertFalse(s.応答(質問, 停止要求=lambda: True).成立)
        self.assertFalse(s.応答(質問 + '、100文字以内で').成立)
        self.assertFalse(s.状態()['前回有効'])

    def test_純粋手順形成後も現資料を再束縛(self):
        s = self.会話(手順形成=True); self.成功(s)
        self.assertTrue(開封(json.loads(s.保存()))['形成手順'])
        s.資料を登録('観測', 本文('設備A', '設備Aは有効である。', 識別子='別設備'), 更新=True)
        r = self.成功(s, '再計算して')
        self.assertEqual(r.結果.データ['構造']['横断']['判定']['判定'], '未確定')
        self.assertTrue(HDS運用セッション.復元(s.保存()).状態()['前回有効'])

    def test_二段の外部読取許可を省略しない(self):
        for 全体, 今回 in ((False, False), (True, False), (False, True)):
            with self.subTest(全体=全体, 今回=今回):
                b = 試験供給(); s = self.会話(外部読取許可=全体, 取得器=b.接続())
                self.assertFalse(s.応答(質問 + '、不足は公開資料で調べて', 外部読取許可=今回).成立)
                self.assertEqual(b.呼出, [])

    def test_横断で必要な支持が得られれば通信を省略(self):
        b = 試験供給(); s = self.会話(外部読取許可=True, 取得器=b.接続())
        r = self.成功(s, 質問 + '、不足は公開資料で調べて', 外部読取許可=True)
        self.assertEqual(len(r.追跡['能力試行']), 6)
        self.assertEqual(b.呼出, [])
        self.assertEqual(r.結果.データ['構造']['取得状態'], '不要')

    def test_取得本文が同じ宣言を含んでも勝手に接続しない(self):
        b = 試験供給(本文('装置B', '装置Bは有効である。'))
        s = self.会話(資料対('設備Aは待機する。'), 外部読取許可=True, 取得器=b.接続())
        r = self.成功(s, 質問 + '、不足は公開資料で調べて', 外部読取許可=True)
        x = r.結果.データ['構造']
        self.assertTrue(b.呼出); self.assertTrue(x['公開資料群'])
        self.assertEqual(x['横断']['判定']['判定'], '未確定')
        self.assertEqual(set(x['横断']['宣言']), {'観測', '規則'})
        self.assertEqual(b.呼出[0], ('検索', '装置Bは稼働する'))
        数 = len(b.呼出)
        t = HDS運用セッション.復元(s.保存(), 外部読取許可=True, 取得器=b.接続())
        self.成功(t, '文章にして'); self.assertEqual(len(b.呼出), 数)

    def test_照会資料や出典改変は再封緘しても復元拒否(self):
        s = self.会話(); self.成功(s)
        raw = 開封(json.loads(s.保存()))
        raw['前回目的']['要求']['照会資料'] = '観測'
        with self.assertRaises(ValueError): HDS運用セッション.復元(json.dumps(封緘(raw), ensure_ascii=False))


class v6保存移行試験(unittest.TestCase):
    根 = Path(__file__).parent / '資料' / 'HDS旧保存'

    def 原本(self):
        return gzip.decompress((self.根 / 'v6_関係回答.json.gz').read_bytes()).decode()

    def test_固定未変更v6の実保存指紋を検査(self):
        m = json.loads((self.根 / '由来_v6.json').read_text(encoding='utf-8'))
        self.assertEqual(m['commit'], '574f22937b093985486d23c30c829d2b25cf7182')
        self.assertEqual(sha256(self.原本().encode()).hexdigest(), m['原本SHA256'])
        self.assertEqual(sha256((self.根 / 'v6_関係回答.json.gz').read_bytes()).hexdigest(), m['圧縮SHA256'])

    def test_旧版を通常復元で黙って読まない(self):
        with self.assertRaisesRegex(ValueError, '版不一致'): HDS運用セッション.復元(self.原本())

    def test_旧原本履歴依存を保ち旧成果を採用しない(self):
        old = 開封(json.loads(self.原本()))
        s = 旧保存を移行(self.原本()); new = 開封(json.loads(s.保存()))
        self.assertEqual(new['版'], 運用版)
        self.assertEqual(new['移行履歴'][0]['原本'], self.原本())
        self.assertEqual(new['移行履歴'][0]['版'], 'HDS旧保存明示移行-v3')
        for 鍵 in ('資料', '旧資料', '前回目的', '前回依存', '発話', '経験'):
            self.assertEqual(new[鍵], old[鍵])
        self.assertIsNone(new['前回結果']); self.assertFalse(new['焦点有効'])
        self.assertFalse(new['形成手順'])

    def test_再計算でも通常要求を勝手に横断へ変えない(self):
        s = 旧保存を移行(self.原本()); r = s.応答('再計算して')
        self.assertTrue(r.成立, r.本文)
        self.assertNotIn('横断', r.結果.データ['構造'])
        HDS運用セッション.復元(s.保存())

    def test_旧版内の未来横断要求を拒否(self):
        raw = 開封(json.loads(self.原本()))
        raw['前回目的']['要求']['版'] = 'HDS資料横断要求-v1'
        raw['前回目的']['要求']['照会資料'] = '規則'
        with self.assertRaisesRegex(ValueError, '当時に存在しない'):
            旧保存を移行(json.dumps(封緘(raw), ensure_ascii=False))

    def test_目録偽装と未許可の権限獲得を拒否(self):
        for 鍵, 値 in (('目録', '0' * 64), ('外部読取許可', True)):
            raw = 開封(json.loads(self.原本())); raw[鍵] = 値
            with self.subTest(鍵=鍵), self.assertRaises(ValueError):
                旧保存を移行(json.dumps(封緘(raw), ensure_ascii=False))


if __name__ == '__main__': unittest.main()

"""固定v5実保存からv6へ明示移行する。旧関係成果は原本に保持し再計算する。"""
from hashlib import sha256
from pathlib import Path
import gzip, json, unittest
from minidora.HDS運用 import HDS運用セッション, 旧保存を移行, 運用版
from minidora.HDS運用.値 import 開封

根=Path(__file__).parent/'資料'/'HDS旧保存'

def 原本(): return gzip.decompress((根/'v5_関係回答.json.gz').read_bytes()).decode()


class 旧v5移行試験(unittest.TestCase):
    def test_固定main由来とハッシュを照合(self):
        meta=json.loads((根/'由来_v5.json').read_text(encoding='utf-8'))
        self.assertEqual(meta['commit'],'49949048d6c20d991caf81025f97b9204505907c')
        self.assertEqual(meta['目録'],'8b09407ac5166e5295549a3a15eb3b690ef873ae0a7e8f324887fe13ad99d367')
        self.assertEqual(sha256(原本().encode()).hexdigest(),meta['原本SHA256'])
        self.assertEqual(sha256((根/'v5_関係回答.json.gz').read_bytes()).hexdigest(),meta['圧縮SHA256'])

    def test_v5通常復元は暗黙変換しない(self):
        with self.assertRaises(ValueError): HDS運用セッション.復元(原本())

    def test_v5関係成果は現行成果へ昇格せず原本に保持(self):
        old=開封(json.loads(原本())); self.assertIsNotNone(old['前回結果'])
        s=旧保存を移行(原本()); new=開封(json.loads(s.保存()))
        self.assertEqual(new['版'],運用版)
        self.assertIsNone(new['前回結果'])
        self.assertFalse(new['焦点有効'])
        self.assertEqual(new['移行履歴'][0]['旧版'],'HDS-MINIDORA-全体運用-v5')
        preserved=開封(json.loads(new['移行履歴'][0]['原本']))
        self.assertEqual(preserved,old)
        self.assertIsNotNone(preserved['前回結果'])

    def test_v5資料と依存履歴は保持する(self):
        old=開封(json.loads(原本())); s=旧保存を移行(原本()); new=開封(json.loads(s.保存()))
        for key in ('資料','旧資料','前回依存','発話','経験'):
            self.assertEqual(new[key],old[key],key)
        # 再計算用の目的だけは意味契約版を現行へ移す。旧目的そのものは移行原本に保持する。
        expected=old['前回目的'].copy(); expected['要求']=old['前回目的']['要求'].copy(); expected['要求']['版']='HDS関係説明要求-v2'
        self.assertEqual(new['前回目的'],expected)
        preserved=開封(json.loads(new['移行履歴'][0]['原本']))
        self.assertEqual(preserved['前回目的'],old['前回目的'])

    def test_明示再計算でv6関係意味へ戻る(self):
        s=旧保存を移行(原本())
        r=s.応答('再計算して')
        self.assertTrue(r.成立,r.本文)
        self.assertEqual(r.状態,'COMMIT')
        self.assertIn('支持',r.本文)
        HDS運用セッション.復元(s.保存())


if __name__=='__main__': unittest.main()

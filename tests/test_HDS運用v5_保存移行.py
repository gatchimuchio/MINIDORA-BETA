"""v4固定commitから得た実保存物。移行済み旧履歴と形成手順も保持する。"""
from copy import deepcopy
from hashlib import sha256
from pathlib import Path
from unittest.mock import patch
import gzip
import json
import unittest
from minidora.HDS運用 import HDS運用セッション, 旧保存を移行, 運用版
from minidora.HDS運用.値 import 開封, 封緘

資料根=Path(__file__).parent/'資料'/'HDS旧保存'

def 原本(名='v4'):
    return gzip.decompress((資料根/(名+'.json.gz')).read_bytes()).decode()

def 文字(状態):return json.dumps(封緘(状態),ensure_ascii=False)


class 旧v4移行試験(unittest.TestCase):
    def test_原本と移行済み原本の実ハッシュを照合(self):
        由来=json.loads((資料根/'由来_v4.json').read_text(encoding='utf-8'))
        self.assertEqual(sha256(原本().encode()).hexdigest(),由来['原本SHA256'])
        self.assertEqual(sha256((資料根/'v4.json.gz').read_bytes()).hexdigest(),由来['圧縮SHA256'])
        self.assertEqual(sha256(原本('v4_移行済').encode()).hexdigest(),由来['追加fixture']['原本SHA256'])
        self.assertEqual(由来['commit'],'09043dc222ecda52996e9dc0e53ad9c5a809c27e')

    def test_旧v4原本と履歴依存を保持し再採用しない(self):
        raw=原本();old=開封(json.loads(raw));s=旧保存を移行(raw);new=開封(json.loads(s.保存()))
        self.assertTrue(old['形成手順']);self.assertEqual(new['形成手順'],{})
        self.assertEqual(new['版'],運用版);self.assertEqual(new['移行履歴'][0]['原本'],raw)
        for key in ('資料','旧資料','前回結果','前回目的','前回依存','発話','経験'):
            self.assertEqual(new[key],old[key],key)
        self.assertFalse(new['焦点有効']);self.assertFalse(s.状態()['前回有効'])
        self.assertEqual(開封(json.loads(HDS運用セッション.復元(s.保存()).保存())),new)

    def test_v3からv4へ移行した元の履歴も原本内に保持(self):
        raw=原本('v4_移行済');old=開封(json.loads(raw));s=旧保存を移行(raw);new=開封(json.loads(s.保存()))
        self.assertEqual(old['移行履歴'][0]['旧版'],'HDS-MINIDORA-全体運用-v3')
        preserved=開封(json.loads(new['移行履歴'][0]['原本']))
        self.assertEqual(preserved['移行履歴'],old['移行履歴'])
        self.assertEqual(preserved,old)
        HDS運用セッション.復元(s.保存())

    def test_通常復元は旧v4を暗黙に変換しない(self):
        with self.assertRaises(ValueError):HDS運用セッション.復元(原本())

    def test_旧v4の独自目録と権限の獲得を拒否(self):
        old=開封(json.loads(原本()));old['目録']='0'*64
        with self.assertRaises(ValueError):旧保存を移行(文字(old))
        with self.assertRaisesRegex(ValueError,'権限'):旧保存を移行(原本(),外部読取許可=True)

    def test_入れ子の由来改変を外側再封緘でも拒否(self):
        old=開封(json.loads(原本('v4_移行済')))
        old['移行履歴'][0]['原本SHA256']='0'*64
        with self.assertRaises(ValueError):旧保存を移行(文字(old))


    def test_v4当時に存在しない移行記録版を拒否(self):
        old=開封(json.loads(原本('v4_移行済')))
        old['移行履歴'][0]['版']='HDS旧保存明示移行-v2'
        with self.assertRaisesRegex(ValueError,'版不一致'):旧保存を移行(文字(old))

    def test_v4当時に存在しない移行履歴を拒否(self):
        old=開封(json.loads(原本('v4_移行済')))
        old['移行履歴'][0]['旧版']='HDS-MINIDORA-全体運用-v4'
        with self.assertRaisesRegex(ValueError,'v4当時'):旧保存を移行(文字(old))

    def test_移行では旧能力や外部サービスを実行しない(self):
        with patch.object(HDS運用セッション,'応答',side_effect=AssertionError('実行禁止')):
            s=旧保存を移行(原本())
        self.assertFalse(s.状態()['前回有効'])

    def test_明示再計算で旧一般説明も新目録で再成立(self):
        s=旧保存を移行(原本());r=s.応答('再計算して')
        self.assertTrue(r.成立,r.本文);self.assertTrue(s.状態()['前回有効'])
        self.assertIn('装置A',r.本文)


if __name__=='__main__':unittest.main()

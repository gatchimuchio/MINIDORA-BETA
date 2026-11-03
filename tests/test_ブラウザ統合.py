"""専用ブラウザCIで共通統合入口から実Chromiumを動かす。"""
from dataclasses import asdict
import json
import os
import unittest
from minidora.統合実行 import 統合セッション
from minidora.ブラウザ閲覧 import ブラウザ閲覧器
from minidora.製品版.型 import 能力結果
from ブラウザ試験素材 import 表要求, 供給器
from test_統合実行 import 一工程


@unittest.skipUnless(os.environ.get('MINIDORA_BROWSER_NAV_TEST')=='1','専用ブラウザCIで実行')
class ブラウザ統合試験(unittest.TestCase):
    def test_共通入口の閲覧は繰返してもキャッシュしない(self):
        provider=供給器()
        browser=ブラウザ閲覧器(実行ファイル=os.environ.get('MINIDORA_BROWSER_EXECUTABLE'),試験供給器=provider)
        session=統合セッション('統合閲覧',外部読取許可=True,閲覧器=browser)
        for _ in range(2):
            r=session.計画実行(*一工程('ブラウザ閲覧',能力結果(True,'',データ={'要求':asdict(表要求())})),外部読取許可=True)
            self.assertTrue(r.成立,(r.理由,r.実行))
            self.assertEqual(json.loads(r.本文)[1][1],'731')
        self.assertEqual(len(provider.calls),2)
        self.assertNotIn('ブラウザ閲覧',session.再利用統計()['能力別'])

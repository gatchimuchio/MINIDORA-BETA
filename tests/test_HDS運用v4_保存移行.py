"""固定commitの本物の保存物を明示移行する。正解コーパスではない。"""
from copy import deepcopy
from hashlib import sha256
from pathlib import Path
import gzip
import json
import subprocess
import sys
import tempfile
import unittest
from minidora.HDS運用 import HDS運用セッション, 旧保存を移行
from minidora.HDS運用.値 import 開封, 封緘

資料根 = Path(__file__).parent / '資料' / 'HDS旧保存'


def 原本(版):
    return gzip.decompress((資料根 / (版 + '.json.gz')).read_bytes()).decode('utf-8')


def 保存文字(状態):
    return json.dumps(封緘(状態), ensure_ascii=False)


class 保存移行試験(unittest.TestCase):
    def test_三版の実原本と履歴依存を完全保持(self):
        由来 = json.loads((資料根 / '由来.json').read_text(encoding='utf-8'))
        for 版 in ('v1', 'v2', 'v3'):
            with self.subTest(版=版):
                文字 = 原本(版)
                旧 = 開封(json.loads(文字))
                会話 = 旧保存を移行(文字)
                新 = 開封(json.loads(会話.保存()))
                for 欄 in ('資料', '旧資料', '発話', '経験', '前回目的', '前回結果', '前回依存'):
                    self.assertEqual(新[欄], 旧[欄], 欄)
                self.assertEqual(新['移行履歴'][0]['原本'], 文字)
                self.assertEqual(新['移行履歴'][0]['原本SHA256'], sha256(文字.encode()).hexdigest())
                self.assertFalse(新['焦点有効']); self.assertFalse(会話.状態()['前回有効'])
                self.assertEqual(新['形成手順'], {})
                復元 = HDS運用セッション.復元(会話.保存())
                self.assertEqual(開封(json.loads(復元.保存())), 新)

    def test_失効した成果を明示再計算で新しい契約へ戻せる(self):
        会話 = 旧保存を移行(原本('v3'))
        応答 = 会話.応答('再計算して')
        self.assertTrue(応答.成立, 応答.本文)
        self.assertIn('5', 応答.本文)

    def test_v1の残存旧資料を原記録にも残す(self):
        会話 = 旧保存を移行(原本('v1'))
        旧本文 = 開封(json.loads(原本('v1')))['旧資料'][0]['本文']
        庫 = 開封(json.loads(会話.保存()))['原記録']['庫']['履歴']
        全文 = json.dumps(庫, ensure_ascii=False)
        self.assertIn(旧本文, 全文)
        self.assertIn('故障時は停止', 全文)

    def test_旧原記録成果は全て失効し履歴は削除しない(self):
        for 版 in ('v2', 'v3'):
            会話 = 旧保存を移行(原本(版))
            成果 = [行['識別子'] for 事象 in 開封(json.loads(原本(版)))['原記録']['庫']['履歴']
                    for 行 in 事象['追加'] if 行['種別'] == '成果']
            self.assertTrue(成果)
            for 識別子 in 成果:
                self.assertFalse(会話._原記録.庫.原記録(識別子)['現行'])

    def test_外部権限を移行データから獲得しない(self):
        with self.assertRaisesRegex(ValueError, '権限'):
            旧保存を移行(原本('v3'), 外部読取許可=True)

    def test_通常復元は旧版を暗黙変換しない(self):
        for 版 in ('v1', 'v2', 'v3'):
            with self.assertRaises(ValueError):
                HDS運用セッション.復元(原本(版))

    def test_未知版と独自目録は移行しない(self):
        for 変更 in ({'版':'HDS-MINIDORA-全体運用-v99'}, {'目録':'0'*64}, {'不明な欄': True}):
            旧 = 開封(json.loads(原本('v3'))); 旧.update(変更)
            with self.assertRaises(ValueError):
                旧保存を移行(保存文字(旧))

    def test_原本封緘と資料改変を検出(self):
        文字 = 原本('v1').replace('半年一回', '毎日一回')
        with self.assertRaises(ValueError):
            旧保存を移行(文字)
        for 版 in ('v1', 'v2', 'v3'):
            旧 = 開封(json.loads(原本(版))); 旧['旧資料'][0]['本文'] = '改変'
            with self.assertRaises(ValueError):
                旧保存を移行(保存文字(旧))

    def test_履歴の型と上限を検査(self):
        for 値 in ({}, ['不正'], [None]*33):
            旧 = 開封(json.loads(原本('v1'))); 旧['旧資料'] = 値
            with self.assertRaises(ValueError):
                旧保存を移行(保存文字(旧))

    def test_移行履歴の内部改変も復元で拒否(self):
        新 = 開封(json.loads(旧保存を移行(原本('v3')).保存()))
        for 欄, 値 in (('原本SHA256', '0'*64), ('方針', '再採用'), ('由来commit', '0'*40)):
            変更 = deepcopy(新); 変更['移行履歴'][0][欄] = 値
            with self.assertRaises(ValueError):
                HDS運用セッション.復元(保存文字(変更))

    def test_移行中は能力実行と外部取得をしない(self):
        from unittest.mock import patch
        with patch('minidora.HDS運用.セッション.HDS運用セッション.応答', side_effect=AssertionError('能力実行禁止')):
            会話 = 旧保存を移行(原本('v3'))
        self.assertFalse(会話.状態()['前回有効'])

    def test_CLIは原本を上書きせず非対話で移行する(self):
        with tempfile.TemporaryDirectory() as 一時:
            元, 先 = Path(一時) / '旧.json', Path(一時) / '新.json'
            元.write_text(原本('v3'), encoding='utf-8')
            指示 = [sys.executable, '-m', 'minidora.HDS運用', '--移行元', str(元), '--保存', str(先), '--json']
            実行 = subprocess.run(指示, capture_output=True, encoding='utf-8', timeout=30)
            self.assertEqual(実行.returncode, 0, 実行.stderr)
            self.assertEqual(json.loads(実行.stdout)['状態'], '移行完了')
            self.assertEqual(元.read_text(encoding='utf-8'), 原本('v3'))
            self.assertFalse(HDS運用セッション.復元(先.read_text(encoding='utf-8')).状態()['前回有効'])
            self.assertNotEqual(subprocess.run(指示, capture_output=True, timeout=30).returncode, 0)
            同一 = [sys.executable, '-m', 'minidora.HDS運用', '--移行元', str(元), '--保存', str(元)]
            self.assertNotEqual(subprocess.run(同一, capture_output=True, timeout=30).returncode, 0)
            self.assertEqual(元.read_text(encoding='utf-8'), 原本('v3'))


if __name__ == '__main__':
    unittest.main()

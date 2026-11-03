"""固定main v7の実保存を保持し、旧目的・成果をv8へ黙って採用しない。"""
from copy import deepcopy
from hashlib import sha256
from pathlib import Path
from unittest.mock import patch
import gzip
import json
import unittest

from minidora.HDS運用 import HDS運用セッション, 旧保存を移行, 運用版
from minidora.HDS運用.値 import 開封, 封緘, 指紋
from minidora.HDS運用.原記録 import 運用原記録, 原記録成果を復元
from minidora.HDS運用.能力 import 運用能力目録
from minidora.HDS運用.保存移行 import _v7純粋名, 旧成果履歴を照合

資料根 = Path(__file__).parent / '資料' / 'HDS旧保存'


def 原本():
    return gzip.decompress((資料根 / 'v7_一般会話.json.gz').read_bytes()).decode('utf-8')


def 文字列(状態):
    return json.dumps(封緘(状態), ensure_ascii=False, allow_nan=False)


class 旧v7明示移行試験(unittest.TestCase):
    def test_固定mainの実原本と圧縮物を照合(self):
        由来 = json.loads((資料根 / '由来_v7.json').read_text(encoding='utf-8'))
        self.assertEqual(由来['commit'], '4cdcf4dc9c9d2ee729a66eb1f1a935bee7bf9e2f')
        self.assertEqual(由来['目録'], '06936b56e38b7a5f85d6bd7787245a8896754fe62f1eaeab53e31b8454faa92b')
        self.assertEqual(sha256(原本().encode()).hexdigest(), 由来['原本SHA256'])
        self.assertEqual(由来['原本SHA256'], 'c700e9ab4621a8a023c947f4e09bc5f7f3a95a7942df70ba609cb515ea258534')
        self.assertEqual(sha256((資料根 / 'v7_一般会話.json.gz').read_bytes()).hexdigest(), 由来['圧縮SHA256'])
        self.assertEqual(len(原本().encode()), 由来['原本バイト数'])

    def test_通常復元は旧版と旧目録を暗黙変換しない(self):
        with self.assertRaisesRegex(ValueError, '形式・版不一致'):
            HDS運用セッション.復元(原本())
        旧 = 開封(json.loads(原本()))
        旧['版'] = 運用版
        with self.assertRaisesRegex(ValueError, '能力・意味契約'):
            HDS運用セッション.復元(文字列(旧))

    def test_原本の文字列と旧目的成果をそのまま履歴へ保持(self):
        旧 = 開封(json.loads(原本()))
        新 = 開封(json.loads(旧保存を移行(原本()).保存()))
        self.assertIsNotNone(旧['前回結果'])
        self.assertIsNotNone(旧['前回目的'])
        self.assertEqual(新['移行履歴'][0]['原本'], 原本())
        self.assertEqual(開封(json.loads(新['移行履歴'][0]['原本'])), 旧)
        self.assertEqual(新['移行履歴'][0]['旧版'], 'HDS-MINIDORA-全体運用-v7')
        self.assertEqual(新['移行履歴'][0]['旧目録'], 旧['目録'])
        self.assertEqual(新['移行履歴'][0]['版'], 'HDS旧保存明示移行-v4')
        self.assertEqual(新['移行履歴'][0]['原本SHA256'], sha256(原本().encode()).hexdigest())

    def test_前回結果と両目的を採用せず焦点と旧手順を失効(self):
        会話 = 旧保存を移行(原本())
        新 = 開封(json.loads(会話.保存()))
        self.assertEqual(新['版'], 運用版)
        for 名 in ('前回目的', '保留目的', '前回結果'):
            self.assertIsNone(新[名], 名)
        self.assertFalse(新['焦点有効'])
        self.assertFalse(会話.状態()['前回有効'])
        self.assertEqual(新['形成手順'], {})

    def test_資料発話依存知識と原記録の旧行を保持(self):
        旧 = 開封(json.loads(原本()))
        新 = 開封(json.loads(旧保存を移行(原本()).保存()))
        for 名 in ('資料', '旧資料', '前回依存', '発話', '経験', '知識資産',
                   '前回知識範囲', '手順形成', 'セッションID', '外部読取許可'):
            self.assertEqual(新[名], 旧[名], 名)
        旧履歴 = 旧['原記録']['庫']['履歴']
        self.assertEqual(新['原記録']['庫']['履歴'][:len(旧履歴)], 旧履歴)
        庫 = 運用原記録.復元(新['原記録'], 新['セッションID'])
        原成果 = [行 for 事象 in 旧履歴 for 行 in 事象['追加'] if 行['種別'] == '成果']
        self.assertTrue(原成果)
        for 行 in 原成果:
            self.assertFalse(庫.庫.原記録(行['識別子'])['現行'])
            self.assertEqual(原記録成果を復元(行), 旧['前回結果'])

    def test_移行と復元は能力や外部取得を実行しない(self):
        with patch.object(HDS運用セッション, '応答', side_effect=AssertionError('能力実行禁止')):
            会話 = 旧保存を移行(原本(), 取得器=lambda *_: self.fail('外部取得禁止'))
            HDS運用セッション.復元(会話.保存(), 取得器=lambda *_: self.fail('外部取得禁止'))
        self.assertFalse(会話.状態()['前回有効'])

    def test_v8保存再復元は同じ移行原本を保持(self):
        保存 = 旧保存を移行(原本()).保存()
        再保存 = HDS運用セッション.復元(保存).保存()
        self.assertEqual(開封(json.loads(再保存)), 開封(json.loads(保存)))

    def test_封緘された原本の直接改変を拒否(self):
        改変 = json.loads(原本())
        self.assertIn('SHA256', 改変)
        改変['SHA256'] = '0' * 64
        with self.assertRaises(ValueError):
            旧保存を移行(json.dumps(改変, ensure_ascii=False))

    def test_未知の旧目録を外側再封緘しても拒否(self):
        旧 = 開封(json.loads(原本()))
        旧['目録'] = '0' * 64
        with self.assertRaisesRegex(ValueError, '未確認の旧能力契約'):
            旧保存を移行(文字列(旧))

    def test_外部許可の変更と真偽値でない保存を拒否(self):
        with self.assertRaisesRegex(ValueError, '権限'):
            旧保存を移行(原本(), 外部読取許可=True)
        for 値 in (0, None, 'false'):
            with self.subTest(値=値):
                旧 = 開封(json.loads(原本()))
                旧['外部読取許可'] = 値
                with self.assertRaises(ValueError):
                    旧保存を移行(文字列(旧))

    def test_他所有者の原記録を移植しない(self):
        旧 = 開封(json.loads(原本()))
        旧['セッションID'] = '別所有者'
        with self.assertRaisesRegex(ValueError, '所有セッション'):
            旧保存を移行(文字列(旧))

    def test_前回結果の移植と要求の変更を拒否(self):
        for 名 in ('結果', '要求', '依存'):
            with self.subTest(名=名):
                旧 = 開封(json.loads(原本()))
                if 名 == '結果':
                    旧['前回結果']['内容']['本文'] += '改変'
                elif 名 == '要求':
                    旧['前回目的']['要求']['最大文字数'] += 1
                else:
                    旧['前回依存']['別資料'] = '0' * 64
                with self.assertRaisesRegex(ValueError, '原記録|原成果要求'):
                    旧保存を移行(文字列(旧))

    def test_成果のない有効焦点を拒否(self):
        旧 = 開封(json.loads(原本()))
        旧['前回結果'] = None
        with self.assertRaisesRegex(ValueError, '原成果がない'):
            旧保存を移行(文字列(旧))

    def test_移行履歴の原本印と由来と方針を再封緘しても拒否(self):
        保存 = 開封(json.loads(旧保存を移行(原本()).保存()))
        for 鍵, 値 in (('原本SHA256', '0' * 64), ('由来commit', '0' * 40),
                       ('旧目録', '0' * 64), ('方針', '旧成果をそのまま採用')):
            with self.subTest(鍵=鍵):
                改変 = deepcopy(保存)
                改変['移行履歴'][0][鍵] = 値
                with self.assertRaisesRegex(ValueError, '原本・方針'):
                    HDS運用セッション.復元(文字列(改変))

    def test_v7当時にない移行履歴や記録版を拒否(self):
        旧 = 開封(json.loads(原本()))
        履歴 = 開封(json.loads(旧保存を移行(原本()).保存()))['移行履歴']
        旧['移行履歴'] = 履歴
        with self.assertRaisesRegex(ValueError, 'v7当時'):
            旧保存を移行(文字列(旧))
        # 歴史上存在するv5原本の履歴へ、v7当時にまだない移行記録v4を付ける人工改変対照。
        元 = gzip.decompress((資料根 / 'v5_関係回答.json.gz').read_bytes()).decode()
        正常履歴 = 開封(json.loads(旧保存を移行(元).保存()))['移行履歴']
        正常履歴[0]['版'] = 'HDS旧保存明示移行-v4'
        旧['移行履歴'] = 正常履歴
        with self.assertRaisesRegex(ValueError, '欄・版不一致'):
            旧保存を移行(文字列(旧))

    def test_新しい能力名を旧手順へ後付けしない(self):
        # 既存v4手順への人工改変。旧v7実保存の成果を生成したと主張する試験ではない。
        元 = 開封(json.loads(gzip.decompress((資料根 / 'v4.json.gz').read_bytes()).decode()))
        手順 = deepcopy(next(iter(元['形成手順'].values())))
        旧 = 開封(json.loads(原本()))
        手順['目録'] = 旧['目録']
        手順['雛型']['目録'] = 旧['目録']
        手順['雛型']['計画']['工程'][0]['能力候補'] = ['将来追加能力']
        手順['SHA256'] = 指紋({鍵: 値 for 鍵, 値 in 手順.items() if 鍵 != 'SHA256'})
        旧['形成手順'] = {手順['目的鍵']: 手順}
        元初期化 = 運用能力目録.__init__

        def 新能力を持つ目録(目録, *引数, **指定):
            元初期化(目録, *引数, **指定)
            目録._純粋 = 目録._純粋 | {'将来追加能力'}

        self.assertNotIn('将来追加能力', _v7純粋名)
        with patch.object(運用能力目録, '__init__', 新能力を持つ目録):
            with self.assertRaisesRegex(ValueError, '未契約又は外部作用'):
                旧保存を移行(文字列(旧))

    def test_v7当時に成立した横断目的を原本へ保全し再採用しない(self):
        # 当時の公開要求契約の人工対照。旧保存の生成成果を作り直す試験ではない。
        旧 = 開封(json.loads(原本()))
        旧.update(前回結果=None, 焦点有効=False,
                  前回目的={'方式': '関係資料', '原文': '旧横断目的', '要求': {
                      '版': 'HDS資料横断要求-v1', '対象': ['資料甲', '資料乙'], '範囲': '指定資料',
                      '問い': '計器は稼働する', '形式': '箇条書き', '不足調査': False,
                      '最大文字数': 24000, '照会資料': '資料乙'}})
        新 = 開封(json.loads(旧保存を移行(文字列(旧)).保存()))
        self.assertIsNone(新['前回目的'])
        self.assertEqual(開封(json.loads(新['移行履歴'][0]['原本'])), 旧)
        for 名 in ('照会資料不足', '未来版'):
            with self.subTest(名=名):
                改変 = deepcopy(旧)
                if 名 == '照会資料不足':
                    改変['前回目的']['要求'].pop('照会資料')
                else:
                    改変['前回目的']['要求']['版'] = 'HDS資料横断要求-v2'
                with self.assertRaises(ValueError):
                    旧保存を移行(文字列(改変))


class 失効成果履歴照合試験(unittest.TestCase):
    def 旧状態(self):
        原 = gzip.decompress((資料根 / 'v3.json.gz').read_bytes()).decode()
        return 開封(json.loads(旧保存を移行(原).保存()))

    def test_既知旧成果と依存所有原本に一致する履歴だけ許す(self):
        状態 = self.旧状態()
        self.assertTrue(旧成果履歴を照合(状態, 状態['前回結果']))
        self.assertFalse(状態['焦点有効'])

    def test_新採用や未知原本や別所有者を例外にしない(self):
        正常 = self.旧状態()
        for 名 in ('焦点', '所有', '許可', '依存', '履歴なし', '未知目録', '原本印', '未来結果'):
            with self.subTest(名=名):
                状態 = deepcopy(正常)
                if 名 == '焦点':
                    状態['焦点有効'] = True
                elif 名 == '所有':
                    状態['セッションID'] = '他所有者'
                elif 名 == '許可':
                    状態['外部読取許可'] = True
                elif 名 == '依存':
                    状態['前回依存'] = {'別資料': '0' * 64}
                elif 名 == '履歴なし':
                    状態['移行履歴'] = []
                elif 名 == '未知目録':
                    状態['移行履歴'][0]['旧目録'] = '0' * 64
                elif 名 == '原本印':
                    状態['移行履歴'][0]['原本SHA256'] = '0' * 64
                else:
                    状態['前回結果']['内容']['本文'] += '未来成果'
                self.assertFalse(旧成果履歴を照合(状態, 状態['前回結果']))

    def test_原本内所有者の書換えも再封緘して免除しない(self):
        状態 = self.旧状態()
        旧 = 開封(json.loads(状態['移行履歴'][0]['原本']))
        旧['セッションID'] = '他所有者'
        原 = 文字列(旧)
        状態['セッションID'] = '他所有者'
        状態['移行履歴'][0]['原本'] = 原
        状態['移行履歴'][0]['原本SHA256'] = sha256(原.encode()).hexdigest()
        self.assertFalse(旧成果履歴を照合(状態, 状態['前回結果']))

    def test_v7原本は失効履歴の例外に使わない(self):
        状態 = 開封(json.loads(旧保存を移行(原本()).保存()))
        状態['前回結果'] = 開封(json.loads(原本()))['前回結果']
        self.assertFalse(旧成果履歴を照合(状態, 状態['前回結果']))

    def test_原本側の成果変更も旧原記録の一致なしに許さない(self):
        状態 = self.旧状態()
        旧 = 開封(json.loads(状態['移行履歴'][0]['原本']))
        旧['前回結果']['内容']['本文'] += '変更された成果'
        原 = 文字列(旧)
        状態['前回結果'] = 旧['前回結果']
        状態['移行履歴'][0]['原本'] = 原
        状態['移行履歴'][0]['原本SHA256'] = sha256(原.encode()).hexdigest()
        self.assertFalse(旧成果履歴を照合(状態, 状態['前回結果']))

    def test_従来のv3移行記録を変更せず原本に保持(self):
        # 歴史上成立する記録形式をv7保存へ添付する人工契約対照。生成結果を旧実保存と偽らない。
        元 = gzip.decompress((資料根 / 'v5_関係回答.json.gz').read_bytes()).decode()
        正常履歴 = 開封(json.loads(旧保存を移行(元).保存()))['移行履歴']
        self.assertEqual(正常履歴[0]['版'], 'HDS旧保存明示移行-v3')
        旧 = 開封(json.loads(原本()))
        旧['移行履歴'] = 正常履歴
        新 = 開封(json.loads(旧保存を移行(文字列(旧)).保存()))
        保存された旧 = 開封(json.loads(新['移行履歴'][0]['原本']))
        self.assertEqual(保存された旧['移行履歴'], 正常履歴)
        self.assertEqual(保存された旧, 旧)


if __name__ == '__main__':
    unittest.main()

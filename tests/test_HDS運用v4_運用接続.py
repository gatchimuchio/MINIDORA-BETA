"""資料文章v4のHDS・公開取得・保存・失効を通す。外部本文は人工試験入力。"""
from copy import deepcopy
from dataclasses import replace
from hashlib import sha256
import json
import unittest
from minidora.HDS運用 import HDS運用セッション
from minidora.HDS運用.値 import 開封, 封緘, 結果を復元
from minidora.HDS運用.一般接続 import 取得資料を復元
from minidora.公開本文取得 import 本文を復号
from minidora.知識取得 import 知識取得器, 知識取得要求
from minidora.製品版.型 import 参照資料


class 公開資料試験供給:
    def __init__(self, 本文='保守は年一回。ただし故障時は停止。'):
        self.本文, self.呼出 = 本文, []

    def 検索(self, 検索語, limit=5):
        self.呼出.append(('検索', 検索語))
        return (参照資料('a', '人工資料', '試験検索', 'https://example.test/a', 本文='保守は毎日。'),)

    def 取得(self, URL):
        self.呼出.append(('取得', URL))
        return 本文を復号(URL, (URL,), {'content-type': 'text/html; charset=utf-8'},
                     ('<p>' + self.本文 + '</p>').encode())

    def 接続(self):
        return 知識取得器(self, self)


class 運用接続試験(unittest.TestCase):
    def 会話(self, **設定):
        return HDS運用セッション('v4-接続試験', 手順形成=False, **設定)

    def 登録(self, 会話, 名前='A', 本文='# 保守\n年一回点検。\n# 注意\nただし故障時は停止。\n# 雑記\n色は青。\n', **設定):
        応答 = 会話.資料を登録(名前, 本文, **設定)
        self.assertTrue(応答.成立, 応答.本文)

    def 成功(self, 会話, 文='資料「A」の保守について要点を整理して', **設定):
        応答 = 会話.応答(文, **設定)
        self.assertTrue(応答.成立, 応答.本文)
        self.assertEqual(応答.状態, 'COMMIT')
        return 応答

    def test_同じHDSから既存生成を含む四作用が発火(self):
        会話 = self.会話(); self.登録(会話)
        応答 = self.成功(会話)
        self.assertEqual({行['能力'] for 行 in 応答.追跡['能力試行']},
                         {'資料意味選択', '資料内容構成', '文章作成', '資料文章照合'})
        self.assertIn('年一回', 応答.本文); self.assertIn('故障時', 応答.本文)
        self.assertNotIn('色は青', 応答.本文)
        self.assertEqual(会話.状態()['資料']['A']['本文'], '# 保守\n年一回点検。\n# 注意\nただし故障時は停止。\n# 雑記\n色は青。\n')

    def test_再表現後の保存復元でも要求と元資料を維持(self):
        会話 = self.会話(); self.登録(会話)
        self.成功(会話)
        前 = 開封(json.loads(会話.保存()))
        応答 = self.成功(会話, '文章にして')
        self.assertEqual({行['能力'] for 行 in 応答.追跡['能力試行']}, {'資料文章再表現'})
        復元 = HDS運用セッション.復元(会話.保存())
        後 = 開封(json.loads(復元.保存()))
        self.assertEqual(前['前回目的'], 後['前回目的'])
        self.assertEqual(前['資料'], 後['資料'])
        self.assertTrue(復元.状態()['前回有効'])
        self.成功(復元, '引用形式にして')

    def test_指定資料の更新は旧回答を失効し再表現を拒否(self):
        会話 = self.会話(); self.登録(会話); self.成功(会話)
        self.登録(会話, 本文='保守は月一回。', 更新=True)
        self.assertFalse(会話.状態()['前回有効'])
        self.assertFalse(会話.応答('文章にして').成立)
        新 = self.成功(会話, '再計算して')
        self.assertIn('月一回', 新.本文); self.assertNotIn('年一回', 新.本文)

    def test_無関係資料の追加は指定資料の回答を失効しない(self):
        会話 = self.会話(); self.登録(会話); self.成功(会話)
        self.登録(会話, 'B', '別の資料。')
        self.assertTrue(会話.状態()['前回有効'])
        self.成功(会話, '文章にして')

    def test_全資料の追加は回答と原記録成果を失効(self):
        会話 = self.会話(); self.登録(会話)
        self.成功(会話, '登録資料から保守について説明して')
        旧 = 開封(json.loads(会話.保存()))['原記録']['庫']['履歴']
        成果 = [行['識別子'] for 事象 in 旧 for 行 in 事象['追加'] if 行['種別'] == '成果' and 行['内容']['データ'].get('全資料範囲')]
        self.assertTrue(成果)
        self.登録(会話, 'B', '保守は月一回。')
        self.assertFalse(会話.状態()['前回有効'])
        self.assertTrue(all(not 会話._原記録.庫.原記録(識別子)['現行'] for 識別子 in 成果))
        self.assertFalse(会話.応答('文章にして').成立)
        self.assertIn('月一回', self.成功(会話, '再計算して').本文)

    def test_比較後の前者照応は提示順を使う(self):
        会話 = self.会話(); self.登録(会話, 'B', '保守は月一回。'); self.登録(会話)
        self.成功(会話, '資料「B」と「A」を保守と寿命の観点で比較し、確認事項を提案して')
        応答 = self.成功(会話, '前者の保守について説明して')
        self.assertIn('月一回', 応答.本文); self.assertNotIn('年一回', 応答.本文)

    def test_未消費の送信依頼や例外削除は保留(self):
        会話 = self.会話(); self.登録(会話)
        for 後 in ('、メールで送って', '、例外を省いて'):
            self.assertFalse(会話.応答('資料「A」の保守について要約して' + 後).成立)

    def test_予算や停止を成功へすり替えない(self):
        会話 = self.会話(); self.登録(会話)
        self.assertFalse(会話.応答('資料「A」の保守について100文字以内で、説明して').成立)
        self.assertFalse(会話.応答('資料「A」の保守について説明して', 停止要求=lambda: True).成立)

    def test_二段の外部許可が両方なければ通信しない(self):
        for セッション, 今回 in ((False, False), (True, False), (False, True)):
            with self.subTest(セッション=セッション, 今回=今回):
                供給 = 公開資料試験供給()
                会話 = self.会話(外部読取許可=セッション, 取得器=供給.接続())
                self.assertFalse(会話.応答('公開資料で保守について調べて、説明して', 外部読取許可=今回).成立)
                self.assertEqual(供給.呼出, [])

    def test_公開取得から内容生成と復元再表現まで通る(self):
        供給 = 公開資料試験供給()
        会話 = self.会話(外部読取許可=True, 取得器=供給.接続())
        応答 = self.成功(会話, '公開資料で保守について調べて、説明して', 外部読取許可=True)
        self.assertIn('年一回', 応答.本文); self.assertNotIn('毎日', 応答.本文)
        self.assertEqual({行['能力'] for 行 in 応答.追跡['能力試行']}, {'知識取得', '取得資料意味選択', '資料内容構成', '文章作成', '資料文章照合'})
        件数 = len(供給.呼出)
        復元 = HDS運用セッション.復元(会話.保存(), 外部読取許可=True, 取得器=供給.接続())
        self.成功(復元, '文章にして')
        self.assertEqual(len(供給.呼出), 件数)

    def test_取得した本文に主題がなければ不成立(self):
        供給 = 公開資料試験供給('別の話題だけ。')
        会話 = self.会話(外部読取許可=True, 取得器=供給.接続())
        self.assertFalse(会話.応答('公開資料で保守について調べて、説明して', 外部読取許可=True).成立)

    def test_取得記録と本文の改変を拒否(self):
        供給 = 公開資料試験供給()
        成果 = 供給.接続().実行(知識取得要求('保守', ('保守',)), 外部読取許可=True)
        self.assertTrue(取得資料を復元(成果))
        with self.assertRaises(ValueError):
            取得資料を復元(replace(成果, 参照=(replace(成果.参照[0], 本文='保守は毎日。'),)))
        データ = deepcopy(成果.データ); データ['要求']['検索語'] = '改変'
        with self.assertRaises(ValueError):
            取得資料を復元(replace(成果, データ=データ))

    def test_自己整合する別要求でも保存時の前回目的との不一致を拒否(self):
        会話 = self.会話(); self.登録(会話); self.成功(会話)
        状態 = 開封(json.loads(会話.保存()))
        状態['前回目的']['要求']['主題'] = ['雑記']
        with self.assertRaises(ValueError):
            HDS運用セッション.復元(json.dumps(封緘(状態), ensure_ascii=False))

    def test_純粋工程形成は回答暗記ではなく再実行契約(self):
        会話 = HDS運用セッション('v4-形成試験', 手順形成=True)
        self.登録(会話); self.成功(会話)
        状態 = 開封(json.loads(会話.保存()))
        self.assertEqual(len(状態['形成手順']), 1)
        self.登録(会話, 本文='保守は月一回。ただし故障時は停止。', 更新=True)
        応答 = self.成功(会話, '再計算して')
        self.assertIn('月一回', 応答.本文); self.assertNotIn('年一回', 応答.本文)
        self.assertTrue(HDS運用セッション.復元(会話.保存()).状態()['前回有効'])


if __name__ == '__main__':
    unittest.main()

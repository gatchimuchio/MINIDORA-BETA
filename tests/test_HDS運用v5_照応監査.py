"""未束縛照応の入れ子経路を検査する。語彙一致を対象同一性にしない。"""
import json
import unittest
from minidora.HDS運用 import HDS運用セッション
from minidora.HDS運用.関係言語 import 関係文を読む
from minidora.HDS運用.関係依頼 import 関係依頼を読む
from minidora.HDS運用.関係読解 import 関係資料を読む, 関係資料を検査
from minidora.HDS運用.関係内容 import 関係回答を検査
from minidora.HDS運用.値 import 開封, 結果を復元
from minidora.製品版.型 import 能力結果


指示語群 = ('それ', 'これ', '同装置', '同対象', '彼', '彼女', '同者',
            'あれ', '私', 'わたし', 'あなた', '前者', '後者')
質問 = '資料「手順」をもとに、装置Aが稼働するかどうか説明して'
不正な導出資料 = '否定（前者は有効である）。否定（前者は有効である）ならば装置Aは稼働する。'


class 入れ子照応試験(unittest.TestCase):
    def 拒否を確認(self, 書式):
        for 語 in 指示語群:
            with self.subTest(指示語=語), self.assertRaises(ValueError):
                関係文を読む(書式.format(語))

    def test_括弧内の未束縛主体を拒否(self):
        self.拒否を確認('（{}は有効である）')

    def test_否定内の未束縛主体を拒否(self):
        self.拒否を確認('否定（{}は有効である）')

    def test_関数第一項の未束縛参照を拒否(self):
        self.拒否を確認('接続({},装置A)')

    def test_関数第二項の未束縛参照を拒否(self):
        self.拒否を確認('接続(装置A,{})')

    def test_量化内の未束縛定数を拒否(self):
        self.拒否を確認('すべてのxについて（接続(x,{})）')

    def test_引用内容の未束縛参照を拒否(self):
        self.拒否を確認('作業者は「{}は有効である」と述べた')

    def test_帰属主体の未束縛参照を拒否(self):
        self.拒否を確認('{}は「装置Aは有効である」と述べた')

    def test_時点と様相内の未束縛参照を拒否(self):
        for 書式 in ('2025年では（接続(装置A,{})）', '可能性として（接続(装置A,{})）'):
            self.拒否を確認(書式)

    def test_複合条件内の未束縛参照を拒否(self):
        self.拒否を確認('否定（{}は有効である）ならば装置Aは稼働する')

    def test_入れ子照応を外側の先行主体へ勝手に結ばない(self):
        for 文 in ('否定（同装置は有効である）', '作業者は「同対象は有効である」と述べた'):
            with self.subTest(文=文), self.assertRaises(ValueError):
                関係文を読む(文, 先行主体='装置A')

    def test_明示的な量化変数と固有名は保持(self):
        for 文 in ('すべてのxについて（接続(x,装置A)）', '否定（装置Aは有効である）',
                  '作業者は「装置Aは有効である」と述べた', '接続(装置A,装置B)',
                  '（装置Aは有効である）', 'あるyについて（接続(y,装置A)）'):
            with self.subTest(文=文):
                式, _, _ = 関係文を読む(文)
                self.assertGreater(式.検査(), 0)

    def test_既存の局所照応は退行させない(self):
        for 語 in ('それ', 'これ', '同装置', '同対象'):
            with self.subTest(指示語=語):
                式, 主体, 履歴 = 関係文を読む(語 + 'は稼働します', 先行主体='装置A')
                self.assertEqual(式.項[0].名前, '装置A')
                self.assertEqual(主体, '装置A')
                self.assertIn('照応:' + 語 + '→装置A', 履歴)

    def test_指示語の部分文字列だけでは拒否しない(self):
        for 名 in ('前者装置', '同装置群', 'あなた装置'):
            with self.subTest(名=名):
                式, _, _ = 関係文を読む('接続(装置A,' + 名 + ')')
                self.assertEqual(式.項[1].名前, 名)

    def test_未束縛照応を使う推論を採用せず原文を残す(self):
        要求 = 関係依頼を読む({'原文': 質問})['要求']
        構造 = 関係資料を読む({'手順': 能力結果(True, 不正な導出資料)}, 要求)
        行 = 構造['資料群']['手順']
        self.assertEqual(行['判定']['判定'], '未確定')
        self.assertEqual(行['判定']['操作数'], 0)
        self.assertIsNone(行['判定']['支持'])
        self.assertEqual(行['読解'], [])
        self.assertEqual(len(行['残差']), 2)
        for 残差 in 行['残差']:
            a, b = 残差['範囲']
            self.assertEqual(残差['原文'], 不正な導出資料[a:b])
            self.assertIn('未束縛', 残差['理由'])
        self.assertTrue(関係資料を検査(構造))

    def test_未解釈照応の次文へ古い主体を持ち越さない(self):
        本文 = '装置Aは有効である。否定（同装置は有効である）。それは稼働する。'
        要求 = 関係依頼を読む({'原文': 質問})['要求']
        構造 = 関係資料を読む({'手順': 能力結果(True, 本文)}, 要求)
        行 = 構造['資料群']['手順']
        self.assertEqual(行['判定']['判定'], '未確定')
        self.assertEqual(len(行['読解']), 1)
        self.assertEqual(len(行['残差']), 2)

    def test_未束縛参照を含む問いは要求成立にしない(self):
        for 問い in ('否定（前者は有効である）', '接続(装置A,同対象)'):
            文 = '資料「手順」をもとに、' + 問い + 'かどうか説明して'
            with self.subTest(問い=問い), self.assertRaises(ValueError):
                関係依頼を読む({'原文': 文})

    def test_通常応答と保存復元にも未確定を保持する(self):
        セッション = HDS運用セッション('照応監査', 手順形成=False)
        self.assertTrue(セッション.資料を登録('手順', 不正な導出資料).成立)
        応答 = セッション.応答(質問)
        self.assertTrue(応答.成立, 応答.本文)
        self.assertEqual(応答.状態, 'COMMIT')
        self.assertIn('未確定', 応答.本文)
        self.assertNotIn('判定：支持', 応答.本文)
        復元 = HDS運用セッション.復元(セッション.保存())
        保存 = 開封(json.loads(復元.保存()))
        結果 = 結果を復元(保存['前回結果'])
        self.assertTrue(関係回答を検査(結果))
        self.assertEqual(結果.データ['構造']['資料群']['手順']['判定']['判定'], '未確定')
        self.assertTrue(復元.応答('文章にして').成立)


if __name__ == '__main__':
    unittest.main()

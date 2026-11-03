"""v6の必要条件・例外作用域を反例中心で検証する。未知を否定へ補完しない。"""
from copy import deepcopy
import unittest
from minidora.HDS運用 import HDS運用セッション
from minidora.HDS運用.関係依頼 import 関係依頼を読む
from minidora.HDS運用.関係言語 import 関係文を読む, 例外節を読む
from minidora.HDS運用.関係読解 import 関係資料を読む, 関係資料を検査
from minidora.HDS運用.関係内容 import 関係回答を検査
from minidora.HDS運用.値 import 結果を復元
from minidora.製品版.型 import 能力結果


def 要求(問い='装置Aが稼働する'):
    return 関係依頼を読む({'原文': f'資料「手順」をもとに、{問い}かどうか説明して'})['要求']


def 読解(本文, 問い='装置Aが稼働する'):
    return 関係資料を読む({'手順': 能力結果(True, 本文)}, 要求(問い))


def 判定(本文, 問い='装置Aが稼働する'):
    return 読解(本文, 問い)['資料群']['手順']['判定']['判定']


class 必要条件試験(unittest.TestCase):
    def test_ためには必要を逆方向の含意として保持(self):
        文='装置Aが稼働するためには装置Aが有効であることが必要である。装置Aは稼働する。'
        self.assertEqual(判定(文, '装置Aは有効である'), '支持')

    def test_必要条件から目的を逆算しない(self):
        文='装置Aが稼働するためには装置Aが有効であることが必要である。装置Aは有効である。'
        self.assertEqual(判定(文), '未確定')

    def test_必要条件という名詞形も同じ方向(self):
        文='装置Aが有効であることは装置Aが稼働するための必要条件である。装置Aは稼働する。'
        self.assertEqual(判定(文, '装置Aは有効である'), '支持')

    def test_場合に限りは必要条件であり十分条件にしない(self):
        規則='装置Aが有効な場合に限り、稼働する。'
        self.assertEqual(判定(規則+'装置Aは稼働する。', '装置Aは有効である'), '支持')
        self.assertEqual(判定(規則+'装置Aは有効である。'), '未確定')

    def test_場合のみも必要条件として扱う(self):
        文='装置Aが有効な場合のみ、稼働する。装置Aは稼働する。'
        self.assertEqual(判定(文, '装置Aは有効である'), '支持')

    def test_必要十分を片方向へ勝手に縮退しない(self):
        with self.assertRaisesRegex(ValueError, '必要十分'):
            関係文を読む('装置Aが有効であることは装置Aが稼働するための必要十分条件である')


class 例外試験(unittest.TestCase):
    本則='装置Aが有効ならば装置Aは稼働する。'
    例外='ただし装置Aが故障している場合を除く。'

    def test_例外不成立の明示があれば本則を適用(self):
        文='装置Aは有効である。装置Aは故障していない。'+self.本則+self.例外
        x=読解(文); 行=x['資料群']['手順']
        self.assertEqual(行['判定']['判定'],'支持')
        self.assertFalse(行['限定隔離'])
        self.assertIn('例外適用', str(行['読解']))
        self.assertTrue(関係資料を検査(x))

    def test_例外かどうか不明なら本則を適用しない(self):
        文='装置Aは有効である。'+self.本則+self.例外
        self.assertEqual(判定(文),'未確定')

    def test_例外成立だけでは本則の逆を作らない(self):
        文='装置Aは有効である。装置Aは故障している。'+self.本則+self.例外
        self.assertEqual(判定(文),'未確定')

    def test_例外時の明示帰結は別規則として使う(self):
        文=('装置Aは有効である。装置Aは故障している。'+self.本則+
             'ただし装置Aが故障している場合は装置Aは稼働しない。')
        self.assertEqual(判定(文),'反証')

    def test_本則側と例外側の根拠が両方あれば矛盾を隠さない(self):
        文=('装置Aは有効である。装置Aは故障している。装置Aは故障していない。'+self.本則+
             'ただし装置Aが故障している場合は装置Aは稼働しない。')
        self.assertEqual(判定(文),'矛盾')

    def test_同一構成句のただしも直前本則に結ぶ(self):
        文=('装置Aは有効である。装置Aは故障していない。'
             '装置Aが有効ならば装置Aは稼働する、ただし装置Aが故障している場合を除く。')
        self.assertEqual(判定(文),'支持')

    def test_曖昧な例外は直前記載だけ隔離(self):
        文='装置Bは稼働する。装置Aは稼働する。例外がある。'
        xb=読解(文,'装置Bは稼働する')['資料群']['手順']
        xa=読解(文,'装置Aは稼働する')['資料群']['手順']
        self.assertEqual(xb['判定']['判定'],'支持')
        self.assertEqual(xa['判定']['判定'],'未確定')
        self.assertTrue(xa['限定隔離'])
        self.assertTrue(any('推論採用を保留' in str(r['変換']) for r in xa['読解']))

    def test_未対応ただしは直前を先に事実化しない(self):
        x=読解('装置Aは稼働する。ただし故障時は除く。')['資料群']['手順']
        self.assertEqual(x['判定']['判定'],'未確定')
        self.assertTrue(x['限定隔離'])
        self.assertTrue(x['読解'])
        self.assertTrue(x['残差'])

    def test_例外節単独では採用しない(self):
        x=読解('ただし装置Aが故障している場合を除く。')['資料群']['手順']
        self.assertEqual(x['判定']['判定'],'未確定')
        self.assertTrue(x['限定隔離'])

    def test_例外条件の指示語を勝手に束縛しない(self):
        with self.assertRaises(ValueError):
            例外節を読む('ただし同装置が故障している場合を除く')


class 運用接続試験(unittest.TestCase):
    def test_通常HDS経路で例外根拠を回答し保存復元(self):
        s=HDS運用セッション('v6-例外',手順形成=False)
        文=('装置Aは有効である。装置Aは故障していない。'
           '装置Aが有効ならば装置Aは稼働する。ただし装置Aが故障している場合を除く。')
        self.assertTrue(s.資料を登録('手順',文).成立)
        r=s.応答('資料「手順」をもとに、装置Aが稼働するかどうか説明して')
        self.assertTrue(r.成立,r.本文); self.assertEqual(r.状態,'COMMIT')
        self.assertIn('支持',r.本文); self.assertIn('条件適用',r.本文)
        saved=s.保存(); restored=HDS運用セッション.復元(saved)
        rr=restored.応答('文章にして')
        self.assertTrue(rr.成立,rr.本文)
        self.assertTrue(関係回答を検査(rr.結果))

    def test_構造改変で例外ガードを外せない(self):
        x=読解('装置Aは有効である。'+例外試験.本則+例外試験.例外)
        y=deepcopy(x)
        row=next(r for r in y['資料群']['手順']['読解'] if r['識別子'].endswith(':本則'))
        row['式']['子'][0]=row['式']['子'][0]['子'][0]
        self.assertFalse(関係資料を検査(y))


if __name__=='__main__':
    unittest.main()

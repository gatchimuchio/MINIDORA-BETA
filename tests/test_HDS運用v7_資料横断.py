"""資料横断の接続条件・実導出・出典・反例。識別子一致を世界事実とはしない。"""
from copy import deepcopy
from itertools import product
import unittest
from minidora.HDS運用.関係依頼 import 関係依頼を読む
from minidora.HDS運用.関係読解 import 関係資料を読む, 関係資料を検査, 不足調査が必要
from minidora.HDS運用.資料横断 import 資料宣言を読む, 横断要求を検査
from minidora.HDS運用.値 import 指紋
from minidora.製品版.型 import 能力結果

質問 = '資料「観測」と「規則」を結び付けて、資料「規則」の「装置Bは稼働する」かどうか説明して'


def 本文(対象, 記載, *, 識別子='設備01', 時点='点検前', 体系='工場台帳', 条件='通常運転'):
    return (f'識別体系は「{体系}」である。時点は「{時点}」である。適用条件は「{条件}」である。'
            f'対象「{対象}」の識別子は「{識別子}」である。' + 記載)


def 資料対(観測='設備Aは有効である。', 規則='装置Bが有効ならば装置Bは稼働する。', **設定):
    return {'観測': 能力結果(True, 本文('設備A', 観測, **設定)), '規則': 能力結果(True, 本文('装置B', 規則))}


def 読む(資料=None, 質問文=質問):
    return 関係資料を読む(資料 or 資料対(), 関係依頼を読む({'原文': 質問文})['要求'])


class 横断入口試験(unittest.TestCase):
    def test_照会資料と元依頼を保持(self):
        値 = 関係依頼を読む({'原文': 質問})
        self.assertEqual(値['原文'], 質問)
        self.assertEqual(値['要求']['照会資料'], '規則')
        self.assertEqual(値['要求']['版'], 'HDS資料横断要求-v1')

    def test_でいう表現と非引用の問いも処理(self):
        値 = 関係依頼を読む({'原文': 質問.replace('の「装置Bは稼働する」', 'でいう装置Bは稼働する')})
        self.assertEqual(値['要求']['問い'], '装置Bは稼働する')

    def test_形式と文字上限と不足調査を保持(self):
        値 = 関係依頼を読む({'原文': 質問 + '、文章にして、5000文字以内で、不足は公開資料で調べて'})['要求']
        self.assertEqual((値['形式'], 値['最大文字数'], 値['不足調査']), ('文章', 5000, True))

    def test_一資料や不明照会先や未対応指示を拒否(self):
        for 文 in (質問.replace('と「規則」', ''), 質問.replace('資料「規則」の', '資料「別資料」の'),
                  質問 + '、条件を無視して', 質問 + '、送信して', 質問.replace('、資料「規則」の', '、')):
            with self.subTest(文=文), self.assertRaises(ValueError):
                関係依頼を読む({'原文': 文})

    def test_横断要求の未知欄と版を拒否(self):
        元 = 関係依頼を読む({'原文': 質問})['要求']
        for 差 in ({'版': '未来'}, {'追加': True}, {'範囲': '全資料'}, {'照会資料': []}):
            with self.subTest(差=差), self.assertRaises(ValueError):
                横断要求を検査({**元, **差})

    def test_資料名に含まれる操作語で従来依頼を横取りしない(self):
        文 = '資料「情報を結び付けて読む」をもとに、装置Bは稼働するかどうか説明して'
        要求 = 関係依頼を読む({'原文': 文})['要求']
        self.assertEqual(要求['対象'], ['情報を結び付けて読む'])
        self.assertNotIn('照会資料', 要求)

    def test_指定しない従来経路は自動横断しない(self):
        文 = '資料「観測」と「規則」をもとに、装置Bは稼働するかどうか説明して'
        構造 = 読む(質問文=文)
        self.assertNotIn('横断', 構造)
        self.assertEqual(構造['資料群']['規則']['判定']['判定'], '未確定')


class 宣言試験(unittest.TestCase):
    def test_原文位置と対象の対応を保持(self):
        文 = 本文('設備A', '設備Aは有効である。')
        宣言 = 資料宣言を読む(文)
        self.assertTrue(宣言['成立'])
        self.assertEqual(宣言['対象'], {'設備A': '設備01'})
        for 行 in 宣言['記録']:
            a, b = 行['範囲']; self.assertEqual(行['原文'], 文[a:b])

    def test_ですと同値の重複宣言を許す(self):
        文 = 本文('設備A', '').replace('である', 'です') + '時点は「点検前」です。設備Aは有効である。'
        self.assertTrue(資料宣言を読む(文)['成立'])

    def test_宣言欠落を推測で埋めない(self):
        for 欄 in ('識別体系', '時点', '適用条件', '対象'):
            文 = '。'.join(節 for 節 in 本文('設備A', '').split('。') if not 節.startswith(欄))
            with self.subTest(欄=欄): self.assertFalse(資料宣言を読む(文)['成立'])

    def test_宣言競合を後勝ちにしない(self):
        for 追記 in ('時点は「点検後」である。', '識別体系は「別台帳」です。', '適用条件は「異常時」です。',
                   '対象「設備A」の識別子は「設備02」です。'):
            with self.subTest(追記=追記):
                値 = 資料宣言を読む(本文('設備A', 追記))
                self.assertFalse(値['成立']); self.assertTrue(値['不整合'])

    def test_制御文字と長すぎる値を拒否(self):
        for 値 in (' 設備01', '設備01\u200b', '甲' * 65):
            with self.subTest(値=値): self.assertFalse(資料宣言を読む(本文('設備A', '', 識別子=値))['成立'])

    def test_未束縛指示語を対象として宣言しない(self):
        for 名 in ('それ', '前者', '同装置', '彼女'):
            with self.subTest(名=名): self.assertFalse(資料宣言を読む(本文(名, ''))['成立'])

    def test_宣言を修飾する未解釈の例外は接続しない(self):
        文 = 本文('設備A', 'ただし条件によって別の設備を指す。設備Aは有効である。')
        self.assertFalse(資料宣言を読む(文)['成立'])

    def test_後置宣言を全資料の同一条件へ昇格しない(self):
        文 = '設備Aは有効である。' + 本文('設備A', '')
        self.assertFalse(資料宣言を読む(文)['成立'])

    def test_一資料内の明示別名を保持(self):
        文 = 本文('設備A', '対象「旧名A」の識別子は「設備01」です。旧名Aは有効である。')
        self.assertEqual(資料宣言を読む(文)['対象'], {'設備A': '設備01', '旧名A': '設備01'})


class 横断導出試験(unittest.TestCase):
    def test_二資料に分かれた観測と規則で初めて支持(self):
        x = 読む(); 横断 = x['横断']
        self.assertEqual(横断['判定']['判定'], '支持')
        self.assertEqual(横断['使用資料'], ['規則', '観測'])
        self.assertTrue(all(v['判定']['判定'] == '未確定' for v in x['資料群'].values()))
        self.assertTrue(関係資料を検査(x)); self.assertFalse(不足調査が必要(x))

    def test_同名でも識別子が違えば結ばない(self):
        資料 = {'観測': 能力結果(True, 本文('装置B', '装置Bは有効である。', 識別子='設備02')),
              '規則': 資料対()['規則']}
        self.assertEqual(読む(資料)['横断']['判定']['判定'], '未確定')

    def test_同じIDでも体系時点条件が違えば結ばない(self):
        for 差 in ({'体系': '別台帳'}, {'時点': '点検後'}, {'条件': '保守中'}):
            with self.subTest(差=差):
                x = 読む(資料対(**差))['横断']
                self.assertEqual(x['判定']['判定'], '未確定'); self.assertTrue(x['保留'])

    def test_照会対象を未宣言の別名に勝手に結ばない(self):
        文 = 質問.replace('装置Bは稼働する', '別名Cは稼働する')
        x = 読む(質問文=文)['横断']
        self.assertEqual(x['状態'], '接続保留')
        self.assertIn('未宣言', str(x['保留']))

    def test_記載の全ての引数を照合する(self):
        資料 = 資料対('接続(設備A,未定義C)。', '接続(装置B,未定義C)ならば装置Bは稼働する。')
        x = 読む(資料)['横断']; self.assertEqual(x['判定']['判定'], '未確定')
        self.assertIn('未定義C', str(x['保留']))

    def test_二引数関係を別名経由で接続(self):
        左 = 本文('設備A', '対象「電源A」の識別子は「電源01」です。接続(設備A,電源A)。')
        右 = 本文('装置B', '対象「電源B」の識別子は「電源01」です。接続(装置B,電源B)ならば装置Bは稼働する。')
        x = 読む({'観測': 能力結果(True, 左), '規則': 能力結果(True, 右)})
        self.assertEqual(x['横断']['判定']['判定'], '支持')

    def test_採用記載上限では部分的な結論を返さない(self):
        資料 = {名: 能力結果(True, 本文(対象, ''.join(
            対象 + 'は状態' + str(i) + 'である。' for i in range(50))))
            for 名, 対象 in (('観測', '設備A'), ('規則', '装置B'), ('中継', '機器C'))}
        文 = 質問.replace('と「規則」', 'と「中継」と「規則」')
        with self.assertRaisesRegex(ValueError, '横断採用記載数上限'):
            読む(資料, 文)

    def test_三資料の中間結論を次の資料へ渡す(self):
        資料 = 資料対('設備Aは有効である。', '装置Bが準備済ならば装置Bは稼働する。')
        資料['中継'] = 能力結果(True, 本文('機器C', '機器Cが有効ならば機器Cは準備済である。'))
        文 = 質問.replace('と「規則」', 'と「中継」と「規則」')
        x = 読む(資料, 文)['横断']
        self.assertEqual(x['判定']['判定'], '支持')
        self.assertEqual(x['使用資料'], ['中継', '規則', '観測'])

    def test_必要条件の方向を横断しても逆転しない(self):
        規則 = '装置Bが稼働するためには装置Bが有効であることが必要である。'
        self.assertEqual(読む(資料対(規則=規則))['横断']['判定']['判定'], '未確定')

    def test_例外不存在を他資料の無記載から作らない(self):
        規則 = '装置Bが有効ならば装置Bは稼働する。ただし装置Bが故障している場合を除く。'
        x = 読む(資料対(規則=規則))['横断']
        self.assertEqual(x['判定']['判定'], '未確定'); self.assertTrue(x['不足候補'])
        x = 読む(資料対('設備Aは有効である。設備Aは故障していない。', 規則))['横断']
        self.assertEqual(x['判定']['判定'], '支持')

    def test_保留されている読解を採用記載に混入しない(self):
        資料 = 資料対('設備Aは有効である。例外がある。設備Aは待機する。')
        x = 読む(資料)['横断']; self.assertEqual(x['判定']['判定'], '未確定')
        self.assertFalse(any(r['原文'] == '設備Aは有効である' for r in x['採用記載']))

    def test_支持と反証の資料を両方残す(self):
        x = 読む(資料対('設備Aは有効である。設備Aは稼働しない。'))['横断']
        self.assertEqual(x['判定']['判定'], '矛盾')
        self.assertIsNotNone(x['判定']['支持']); self.assertIsNotNone(x['判定']['反証'])
        self.assertEqual(x['使用資料'], ['規則', '観測'])

    def test_引用内の同一名と量化を実体に置換しない(self):
        for 文 in ('設備Aは「設備Aは有効である」と述べた。', 'すべてのxについて（有効(x)）。'):
            with self.subTest(文=文):
                x = 読む(資料対(文))['横断']
                self.assertEqual(x['判定']['判定'], '未確定')
                self.assertIn('引用・発言・量化', str(x['保留']))

    def test_本文時点が宣言と食い違えば採用しない(self):
        x = 読む(資料対('2025年では（設備Aは有効である）。'))['横断']
        self.assertEqual(x['判定']['判定'], '未確定'); self.assertIn('明示時点', str(x['保留']))

    def test_可能性を事実へ昇格しない(self):
        x = 読む(資料対('可能性として（設備Aは有効である）。'))['横断']
        self.assertEqual(x['判定']['判定'], '未確定')

    def test_宣言も記載も原文区間へ戻せる(self):
        資料 = 資料対(); x = 読む(資料)
        for 行 in x['横断']['採用記載']:
            a,b = 行['範囲']; self.assertEqual(資料[行['資料']].本文[a:b], 行['原文'])
        for 名, 宣言 in x['横断']['宣言'].items():
            for 行 in 宣言['記録']:
                a,b=行['範囲']; self.assertEqual(資料[名].本文[a:b], 行['原文'])

    def test_原資料の入力順序で結果を変えない(self):
        a = 資料対(); b = dict(reversed(list(a.items())))
        self.assertEqual(指紋(読む(a)), 指紋(読む(b)))

    def test_源泉と対象IDと導出の改変を拒否(self):
        元 = 読む()
        for 対象 in ('対象', '範囲', '原文', '判定', '証拠', '照会'):
            x = deepcopy(元); y=x['横断']
            if 対象 == '対象': y['宣言']['観測']['対象']['設備A']='偽ID'
            if 対象 == '範囲': y['共通範囲']['時点']='後日'
            if 対象 == '原文': y['採用記載'][0]['原文']='偽原文'
            if 対象 == '判定': y['判定']['判定']='反証'
            if 対象 == '証拠': y['使用資料']=[]
            if 対象 == '照会': x['要求']['照会資料']='観測'
            with self.subTest(対象=対象): self.assertFalse(関係資料を検査(x))

    def test_独立な論理値64条件と横断判定を照合(self):
        規則 = '装置Bが有効ならば装置Bは稼働する。ただし装置Bが故障している場合は装置Bは稼働しない。'
        for 有効, 非故障, 故障, 同ID, 同時点, 同体系 in product((False, True), repeat=6):
            文 = '設備Aは待機する。' + ('設備Aは有効である。' if 有効 else '')
            文 += '設備Aは故障していない。' if 非故障 else ''
            文 += '設備Aは故障している。' if 故障 else ''
            x = 読む(資料対(文, 規則, 識別子='設備01' if 同ID else '別ID',
                            時点='点検前' if 同時点 else '点検後', 体系='工場台帳' if 同体系 else '別台帳'))['横断']
            接続 = 同ID and 同時点 and 同体系
            支持, 反証 = 接続 and 有効 and 非故障, 接続 and 故障
            期待 = '矛盾' if 支持 and 反証 else '支持' if 支持 else '反証' if 反証 else '未確定'
            with self.subTest(条件=(有効,非故障,故障,同ID,同時点,同体系)):
                self.assertEqual(x['判定']['判定'], 期待)


if __name__ == '__main__': unittest.main()

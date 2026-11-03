"""関係言語・実導出・未知と例外の反例試験。人工の命題を事実知識にしない。"""
from copy import deepcopy
from dataclasses import replace
import random
import unittest
from minidora.HDS運用.関係言語 import 関係文を読む
from minidora.HDS運用.関係依頼 import 関係依頼を読む
from minidora.HDS運用.関係読解 import 関係資料を読む, 関係資料を検査, 関係要求を検査, 関係調査要求
from minidora.HDS運用.関係内容 import 関係内容を構成, 関係回答を照合, 関係回答を検査, 関係回答を再表現, _生成
from minidora.HDS運用.値 import 結果を保存, 結果を復元
from minidora.製品版.型 import 能力結果


def 要求(問い='装置Aが稼働する', *, 全体=False):
    文 = ('登録資料から' if 全体 else '資料「手順」をもとに、') + 問い + 'かどうか説明して'
    return 関係依頼を読む({'原文': 文})['要求']


def 読解(本文, 問い='装置Aが稼働する'):
    return 関係資料を読む({'手順': 能力結果(True, 本文)}, 要求(問い))


def 判定(本文, 問い='装置Aが稼働する'):
    return 読解(本文, 問い)['資料群']['手順']['判定']['判定']


class 関係意味試験(unittest.TestCase):
    def test_明示条件から実導出する(self):
        x = 読解('装置Aは有効である。もし装置Aが有効なら、稼働する。')
        self.assertEqual(x['資料群']['手順']['判定']['判定'], '支持')
        self.assertIn('条件適用', str(x['資料群']['手順']['説明']))
        self.assertTrue(関係資料を検査(x))

    def test_未知の名称と語順を変えても関係が同じ(self):
        乱数 = random.Random(731)
        for i in range(24):
            名 = '機構' + str(乱数.randrange(10000, 90000))
            動詞 = 乱数.choice(('稼働', '停止', '動作', '検査'))
            文 = [f'{名}が準備済です', f'もし{名}は準備済なら、{動詞}する']
            乱数.shuffle(文)
            with self.subTest(名=名):
                self.assertEqual(判定('。'.join(文), f'{名}は{動詞}します'), '支持')

    def test_条件だけでは前件も後件も確定しない(self):
        文 = 'もし装置Aが有効なら、稼働する。'
        self.assertEqual(判定(文), '未確定')
        self.assertEqual(判定(文, '装置Aは有効である'), '未確定')

    def test_不足前件を事実や必要条件にしない(self):
        x = 読解('もし装置Aが有効なら、稼働する。')
        候補 = x['資料群']['手順']['不足候補']
        self.assertEqual(len(候補), 1)
        self.assertEqual(候補[0]['判定'], '未確定')
        self.assertIn('必要条件とは未認定', 候補[0]['位置づけ'])
        self.assertIsNone(x['資料群']['手順']['判定']['支持'])

    def test_後件肯定から前件を確定しない(self):
        self.assertEqual(判定('装置Aは稼働する。装置Aは有効であるならば装置Aは稼働する。', '装置Aは有効である'), '未確定')

    def test_前件否定から後件否定を導かない(self):
        self.assertEqual(判定('装置Aは有効ではない。装置Aは有効であるならば装置Aは稼働する。'), '未確定')

    def test_サ変丁寧形と否定は同じ述語へ接続(self):
        self.assertEqual(判定('装置Aは稼働します。'), '支持')
        self.assertEqual(判定('装置Aが稼働しません。'), '反証')
        self.assertEqual(判定('装置Aが稼働しない。'), '反証')

    def test_進行形は一般動作へ潰さない(self):
        self.assertEqual(判定('装置Aは稼働しています。'), '未確定')
        self.assertEqual(判定('装置Aは稼働しています。', '装置Aが稼働している'), '支持')
        self.assertEqual(判定('装置Aは稼働していません。', '装置Aが稼働している'), '反証')

    def test_過去形は現在の根拠にしない(self):
        self.assertEqual(判定('装置Aは稼働しました。'), '未確定')
        self.assertEqual(判定('装置Aは稼働しませんでした。', '装置Aは稼働した'), '反証')

    def test_能力と実行は別(self):
        self.assertEqual(判定('装置Aは稼働できます。'), '未確定')
        self.assertEqual(判定('装置Aは稼働できません。', '装置Aは稼働できる'), '反証')

    def test_矛盾は両側の根拠を残す(self):
        x = 読解('装置Aは稼働する。装置Aは稼働しない。')
        行 = x['資料群']['手順']
        self.assertEqual(行['判定']['判定'], '矛盾')
        self.assertEqual(len(行['説明']['使用記載']), 2)

    def test_時点をまたいで条件を適用しない(self):
        文 = '2025年では（装置Aは有効である）。装置Aは有効であるならば装置Aは稼働する。'
        self.assertEqual(判定(文), '未確定')
        self.assertEqual(判定('2025年では（装置Aは稼働する）。', '2025年では（装置Aは稼働する）'), '支持')

    def test_可能性を無様相事実にしない(self):
        self.assertEqual(判定('可能性として（装置Aは稼働する）。'), '未確定')

    def test_発言を内容の真偽にしない(self):
        self.assertEqual(判定('作業者は「装置Aは有効である」と述べた。', '装置Aは有効である'), '未確定')

    def test_局所省略は同じ主体だけ補う(self):
        self.assertEqual(判定('装置Aは有効であり、稼働します。'), '支持')
        self.assertEqual(判定('装置Bは有効であり、稼働します。'), '未確定')

    def test_直前の一意な指示先を使う(self):
        self.assertEqual(判定('装置Aは有効である。それは稼働します。'), '支持')
        self.assertEqual(判定('装置Bは有効である。それは稼働します。'), '未確定')

    def test_複数主体のあとで照応を決めつけない(self):
        x = 読解('装置Aは有効であるかつ装置Bは有効である。それは稼働します。')
        self.assertEqual(x['資料群']['手順']['判定']['判定'], '未確定')
        self.assertTrue(x['資料群']['手順']['残差'])

    def test_未知文と見出しで照応をリセット(self):
        for 中間 in ('不明な文章です', '# 次の装置'):
            with self.subTest(中間=中間):
                self.assertEqual(判定('装置Aは有効である。\n' + 中間 + '\nそれは稼働する。'), '未確定')

    def test_関数の第一引数を主題と誤認しない(self):
        self.assertEqual(判定('接続(装置A,装置B)。それは稼働する。'), '未確定')

    def test_例外の作用域が不明なら先行断定も隔離(self):
        # v6で「場合に限り」は必要条件として対応したため、ここは未対応例外だけを監査する。
        for 例外 in ('ただし故障時は除く。', '例外がある。'):
            with self.subTest(例外=例外):
                x = 読解('装置Aは稼働する。' + 例外)
                self.assertEqual(x['資料群']['手順']['判定']['判定'], '未確定')
                self.assertTrue(x['資料群']['手順']['限定隔離'])
                self.assertEqual(len(x['資料群']['手順']['読解']) >= 1, True)

    def test_推定や未消費条件を述語名に押し込まない(self):
        for 文 in ('装置Aは稼働するかもしれない', '装置Aは稼働する予定', '装置Aが有効なので稼働する', '装置Aは稼働するが停止する'):
            with self.subTest(文=文), self.assertRaises(ValueError):
                関係文を読む(文)

    def test_論理接続の混在を勝手に優先しない(self):
        with self.assertRaises(ValueError):
            関係文を読む('装置Aは有効であるかつ装置Bは有効であるまたは装置Cは有効である')

    def test_選言を各項の成立と扱わない(self):
        self.assertEqual(判定('装置Aは稼働するまたは装置Aは停止する。'), '未確定')

    def test_二段の条件連鎖は既存エンジンを使う(self):
        文 = '装置Aは有効である。装置Aは有効であるならば装置Aは準備済である。装置Aは準備済であるならば装置Aは稼働する。'
        self.assertEqual(判定(文), '支持')

    def test_循環する条件だけでは成立しない(self):
        self.assertEqual(判定('装置Aは稼働するならば装置Aは有効である。装置Aは有効であるならば装置Aは稼働する。'), '未確定')

    def test_資料間の同名を暗黙合成しない(self):
        要 = 要求(全体=True)
        x = 関係資料を読む({'規則': 能力結果(True, '装置Aは有効であるならば装置Aは稼働する。'),
                          '観測': 能力結果(True, '装置Aは有効である。')}, 要)
        self.assertEqual({行['判定']['判定'] for 行 in x['資料群'].values()}, {'未確定'})

    def test_原文区間と変換を保持(self):
        文 = '  装置Aが有効です。\nそれは稼働します。'
        x = 読解(文)
        for 行 in x['資料群']['手順']['読解']:
            a,b = 行['範囲']; self.assertEqual(行['原文'], 文[a:b]); self.assertTrue(行['変換'])
        self.assertEqual(結果を復元(x['資料群']['手順']['資料']).本文, 文)

    def test_資料順序で意味結果を変えない(self):
        a,b = 能力結果(True,'装置Aは稼働する。'), 能力結果(True,'装置Aは稼働しない。')
        self.assertEqual(関係資料を読む({'A':a,'B':b},要求(全体=True)), 関係資料を読む({'B':b,'A':a},要求(全体=True)))

    def test_資料語彙の一致だけで結論しない(self):
        self.assertEqual(判定('索引用語：装置A 稼働する。'), '未確定')

    def test_問いの対象をすり替えない(self):
        self.assertEqual(判定('装置Bは稼働する。'), '未確定')

    def test_構造内部の改変を拒否(self):
        x = 読解('装置Aは稼働する。')
        for 編集 in ('判定', '式', '原文', '要求', '範囲', '余計な欄'):
            y=deepcopy(x)
            if 編集=='判定':y['資料群']['手順']['判定']['判定']='反証'
            elif 編集=='式':y['問い式']['述語']='停止する'
            elif 編集=='原文':y['資料群']['手順']['読解'][0]['原文']='別原文'
            elif 編集=='要求':y['要求']['問い']='装置Bが稼働する'
            elif 編集=='範囲':y['資料群']['手順']['読解'][0]['範囲']=[1,2]
            else:y['余計な欄']=True
            with self.subTest(編集=編集):self.assertFalse(関係資料を検査(y))

    def test_関係質問の三表現は同じ問いを保持(self):
        文群 = ('資料「手順」をもとに、装置Aは稼働するかどうか説明して',
              '資料「手順」から「装置Aは稼働する」の根拠と条件を説明して',
              '資料「手順」ではなぜ装置Aは稼働するのか説明して')
        for 文 in 文群:self.assertEqual(関係依頼を読む({'原文':文})['要求']['問い'],'装置Aは稼働する')

    def test_追加条件を捨てず不成立にする(self):
        for 尾 in ('、メールで送って', '、例外を省いて', 'ということにして', '、文章にして、箇条書きにして'):
            with self.subTest(尾=尾), self.assertRaises(ValueError):
                関係依頼を読む({'原文':'資料「手順」をもとに装置Aは稼働するかどうか説明して'+尾})

    def test_旧一般依頼を奪わない(self):
        self.assertIsNone(関係依頼を読む({'原文':'資料「手順」をもとに説明して'}))
        self.assertIsNone(関係依頼を読む({'原文':'資料「手順」の保守について要約して'}))

    def test_要求の型件数予算を検査(self):
        for 差分 in ({'最大文字数':True},{'最大文字数':99},{'対象':['手順','手順']},{'不足調査':'yes'}, {'謎':1}):
            with self.subTest(差分=差分), self.assertRaises(ValueError):関係要求を検査({**要求(),**差分})

    def test_括弧や容量不正を拒否(self):
        for 文 in ('装置Aは「未閉じ', 'a'*16001, '装置Aは稼働する。'*65):
            with self.subTest(文=文[:20]), self.assertRaises(ValueError):読解(文)

    def test_予算を超えた説明を黙って切らない(self):
        x=読解('装置Aは稼働する。');x['要求']['最大文字数']=100
        with self.assertRaises(ValueError):_生成(x,'文章')

    def test_実導出の説明を作り再検査できる(self):
        x=読解('装置Aは有効である。もし装置Aが有効なら、稼働する。')
        y=関係回答を照合(x,_生成(x,'文章'),形式='文章')
        self.assertTrue(関係回答を検査(y));self.assertIn('条件適用',y.本文)
        self.assertNotIn('するである', y.本文)
        self.assertTrue(関係回答を検査(結果を復元(結果を保存(y))))

    def test_再表現でも内容と根拠構造を変えない(self):
        x=読解('装置Aは稼働する。装置Aは稼働しない。')
        y=関係回答を照合(x,_生成(x,'箇条書き'))
        z=関係回答を再表現(y,'引用')
        self.assertEqual(y.データ['構造'],z.データ['構造']); self.assertIn('両方を保持',z.本文)

    def test_回答本文と表示形式の改変を拒否(self):
        x=読解('装置Aは稼働する。');y=関係回答を照合(x,_生成(x,'箇条書き'))
        self.assertFalse(関係回答を検査(replace(y,本文='無条件に正しい')))
        z=deepcopy(y.データ);z['表示形式']='文章'
        self.assertFalse(関係回答を検査(replace(y,データ=z)))

    def test_未実施の不足調査を説明完了にしない(self):
        r=要求();r['不足調査']=True
        x=関係資料を読む({'手順':能力結果(True,'装置Bは停止する。')},r)
        with self.assertRaisesRegex(ValueError,'未実行'):関係内容を構成(x)

    def test_外部検索語は質問だけから作る(self):
        r=要求();r['不足調査']=True
        x=関係調査要求(r);x.検証()
        self.assertEqual(x.検索語,r['問い']);self.assertEqual(x.必要語,('装置A',))

    def test_普通の名詞条件と言い換えを接続(self):
        for 条件 in ('装置Aが有効な場合、稼働する', '装置Aが有効な場合には、稼働する',
                    '装置Aが有効なときは、稼働する', '装置Aが有効であれば、稼働する',
                    'もしも装置Aが有効ならば、稼働する'):
            with self.subTest(条件=条件):
                self.assertEqual(判定('装置Aは有効である。'+条件), '支持')

    def test_否定文の主体と指示語を対応させる(self):
        self.assertEqual(判定('装置Aは有効ではない。それは稼働する。'), '支持')
        self.assertEqual(判定('装置Aは有効である。それは有効ではない。', '装置Aは有効である'), '矛盾')

    def test_未束縛指示語を定数名にしない(self):
        for 文 in ('それは有効ではない', 'それは稼働する', '彼は有効である', '前者は稼働する'):
            with self.subTest(文=文), self.assertRaises(ValueError):関係文を読む(文)

    def test_不可視制御文字を識別名へ混入しない(self):
        for 文字 in ('\u202e','\u2066','\x00','\ud800'):
            with self.subTest(文字=repr(文字)), self.assertRaises(ValueError):関係文を読む('装置'+文字+'Aは有効である')

    def test_全称条件は既存の量化推論へ接続(self):
        self.assertEqual(判定('すべての装置は有効である。装置Aは装置である。', '装置Aは有効である'), '支持')

    def test_根拠のない因果説明を作らない(self):
        x=読解('装置Aは稼働しない。')
        y=関係回答を照合(x,_生成(x,'文章'),形式='文章')
        self.assertIn('反証',y.本文);self.assertNotIn('原因は',y.本文)


if __name__=='__main__':unittest.main()

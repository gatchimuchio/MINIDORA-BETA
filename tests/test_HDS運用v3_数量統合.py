"""自然文からHDS工程、条件訂正、形成、保存復元まで同じ実入口で確認する。"""
from copy import deepcopy
import json
from pathlib import Path
import os
import subprocess
import sys
from tempfile import TemporaryDirectory
import unittest
from minidora.HDS運用 import HDS運用セッション
from minidora.HDS運用.製品 import HDS製品ミニドラ
from minidora.HDS運用.値 import 封緘,結果を保存,結果を復元
from minidora.HDS運用.数量構造 import 数量を構成,数量を評価
from minidora.HDS運用.数量生成 import 数量コード仕様,数量コード実行,数量を照合,数量を説明
from minidora.コード生成 import 関数を生成
from minidora.製品版.型 import 能力結果


class 数量統合試験(unittest.TestCase):
    def 会話(self, 形成=False):
        会話=HDS運用セッション(手順形成=形成, 再利用=False)
        for 名,本文 in [('A','単価は12円/個。数量は3個。'),('B','単価は7円/個。数量は4個。'),('規則','費用は単価と数量の積。')]:
            self.assertTrue(会話.資料を登録(名,本文).成立)
        return 会話

    def 問い(self):
        return '資料「A」と資料「B」に資料「規則」を使って、費用を計算して、比較して、Pythonで検算し、詳しく説明して'

    def test_同じHDSが構文化からコードと説明を駆動(self):
        会話=self.会話();結果=会話.応答(self.問い())
        self.assertTrue(結果.成立,結果.本文)
        self.assertIn('36円',結果.本文);self.assertIn('28円',結果.本文)
        self.assertEqual({x['能力'] for x in 結果.追跡['能力試行']},
             {'数量文構文化','数量式評価','数量コード仕様','コード生成','数量コード実行','数量結果照合','数量回答構成'})
        self.assertEqual(set(会話._前回依存),{'A','B','規則'})

    def test_条件変更と再表現と原資料維持(self):
        会話=self.会話();self.assertTrue(会話.応答(self.問い()).成立)
        結果=会話.応答('資料「A」の数量を6個に変更して')
        self.assertTrue(結果.成立,結果.本文);self.assertIn('72円',結果.本文)
        self.assertIn('数量は3個',会話.状態()['資料']['A']['本文'])
        for 文 in ('短く説明して','表にして','コードを省いて'):
            結果=会話.応答(文);self.assertTrue(結果.成立,結果.本文)
            self.assertEqual([x['能力'] for x in 結果.追跡['能力試行']],['数量回答再表現'])
        self.assertNotIn('```python',結果.本文);self.assertIn('6個',結果.本文)
        結果=会話.応答('元の条件で再計算して');self.assertTrue(結果.成立,結果.本文);self.assertIn('36円',結果.本文)

    def test_不足を確認して明示返答から再開(self):
        会話=HDS運用セッション(手順形成=False)
        self.assertTrue(会話.資料を登録('A','単価は12円/個。費用は単価*数量。').成立)
        結果=会話.応答('資料「A」の費用を計算して')
        self.assertFalse(結果.成立);self.assertIn('数量不足',結果.本文)
        結果=会話.応答('数量は5個です')
        self.assertTrue(結果.成立,結果.本文);self.assertIn('60円',結果.本文)

    def test_曖昧な訂正と未知条件を勝手に補わない(self):
        会話=self.会話();self.assertTrue(会話.応答(self.問い()).成立)
        結果=会話.応答('数量を5個に変更して')
        self.assertFalse(結果.成立);self.assertIn('曖昧',結果.本文)
        self.assertFalse(会話.応答(self.問い()+'、メールで送信して').成立)

    def test_共通規則の更新で旧回答を失効(self):
        会話=self.会話();self.assertTrue(会話.応答(self.問い()).成立)
        self.assertTrue(会話.資料を登録('規則','費用は単価*数量*2。',更新=True).成立)
        self.assertFalse(会話.状態()['前回有効'])
        self.assertFalse(会話.応答('詳しく説明して').成立)
        後=会話.応答('再計算して');self.assertTrue(後.成立,後.本文);self.assertIn('72円',後.本文)

    def test_形成手順を値変更後に再利用し保存復元(self):
        会話=HDS運用セッション(手順形成=True,再利用=False)
        self.assertTrue(会話.資料を登録('A','個数は3個。結果は個数*2。').成立)
        前=会話.応答('資料「A」の結果を計算して')
        self.assertTrue(前.成立,前.本文);self.assertEqual(前.追跡['手順形成']['再現工程数'],7)
        self.assertTrue(会話.資料を登録('A','個数は4個。結果は個数*2。',更新=True).成立)
        後=会話.応答('再計算して')
        self.assertTrue(後.成立,後.本文);self.assertTrue(後.追跡['手順形成']['手順再利用'])
        self.assertIn('8個',後.本文)
        復元=HDS運用セッション.復元(会話.保存());新=復元.応答('再計算して')
        self.assertTrue(新.成立,新.本文);self.assertIn('8個',新.本文)

    def test_保存された別要求の自己整合回答を拒否(self):
        会話=HDS運用セッション(手順形成=False)
        会話.資料を登録('A','結果は3+2。');採用=会話.応答('資料「A」の結果を計算して')
        self.assertTrue(採用.成立,採用.本文)
        保存=json.loads(会話.保存())['内容']
        原=採用.結果.データ
        式=数量を構成({'A':'結果は9+9。'},原['構造']['要求'])
        計算=数量を評価(式);仕様=数量コード仕様(式);コード=関数を生成(**仕様)
        検算=数量コード実行(式,コード.本文);照合=数量を照合(式,計算,検算)
        本文,データ=数量を説明(式,計算,検算,照合)
        保存['前回結果']=結果を保存(能力結果(True,本文,参照=採用.結果.参照,データ=データ))
        with self.assertRaisesRegex(ValueError,'現行原資料'):
            HDS運用セッション.復元(json.dumps(封緘(保存),ensure_ascii=False))

    def test_保存目的だけを改変した再表現を拒否(self):
        会話=HDS運用セッション(手順形成=False)
        会話.資料を登録('A','結果は3+2。別量は9。')
        self.assertTrue(会話.応答('資料「A」の結果を計算して').成立)
        保存=json.loads(会話.保存())['内容']
        保存['前回目的']['要求']['数量']='別量'
        with self.assertRaisesRegex(ValueError,'前回目的'):
            HDS運用セッション.復元(json.dumps(封緘(保存),ensure_ascii=False))

    def test_取消と予算不足で成果や手順を採用しない(self):
        会話=self.会話()
        self.assertFalse(会話.応答(self.問い(),停止要求=lambda:True).成立)
        self.assertEqual(会話.状態()['形成手順数'],0)
        小=HDS運用セッション(最大作用回数=4,手順形成=False)
        self.assertFalse(小.応答('資料「A」の費用を計算して').成立)

    def test_製品経路とセッション分離(self):
        製品=HDS製品ミニドラ(手順形成=False)
        self.assertEqual(製品.応答('資料「A」を登録:原価は5円。売価は8円。利益は売価から原価を引いた値。',セッションID='甲').状態,'APPROVE')
        結果=製品.応答('資料「A」の利益を計算して',セッションID='甲')
        self.assertEqual(結果.状態,'APPROVE',結果.本文);self.assertIn('3円',結果.本文)
        self.assertTrue(製品.監査台帳.検証(結果.追跡ID))
        self.assertEqual(製品.応答('資料「A」の利益を計算して',セッションID='乙').状態,'SUSPEND')

    def test_CLIの資料投入と生成回答(self):
        with TemporaryDirectory() as 場所:
            資料=Path(場所)/'数量.txt';資料.write_text('速度は12km/時間。時間は2時間。距離は速度と時間の積。',encoding='utf-8')
            根=Path(__file__).resolve().parents[1]
            実行=subprocess.run([sys.executable,'-m','minidora.HDS運用','--資料',f'A={資料}','--形成なし','--json','資料「A」の距離を計算して'],
                cwd=根,env={**os.environ,'PYTHONPATH':str(根/'src')},capture_output=True,text=True,encoding='utf-8',timeout=60)
            self.assertEqual(実行.returncode,0,実行.stderr)
            self.assertIn('24km',json.loads(実行.stdout)['本文'])

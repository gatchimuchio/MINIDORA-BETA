from pathlib import Path
import importlib,sys,tempfile,json,time,os
from unittest import TestCase
from unittest.mock import patch
from GPQA実測管理 import _実行を管理,原子的保存,GPQAを測定,時間上限秒,性能継承下限

class GPQA実測管理試験(TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup);self.out=Path(self.tmp.name)/'out.json'
    def run_it(self,mode='直列',questions=('ok','ok','ok'),**kw):
        rows=[(i,q,('x','y','z','w'),'A' if i%2==0 else 'B')for i,q in enumerate(questions)]
        return _実行を管理(rows,self.out,方式=mode,条件={'資料集合CSV_SHA256':'TOY'},期限秒=kw.pop('期限秒',10),並列数=kw.pop('並列数',2),実行器名=kw.pop('実行器名','GPQA実測試験部品:記憶実行器'),**kw)
    def test_直列は同じ記憶を保持(self):
        r=self.run_it();self.assertEqual([x['記憶回数']for x in r['個票']],[1,2,3]);self.assertTrue(r['実測']['完走'])
    def test_並列は各問独立(self):
        r=self.run_it('並列');self.assertEqual([x['記憶回数']for x in r['個票']],[1,1,1]);self.assertTrue(r['実測']['完走'])
    def test_親だけで正解採点(self):
        r=self.run_it();self.assertEqual(r['指標']['正答'],2);self.assertTrue(all(x['選択入力長']==4 for x in r['個票']))
    def test_部分実行を全数正本にしない(self):
        r=self.run_it();self.assertFalse(r['実測']['全数完走']);self.assertFalse(r['実測']['正本採用可'])
    def test_期限で停止し完了個票と停止工程を保持(self):
        start=time.monotonic();r=self.run_it(questions=('ok','停止'),期限秒=2.0)
        self.assertEqual(r['測定状態'],'時間超過');self.assertLess(time.monotonic()-start,4)
        self.assertEqual(r['指標']['完了数'],1);self.assertEqual(r['実行中'][0]['工程'],'人工推論')
        self.assertEqual(json.loads(self.out.read_text())['個票'],r['個票'])
    def test_処理例外でも完了個票保存(self):
        r=self.run_it(questions=('ok','例外'));self.assertEqual(r['測定状態'],'処理失敗');self.assertEqual(r['指標']['完了数'],1)
    def test_ワーカー異常終了を保留成功にしない(self):
        r=self.run_it(questions=('終了',));self.assertEqual(r['測定状態'],'処理失敗');self.assertFalse(r['実測']['正本採用可'])
    def test_並列片側失敗で他問題の記録を捨てない(self):
        r=self.run_it('並列',questions=('ok','例外','ok'));self.assertEqual(r['指標']['完了数'],2);self.assertFalse(r['実測']['完走'])
    def test_候補外の結果を拒否(self):
        r=self.run_it(実行器名='GPQA実測試験部品:偽装実行器');self.assertEqual(r['測定状態'],'処理失敗')
    def test_二回形成は完走契約違反(self):
        r=self.run_it(実行器名='GPQA実測試験部品:重複形成器');self.assertFalse(r['実測']['完走'])
    def test_重複問題IDを事前拒否(self):
        with self.assertRaises(ValueError):_実行を管理([(0,'q',(),'A'),(0,'q',(),'A')],self.out,方式='直列',条件={},期限秒=1)
    def test_時間と下限を変更しない(self):
        self.assertEqual(時間上限秒,5400);self.assertEqual(性能継承下限,40)
    def test_90分超過の締切延長を拒否(self):
        with self.assertRaises(ValueError):GPQAを測定(self.out,方式='直列',期限epoch=time.time()+10000)
    def test_全数前提の直列を分割しない(self):
        with self.assertRaises(ValueError):GPQAを測定(self.out,方式='直列',件数=5)
    def test_不正な方式と並列数を拒否(self):
        for mode,n in [('x',1),('並列',0),('並列',41)]:
            with self.subTest(mode=mode,n=n),self.assertRaises(ValueError):_実行を管理([],self.out,方式=mode,条件={},期限秒=1,並列数=n)
    def test_期限切れなら推論を開始しない(self):
        r=self.run_it(期限秒=0);self.assertEqual(r['指標']['完了数'],0);self.assertEqual(r['測定状態'],'時間超過')
    def test_原子的保存で不正値を正本へ上書きしない(self):
        原子的保存(self.out,{'good':1})
        with self.assertRaises(ValueError):原子的保存(self.out,{'bad':float('nan')})
        self.assertEqual(json.loads(self.out.read_text()),{'good':1})
    def test_子プロセスも期限で終了する(self):
        if os.name!='posix':self.skipTest('POSIXプロセス群の試験')
        pidfile=Path(self.tmp.name)/'pid';r=self.run_it(questions=('子プロセス:'+str(pidfile),),期限秒=2.0)
        self.assertEqual(r['測定状態'],'時間超過');self.assertTrue(pidfile.exists())
        p=Path('/proc')/pidfile.read_text()/'stat'
        if p.exists():self.assertIn(p.read_text().split()[2],['Z','X'])

class GPQA並列入口接続試験(TestCase):
    def test_並列入口は新しい実測管理引数へ接続する(self):
        import GPQA並列性能継承評価 as 入口
        from types import SimpleNamespace
        呼出 = {}
        def 偽測定(出力, **kwargs):
            呼出['出力'] = 出力
            呼出.update(kwargs)
            return {'実測': {'完走': True}}
        引数 = SimpleNamespace(start_index=10, limit=5, out=Path('x.json'), started_epoch=100.0, deadline_epoch=5500.0)
        with patch.object(入口, 'GPQAを測定', side_effect=偽測定), patch.object(入口, '引数解析器') as 解析:
            解析.return_value.parse_args.return_value = 引数
            self.assertEqual(入口.main(), 0)
        self.assertEqual(呼出['方式'], '並列')
        self.assertEqual(呼出['開始番号'], 10)
        self.assertEqual(呼出['件数'], 5)
        self.assertEqual(呼出['期限epoch'], 5500.0)
        self.assertEqual(呼出['共通開始epoch'], 100.0)
        self.assertEqual(呼出['並列数'], 1)

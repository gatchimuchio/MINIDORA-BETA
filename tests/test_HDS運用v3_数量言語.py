"""数量言語の構文・原文位置・否定と未解釈境界。"""
from fractions import Fraction
import unittest
from minidora.HDS運用.数量言語 import 式を読む, 定義を読む, 単位を読む, 有理を読む
from minidora.HDS運用.数量構造 import 数量を構成, 数量を評価


def 要求(数量="結果", 対象=None, 規則=None, 操作=None):
    return {"対象":対象 or ["入力"], "規則":規則 or [], "数量":数量,
            "操作":操作 or ["計算"], "条件変更":[],
            "表示":{"詳細":False,"コード":False,"形式":"文章","読者":"一般"}}


class 数量言語試験(unittest.TestCase):
    def 評価(self, 文):
        return 数量を評価(数量を構成({"入力":文},要求()))["結果"][0]

    def test_日本語と記号の構成同値(self):
        for 式, 期待 in (("甲と乙の和",9),("甲と乙の差",3),("甲と乙の積",18),("甲と乙の商",2),
                         ("甲に乙を足した値",9),("甲から乙を引いた値",3),("甲を乙で割った値",2),
                         ("甲に乙を加える",9),("甲から乙を引く",3),("甲を乙で割る",2),
                         ("甲の2倍",12),("甲の25%",Fraction(3,2))):
            with self.subTest(式=式):
                self.assertEqual(Fraction(self.評価(f"甲は6。乙は3。結果は{式}。")['値']),期待)

    def test_括弧と四則優先(self):
        for 式,期待 in (("2+3*4",14),("(2+3)*4",20),("12/3/2",2),("12-(3-2)",11),
                        ("-3+2",-1),("2 * -3",-6),("-(3+2)",-5),("((甲と乙の積)+1)",19)):
            with self.subTest(式=式):
                self.assertEqual(Fraction(self.評価(f"甲は6。乙は3。結果は{式}。")['値']),期待)

    def test_全角記号でも原文位置保持(self):
        本文="  単価は１２０円/個です。\n 数量は３個です。結果は単価×数量です。"
        定義=定義を読む(本文)
        for 行 in 定義.values():
            self.assertEqual(本文[slice(*行['範囲'])],行['原文'])
        self.assertEqual(self.評価(本文)['値'],'360')

    def test_任意の数量名を資料から得る(self):
        文="輸送係数は7。稼働量は9。補整量は2。結果は輸送係数と稼働量の積に補整量を足した値。"
        self.assertEqual(self.評価(文)['値'],'65')

    def test_演算文字を含む数量名は引用で保護(self):
        文="加算用の値は7。結果は「加算用の値」+2。"
        self.assertEqual(self.評価(文)['値'],'9')

    def test_分数小数百分率を丸めない(self):
        for 式,期待 in (("0.1+0.2","3/10"),("1/3+1/6","1/2"),("12.5%","1/8"),(".25","1/4")):
            with self.subTest(式=式): self.assertEqual(self.評価("結果は"+式+"。")['値'],期待)

    def test_単位の積商を数量演算として扱う(self):
        結果=self.評価("単価は120円/個。数量は3個。結果は単価*数量。")
        self.assertEqual(結果['単位'],{'円':1});self.assertEqual(結果['値'],'360')
        self.assertEqual(self.評価("数量は3個。結果は120円/個*数量。")['値'],'360')

    def test_負量の単位を保持(self):
        self.assertEqual(self.評価("結果は-3円。")['単位'],{'円':1})

    def test_演算途中を切り捨てない(self):
        for 式 in ('2+','2 3','2**3','(2+3','2+3)',"__import__('os')",'2;print(1)'):
            with self.subTest(式=式),self.assertRaises((ValueError,RecursionError)):
                式を読む(式)

    def test_否定条件近似範囲を点値へ変えない(self):
        for 片 in ("3個ではない","3個ではありません","約3個","3個以下","3個未満","3個の場合",
                    "3個以上","少なくとも3個","3個かもしれない"):
            with self.subTest(片=片),self.assertRaises(ValueError): 定義を読む('数量は'+片+'。')

    def test_未知の文を捨てない(self):
        with self.assertRaises(ValueError): self.評価("結果は3。詳細は図を参照。")

    def test_重複定義を拒否(self):
        with self.assertRaises(ValueError): 定義を読む("結果は1。結果は2。")

    def test_構文規模に上限(self):
        for 文 in ('(' * 30+'1'+')'*30,'1+'*130+'1','+'*500+'1','1'*90):
            with self.subTest(文=文[:20]),self.assertRaises((ValueError,RecursionError)): 式を読む(文)

    def test_名前や単位に実行構文を通さない(self):
        for 文 in ('m.__class__','m[0]','<script>'):
            with self.subTest(文=文),self.assertRaises(ValueError): 単位を読む(文)

    def test_有理数にNaNや無限やboolを通さない(self):
        for 値 in ('nan','inf','Infinity',True,1.2,'1/0'):
            with self.subTest(値=値),self.assertRaises(ValueError): 有理を読む(値)

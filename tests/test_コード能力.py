"""限定Python評価の契約と、信頼する固定コードに対するCPythonの独立対照。"""
from copy import deepcopy
from itertools import product
from unittest.mock import patch
import ast
import unittest

from minidora.コード能力 import コードを読む, コードを評価, コードを検証


class コード契約試験(unittest.TestCase):
    def value(self, 情報源, args=None):
        r = コードを評価(情報源, args or {})
        self.assertTrue(r.成立, (r.保留理由, r.データ))
        return r.データ["値"]

    def test_引数と算術(self):
        self.assertEqual(self.value("def f(x,y):\n return (x+y)*2", {"x":3,"y":4}), 14)

    def test_構造と日本語原文の位置(self):
        s="def 合計(数列):\n    値 = 0\n    for 数 in 数列:\n        値 += 数\n    return 値\n"
        r=コードを読む(s)
        self.assertTrue(r.成立)
        self.assertEqual(r.データ["関数"],"合計")
        for item in r.データ["構造"]:
            node=next(n for n in ast.walk(ast.parse(s)) if type(n).__name__==item["構文"] and getattr(n,"lineno",None)==item["行"])
            self.assertEqual(item["原文"],ast.get_source_segment(s,node))

    def test_構文エラー(self):
        r=コードを読む("def f(:\n pass")
        self.assertFalse(r.成立)
        self.assertEqual(r.データ["行"],1)

    def test_未対応構文は死んだ枝でも拒否(self):
        for body in ("import os", "x = ().__class__", "return eval('1')", "return __import__('os')",
                     "return open('/tmp/evil','w')", "return [x for x in []]", "while True:\n   pass",
                     "global x", "raise ValueError()", "return lambda:1", "return 2**10000000", "return 1/2"):
            with self.subTest(body=body):
                s="def f():\n if False:\n  "+body.replace("\n","\n ")+"\n return 0"
                self.assertFalse(コードを読む(s).成立)
                self.assertFalse(コードを評価(s,{}).成立)

    def test_トップレベルの作用と複数関数を拒否(self):
        for s in ("x=1\ndef f():\n return x", "def f():\n return 1\ndef g():\n return 2", "return 1"):
            self.assertFalse(コードを読む(s).成立)

    def test_デコレータ既定値注釈を評価しない(self):
        for s in ("@abc\ndef f():\n return 1", "def f(x=[]):\n return x", "def f(x:int):\n return x",
                  "def f(*xs):\n return xs", "def f(x,/,y):\n return x", "def f(x)->int:\n return x"):
            self.assertFalse(コードを読む(s).成立)

    def test_組込名の上書きは拒否(self):
        for s in ("def f(sum):\n return sum([1])", "def f():\n range=1\n return range(2)", "def len():\n return 0"):
            self.assertFalse(コードを読む(s).成立)

    def test_引数の欠落余分を拒否(self):
        for args in ({},{"y":1},{"x":1,"y":2},[],None):
            self.assertFalse(コードを評価("def f(x):\n return x",args).成立)

    def test_入力型の制限と任意オブジェクト拒否(self):
        for value in (object(),1.5,float('nan'),{1:2},(1,2),b'bytes',10**100):
            self.assertFalse(コードを評価("def f(x):\n return x",{"x":value}).成立)

    def test_循環値と入れ子上限(self):
        x=[];x.append(x)
        self.assertFalse(コードを評価("def f(x):\n return x",{"x":x}).成立)

    def test_呼出し入力を変更しない(self):
        values={"xs":[1,2]}; before=deepcopy(values)
        r=コードを評価("def f(xs):\n xs=xs+[3]\n return xs",values)
        self.assertTrue(r.成立)
        r.データ["値"].append(99)
        self.assertEqual(values,before)

    def test_添字代入やappendは未対応(self):
        for body in ("x[0]=1", "x.append(1)"):
            self.assertFalse(コードを評価("def f(x):\n "+body+"\n return x",{"x":[]}).成立)

    def test_list累算のaliasを数値累算にしない(self):
        self.assertFalse(コードを評価("def f():\n x=[]\n x += [1]\n return x",{}).成立)

    def test_辞書参照と負の添字(self):
        self.assertEqual(self.value("def f(d,xs):\n return d['x']+xs[-1]",{"d":{"x":2},"xs":[1,4]}),6)

    def test_失敗位置と型を返す(self):
        for expr, kind in (("x//0","ZeroDivisionError"),("xs[8]","IndexError"),("missing","NameError")):
            r=コードを評価(f"def f(x,xs):\n return {expr}",{"x":1,"xs":[]})
            self.assertFalse(r.成立)
            self.assertEqual(r.データ["診断"],kind)
            self.assertEqual(r.データ["最終評価行"],2)

    def test_短絡評価は不要な例外を呼ばない(self):
        self.assertFalse(self.value("def f():\n return False and 1//0"))
        self.assertEqual(self.value("def f():\n return 7 or 1//0"),7)
        self.assertFalse(self.value("def f():\n return 2<1<1//0"))

    def test_条件式(self):
        self.assertEqual(self.value("def f(x):\n return 1 if x else 2",{"x":0}),2)

    def test_forのbreak_continue_else(self):
        code="def f(xs):\n total=0\n for x in xs:\n  if x<0:\n   continue\n  if x>10:\n   break\n  total+=x\n else:\n  total+=100\n return total"
        self.assertEqual(self.value(code,{"xs":[-1,2,3]}),105)
        self.assertEqual(self.value(code,{"xs":[2,99,3]}),2)

    def test_早期return(self):
        self.assertEqual(self.value("def f(xs):\n for x in xs:\n  if x>1:\n   return x\n return None",{"xs":[0,3,8]}),3)

    def test_反復外の制御は構文検査で拒否(self):
        for s in ("def f():\n break", "def f():\n continue", "def f():\n for x in []:\n  pass\n else:\n  break"):
            self.assertFalse(コードを読む(s).成立)

    def test_集合生成と集約(self):
        self.assertEqual(self.value("def f():\n return sorted(list(range(5,0,-1)))"),[1,2,3,4,5])
        self.assertEqual(self.value("def f():\n return sum([1,2,3])+max(1,2,3)-min([1,2])"),8)
        self.assertTrue(self.value("def f():\n return all([1,2]) and any([0,3])"))

    def test_反復の変数束縛はPythonに合わせる(self):
        self.assertEqual(self.value("def f():\n for x in [3,4]:\n  pass\n return x"),4)
        self.assertFalse(コードを評価("def f():\n for x in []:\n  pass\n return x",{}).成立)

    def test_暗黙returnとdocstring(self):
        self.assertIsNone(self.value('def f():\n "説明"\n pass'))
        self.assertFalse(コードを読む('def f():\n pass\n "後からの文字列"').成立)

    def test_大きい反復や文字列積を確保前に拒否(self):
        for expr in ("'x'*1000000000", "[0]*1000000000", "list(range(1000000000))", "'x' * (10*100000000)"):
            self.assertFalse(コードを評価("def f():\n return "+expr,{}).成立)

    def test_書式文字列の巨大幅を実行しない(self):
        self.assertFalse(コードを評価("def f():\n return '%999999999s' % 'x'",{}).成立)

    def test_整数桁上限(self):
        self.assertFalse(コードを評価("def f(x):\n return x*x",{"x":2**200}).成立)

    def test_手数上限で部分値を返さない(self):
        r=コードを評価("def f():\n total=0\n for x in range(1000):\n  total+=x\n return total",{},最大手数=20)
        self.assertFalse(r.成立)
        self.assertEqual(r.本文,"")
        self.assertNotIn("値",r.データ)

    def test_停止要求と停止判定故障(self):
        for stop in (lambda:True,lambda:'false',lambda:1/0):
            self.assertFalse(コードを評価("def f():\n return 1",{},停止要求=stop).成立)

    def test_検証でboolとintを同一視しない(self):
        r=コードを検証("def f():\n return True",({"引数":{},"期待値":1},))
        self.assertFalse(r.成立)
        self.assertEqual(r.データ["試験"][0]["実値"],True)

    def test_検証は失敗事例を記録して条件を緩めない(self):
        cases=({"引数":{"x":2},"期待値":4},{"引数":{"x":3},"期待値":9})
        r=コードを検証("def f(x):\n return x+x",cases)
        self.assertFalse(r.成立)
        self.assertEqual([row["一致"] for row in r.データ["試験"]],[True,False])
        self.assertEqual(r.データ["試験"][1]["実値"],6)

    def test_空試験では合格にしない(self):
        for cases in ((),[],({"引数":{}},),({"引数":{},"期待値":1,"任意":True},)):
            self.assertFalse(コードを検証("def f():\n return 1",cases).成立)

    def test_検証の入力変更でハッシュが変わる(self):
        code="def f(x):\n return x"
        a=コードを検証(code,({"引数":{"x":1},"期待値":1},))
        b=コードを検証(code,({"引数":{"x":2},"期待値":2},))
        self.assertTrue(a.成立 and b.成立)
        self.assertNotEqual(a.データ["試験SHA256"],b.データ["試験SHA256"])

    def test_評価中にexec_評価を呼ばない(self):
        with patch('builtins.exec',side_effect=AssertionError()),patch('builtins.eval',side_effect=AssertionError()):
            self.assertEqual(self.value("def f(x):\n return x*2",{"x":3}),6)

    def test_同一入力は同じ評価記録(self):
        s="def f(x):\n return x+1"
        self.assertEqual(コードを評価(s,{"x":1}),コードを評価(s,{"x":1}))

    def test_不正手数とソース長(self):
        for n in (0,True,1.5,200001):
            self.assertFalse(コードを評価("def f():\n return 0",{},最大手数=n).成立)
        self.assertFalse(コードを読む("#"*32769).成立)


class CPython独立対照試験(unittest.TestCase):
    def test_信頼する固定式の算術比較対照(self):
        # 実行するのは試験コード内の固定式だけ。ユーザー入力をexecに渡さない。
        expressions=("x+y","x-y","x*y","x//y","x%y","x<y","x<=y","x==y","x!=y","x>y","x>=y", "x if x>y else y", "x and y", "x or y")
        count=0
        for expression in expressions:
            情報源="def f(x,y):\n return "+expression
            env={};exec(情報源,env)
            for x,y in product(range(-3,4),(-3,-1,1,3)):
                r=コードを評価(情報源,{"x":x,"y":y})
                self.assertTrue(r.成立,(expression,r.保留理由))
                self.assertEqual(r.データ["値"],env["f"](x,y))
                self.assertIs(type(r.データ["値"]),type(env["f"](x,y)))
                count+=1
        self.assertEqual(count,392)

    def test_信頼する固定制御構造との対照(self):
        sources=[
            "def f(xs):\n total=0\n for x in xs:\n  if x>0:\n   total+=x*x\n return total",
            "def f(xs):\n out=[]\n for x in xs:\n  if x%2==0:\n   out=out+[x]\n return out",
            "def f(xs):\n for x in xs:\n  if x<0:\n   continue\n  if x>1:\n   break\n else:\n  return -1\n return x",
            "def f(xs):\n return sorted(xs)",
            "def f(xs):\n return sum(xs)",
            "def f(xs):\n return len(xs)",
        ]
        for 情報源 in sources:
            env={};exec(情報源,env)
            for n in range(4):
                for xs in product((-2,0,3),repeat=n):
                    args={"xs":list(xs)}
                    r=コードを評価(情報源,args)
                    self.assertTrue(r.成立,r.保留理由)
                    self.assertEqual(r.データ["値"],env["f"](list(xs)))

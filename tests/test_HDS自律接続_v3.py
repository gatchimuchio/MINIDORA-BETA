"""監査残件の独立した受入・反例・連結試験。外部モデルの性能試験ではない。"""
from __future__ import annotations
from dataclasses import replace
import unittest
from minidora.HDS実行主体 import *
from minidora.HDS駆動コア import HDS駆動コア
from minidora.統合駆動_v2 import *
from minidora.統合駆動_v2.意味構成 import _前方予測
from minidora.統合駆動_v2.値 import 署名


def known(ID, target, 関係名, value, doc):
    return HDS認識項目(ID, target, 関係名, value, 認識区分.確定,
                      根拠=(doc.出典(),), 検証契約="試験原資料照合")


def rules_fixture(対象名="装置甲"):
    d = HDS資料("規則原本", "1", "入力例: 原因Aなら警報、原因Bなら警報。原因Aなら測定高、原因Bなら測定低。", "試験定義")
    def atom(rel, val):return HDS命題("?対象", rel, val)
    rules = (HDS関係規則("A警報",(atom("原因A",True),),atom("警報",True),(d.出典(),),排他群="説明分類"),
             HDS関係規則("B警報",(atom("原因B",True),),atom("警報",True),(d.出典(),),排他群="説明分類"),
             HDS関係規則("A予測",(atom("原因A",True),),atom("測定", "高"),(d.出典(),)),
             HDS関係規則("B予測",(atom("原因B",True),),atom("測定", "低"),(d.出典(),)))
    状態 = HDS実行状態(要求状態=frozenset({"未完了"}),
                        認識=(known("警報",対象名,"警報",True,d),), 記憶=HDS記憶((d,)))
    return 状態, rules


class 不足生成試験(unittest.TestCase):
    def test_自由文残差から取得まで通常循環で自動接続(self):
        doc = HDS資料("原資料","1","ポンプの流量は12", "試験")
        calls=[]
        def get(req,状態):
            calls.append(req)
            return HDS観測値(12,(doc.出典(),),req.対象,req.関係,資料群=(doc,))
        observer=HDS観測器("計測",get,lambda r,v: v.値==12)
        r=HDS駆動コア(観測器=(observer,)).実行("ポンプの流量を確認して",初期残差=("ポンプの流量が不明",))
        self.assertEqual(r.終端,HDS終端.採用,r.理由)
        self.assertEqual(len(calls),1)
        self.assertEqual((calls[0].対象,calls[0].関係),("ポンプ","流量"))
        self.assertGreater(r.計装.意味構成数,0)
        self.assertEqual(r.状態.認識[0].値,12)

    def test_観測先未接続でも要求は生成され残差は消えない(self):
        r=HDS実行主体(()).実行(HDS実行状態(残差=frozenset({"部品の材質が未確認"})))
        self.assertEqual(r.終端,HDS終端.保留)
        self.assertIn("部品の材質が未確認",r.状態.残差)
        self.assertEqual(r.観測待ち[0].対象,"部品")

    def test_非対応の多義文を補完しない(self):
        for text in ("とにかく不足", "ポンプの流量が不明ではない", "箱の中の温度が不明", "材質が不明という文", "ポンプの流量が不明。ただし調べなくていい"):
            with self.subTest(text=text):self.assertIsNone(不足を抽出(text))

    def test_異なる対象名にも適用する(self):
        for i in range(30):
            x=不足を抽出(f"対象{i}の測定{i}が未確認")
            self.assertEqual((x.対象,x.関係),(f"対象{i}",f"測定{i}"))

    def test_要求認識のエラー文を同期(self):
        with self.assertRaisesRegex(ValueError,"要求認識"):
            HDS駆動コア().実行("入力")


class 仮説生成試験(unittest.TestCase):
    def test_雛型なしで規則から二仮説と識別予測を構成(self):
        s, rules=rules_fixture()
        hs, rs, qs=関係から仮説を構成(s,rules)
        self.assertEqual(len(hs),2)
        mid=next(x.ID for x in rs if x.関係=="測定")
        self.assertEqual(識別対数(hs,mid),1)
        self.assertEqual({next(p.値 for p in h.予測 if p.観測ID==mid) for h in hs},{"高","低"})
        self.assertTrue(all(h.区分!=認識区分.確定 for h in hs))

    def test_通常循環が座標仮説枝を自動生成する(self):
        s,rules=rules_fixture()
        _hs, rs, _qs = 関係から仮説を構成(s,rules)
        s=replace(s,要求認識=frozenset(x.ID for x in rs))
        r=HDS実行主体((),関係規則=rules,最大作用回数=25).実行(s)
        self.assertEqual(r.終端,HDS終端.保留,r.理由)
        self.assertEqual(len(r.状態.仮説),2)
        self.assertEqual(len(r.状態.枝),2)
        self.assertGreater(r.計装.仮説生成数,0)
        self.assertGreater(r.計装.枝生成数,0)
        self.assertLess(len(r.履歴),25)

    def test_変数束縛で未知の対象へ同じ規則を適用(self):
        for name in ("計測器987", "配管乙", "対象new"):
            s,rules=rules_fixture(name)
            hs,rs,_=関係から仮説を構成(s,rules)
            self.assertEqual(len(hs),2)
            self.assertTrue(all(x.対象==name for x in rs))

    def test_未束縛変数を勝手に埋めない(self):
        s,rules=rules_fixture()
        rule=replace(rules[0],前提=(HDS命題("?未知","原因A",True),))
        hs,_,_=関係から仮説を構成(s,(rule,))
        self.assertEqual(hs,())

    def test_出典版が違う規則を使わない(self):
        s,rules=rules_fixture()
        s=replace(s,記憶=s.記憶.更新((replace(s.記憶.正本[0],版="2"),)))
        self.assertEqual(関係から仮説を構成(s,rules)[0],())

    def test_多段の予測を自動生成(self):
        s,rules=rules_fixture()
        d=s.記憶.正本[0]
        extra=HDS関係規則("先の予測",(HDS命題("?対象","測定","高"),),HDS命題("?対象","次段","上昇"),(d.出典(),))
        hs,rs,_=関係から仮説を構成(s,rules+(extra,))
        k=next(x.ID for x in rs if x.関係=="次段")
        self.assertTrue(any(any(p.観測ID==k and p.値=="上昇" for p in h.予測) for h in hs))

    def test_仮説棄却を作業枝に反映(self):
        s,rules=rules_fixture()
        hs,rs,_=関係から仮説を構成(s,rules)
        状態=replace(s,認識=s.認識+rs,仮説=tuple(replace(h,区分=認識区分.棄却) for h in hs))
        branches=仮説から枝を構成(状態)
        self.assertTrue(all(x.区分==認識区分.失効 for b in branches for x in b.認識))

    def test_候補上限超過は明示停止し一部だけを採らない(self):
        s,rules=rules_fixture()
        with self.assertRaises(ValueError):関係から仮説を構成(s,rules,最大件数=1)

    def test_後件肯定を確定根拠へ昇格しない(self):
        s,rules=rules_fixture()
        r=HDS実行主体((),関係規則=rules,最大作用回数=30).実行(s)
        self.assertEqual([x.ID for x in r.状態.認識 if x.区分==認識区分.確定],["警報"])


class 記憶生成試験(unittest.TestCase):
    def test_通常循環で長文を自動圧縮し原文復帰可能(self):
        d=HDS資料("長文","1",("一般の記録です。"*160)+"ただし圧力が低い場合は作動しない。","試験")
        r=HDS実行主体(()).実行(HDS実行状態(記憶=HDS記憶((d,))))
        self.assertEqual(r.終端,HDS終端.採用)
        self.assertEqual(r.計装.圧縮数,1)
        c=r.状態.記憶.圧縮[0]
        self.assertEqual(c.正本へ戻る(r.状態.記憶),(d,))
        self.assertTrue(c.未収録範囲)
        self.assertIn("作動しない",c.要約)
        self.assertEqual(r.状態.認識,())

    def test_原文範囲の完全な分割(self):
        d=HDS資料("文","1","甲は不明。乙は条件付き。丙は確定ではない。"*100,"試験")
        c=原資料を圧縮(d,("乙",),最大文字数=64)
        spans=sorted(c.抽出範囲+c.未収録範囲)
        self.assertEqual(spans[0][0],0);self.assertEqual(spans[-1][1],len(d.本文))
        self.assertTrue(all(a[1]==b[0] for a,b in zip(spans,spans[1:])))
        self.assertTrue(all(d.本文[a:b] in c.要約 for a,b in c.抽出範囲))

    def test_長大単文を偽の完全な要約にしない(self):
        d=HDS資料("長句","1","あ"*2000+"ではない。","試験")
        c=原資料を圧縮(d,最大文字数=64)
        self.assertEqual(c.抽出範囲,())
        self.assertEqual(c.未収録範囲,((0,len(d.本文)),))

    def test_圧縮後に原資料変更なら古い圧縮は失効(self):
        d=HDS資料("文","1","文章。"*400,"試験")
        c=原資料を圧縮(d)
        with self.assertRaises(ValueError):c.正本へ戻る(HDS記憶((replace(d,版="2"),)))


class 形成接続試験(unittest.TestCase):
    def _actions(self, pure=True):
        return tuple(HDS関数作用(str(i),lambda s,j=i:HDS作用結果(HDS作用状態.成立,追加状態=frozenset({str(j+1)})),
                             入力状態=(str(i),),出力状態=(str(i+1),),純粋作用=pure) for i in range(3))

    def test_正常終了から自動形成と実再現検証まで接続(self):
        s=HDS実行状態(成立状態=frozenset({"0"}),要求状態=frozenset({"3"}))
        r=HDS実行主体(self._actions(),最大作用回数=12).実行(s)
        self.assertEqual(r.終端,HDS終端.採用,r.理由)
        self.assertEqual(r.計装.自動形成数,1)
        self.assertEqual(r.計装.形成再検証数,1,r.履歴[-1].理由)
        self.assertEqual(r.計装.再現作用数,3)
        self.assertTrue(r.状態.形成関係[0].使用可能)
        self.assertEqual(r.計装.消費資源,7)

    def test_副作用安全宣言のない処理を勝手に再実行しない(self):
        calls=[]
        a=HDS関数作用("外部",lambda s:(calls.append(1) or HDS作用結果(HDS作用状態.成立,追加状態=frozenset({"済"}))),出力状態=("済",))
        r=HDS実行主体((a,)).実行(HDS実行状態(要求状態=frozenset({"済"})))
        self.assertEqual(calls,[1]);self.assertEqual(r.状態.形成関係,())

    def test_再現で結果変化なら反例として保存(self):
        calls=[]
        a=HDS関数作用("自称純粋",lambda s:(calls.append(1) or HDS作用結果(HDS作用状態.成立,成果=(("値",len(calls)),),追加状態=frozenset({"済"}))),出力状態=("済",),純粋作用=True)
        r=HDS実行主体((a,),最大作用回数=10).実行(HDS実行状態(要求状態=frozenset({"済"})))
        self.assertFalse(r.状態.形成関係[0].使用可能)
        self.assertTrue(r.状態.形成関係[0].反例)
        self.assertEqual(r.計装.形成再検証数,0)

    def test_資源不足で再実行を隠れて実施しない(self):
        s=HDS実行状態(成立状態=frozenset({"0"}),要求状態=frozenset({"3"}))
        r=HDS実行主体(self._actions(),政策=HDS運用政策(最大資源=3)).実行(s)
        self.assertEqual(r.計装.再現作用数,0)
        self.assertEqual(r.計装.消費資源,3)

    def test_保存復元で形成関係と計装が保存される(self):
        s=HDS実行状態(成立状態=frozenset({"0"}),要求状態=frozenset({"3"}))
        r=HDS実行主体(self._actions()).実行(s)
        self.assertEqual(復元する(保存する(r)),r)


class 未知失敗試験(unittest.TestCase):
    def test_未知失敗を分類済みと偽らず代替経路で完了(self):
        def fail(s):raise RuntimeError("原因不明の失敗")
        a=HDS関数作用("A",fail,出力状態=("済",),純粋作用=True)
        b=HDS関数作用("B",lambda s:HDS作用結果(HDS作用状態.成立,追加状態=frozenset({"済"})),出力状態=("済",),純粋作用=True)
        r=HDS実行主体((a,b)).実行(HDS実行状態(要求状態=frozenset({"済"})))
        self.assertEqual(r.終端,HDS終端.採用,r.理由)
        self.assertEqual([x.作用ID for x in r.履歴],["A","B"])
        self.assertFalse(r.履歴[0].診断.原因確定)
        self.assertEqual(r.履歴[0].阻害.種別,停止理由.未知失敗)
        self.assertEqual(r.計装.失敗診断数,1)

    def test_権限例外を代替経路で迂回しない(self):
        def fail(s):raise PermissionError("権限なし")
        a=HDS関数作用("A",fail,出力状態=("済",),純粋作用=True)
        b=HDS関数作用("B",lambda s:HDS作用結果(HDS作用状態.成立,追加状態=frozenset({"済"})),出力状態=("済",))
        r=HDS実行主体((a,b)).実行(HDS実行状態(要求状態=frozenset({"済"})))
        self.assertEqual(r.停止種別,停止理由.権限制約)
        self.assertEqual(len(r.履歴),1)

    def test_安全性不明の例外は停止(self):
        def fail(s):raise RuntimeError("外部処理の成否が不明")
        a=HDS関数作用("A",fail,出力状態=("済",))
        r=HDS実行主体((a,)).実行(HDS実行状態(要求状態=frozenset({"済"})))
        self.assertEqual(r.終端,HDS終端.失敗)
        self.assertFalse(r.履歴[0].診断.再実行安全)

    def test_代替手段がなければ同一失敗を反復しない(self):
        calls=[]
        def fail(s):calls.append(1);raise KeyError("見えない鍵")
        a=HDS関数作用("A",fail,出力状態=("済",),純粋作用=True)
        r=HDS実行主体((a,)).実行(HDS実行状態(要求状態=frozenset({"済"})))
        self.assertEqual(calls,[1]);self.assertEqual(r.終端,HDS終端.保留)
        self.assertTrue(r.履歴[0].診断.不足候補)


class 未来生成試験(unittest.TestCase):
    def test_明示作用から複数段の未来状態を生成する(self):
        specs=(HDS作用仕様("a",追加状態=frozenset({"中間"})),HDS作用仕様("b",入力状態=frozenset({"中間"}),追加状態=frozenset({"完了"})))
        rows=未来列を構成(frozenset(),frozenset(),("a","b"),specs)
        self.assertIn("中間",rows[0].成立状態);self.assertIn("完了",rows[1].成立状態)
        self.assertTrue(all(x.区分=="条件付き予測" for x in rows))

    def test_途中の禁止状態を予測して別計画へ切替(self):
        unsafe=HDS関数作用("A危険",lambda s:HDS作用結果(HDS作用状態.成立,追加状態=frozenset({"完了","危険"})),出力状態=("完了","危険"))
        safe=HDS関数作用("B安全",lambda s:HDS作用結果(HDS作用状態.成立,追加状態=frozenset({"完了"})),出力状態=("完了",))
        c=HDS未来制約("禁止",同時禁止=(frozenset({"危険"}),))
        r=HDS実行主体((unsafe,safe),未来制約=(c,)).実行(HDS実行状態(要求状態=frozenset({"完了"})))
        self.assertEqual(r.終端,HDS終端.採用)
        self.assertEqual(r.履歴[0].作用ID,"B安全")
        self.assertTrue(r.履歴[0].未来状態)

    def test_将来最終状態が良くても途中違反を認めない(self):
        c=HDS未来制約("禁止",同時禁止=(frozenset({"危険"}),))
        specs=(HDS作用仕様("a",追加状態=frozenset({"危険"})), HDS作用仕様("b",入力状態=frozenset({"危険"}),追加状態=frozenset({"済"}),削除状態=frozenset({"危険"})))
        p=作用列を構成(frozenset(),frozenset(),frozenset({"済"}),specs,制約群=(c,))
        self.assertFalse(p.成立)

    def test_実結果が予測外の制約違反なら採用しない(self):
        a=HDS関数作用("A",lambda s:HDS作用結果(HDS作用状態.成立,追加状態=frozenset({"済","危険"})),出力状態=("済",))
        c=HDS未来制約("禁止",同時禁止=(frozenset({"危険"}),))
        r=HDS実行主体((a,),未来制約=(c,)).実行(HDS実行状態(要求状態=frozenset({"済"})))
        self.assertEqual(r.終端,HDS終端.失敗)
        self.assertIn("危険",r.状態.成立状態)

    def test_保存復元で予測と実状態の区分が維持される(self):
        a=HDS関数作用("A",lambda s:HDS作用結果(HDS作用状態.成立,追加状態=frozenset({"済"})),出力状態=("済",))
        r=HDS実行主体((a,)).実行(HDS実行状態(要求状態=frozenset({"済"})))
        restored=復元する(保存する(r))
        self.assertEqual(restored,r)


class 再監査反例試験(unittest.TestCase):
    def test_形成手順を通常の次回実行が自動再利用(self):
        actions=tuple(HDS関数作用(str(i),lambda s,j=i:HDS作用結果(HDS作用状態.成立,追加状態=frozenset({str(j+1)})),
                                 入力状態=(str(i),),出力状態=(str(i+1),),純粋作用=True) for i in range(3))
        s=HDS実行状態(成立状態=frozenset({"0"}),要求状態=frozenset({"3"}))
        実行主体=HDS実行主体(actions)
        r=実行主体.実行(s)
        t=実行主体.実行(replace(s,形成関係=r.状態.形成関係))
        self.assertEqual(t.終端,HDS終端.採用)
        self.assertGreater(t.計装.形成再利用数,0)
        self.assertEqual([h.作用ID for h in t.履歴[:3]],["0","1","2"])

    def test_仮説の相互排他を勝手に仮定しない(self):
        s,rules=rules_fixture()
        hs,rs,_=関係から仮説を構成(s,tuple(replace(r,排他群="") for r in rules))
        self.assertTrue(all(not h.排他群 for h in hs))
        mid=next(x.ID for x in rs if x.関係=="測定")
        self.assertEqual(識別対数(hs,mid),0)

    def test_認識源更新で仮説とその枝を再開放(self):
        s,rules=rules_fixture()
        _hs, rs, _qs = 関係から仮説を構成(s,rules)
        s=replace(s,要求認識=frozenset(x.ID for x in rs))
        実行主体=HDS実行主体((),関係規則=rules,最大作用回数=40)
        r=実行主体.実行(s)
        d=HDS資料("計測更新","1","警報なし", "試験")
        update=HDS作用結果(HDS作用状態.成立,認識更新=(known("警報","装置甲","警報",False,d),),記憶更新=r.状態.記憶.更新((d,)))
        t=実行主体.再開(r,update)
        self.assertGreater(t.計装.依存失効数,0)
        self.assertNotEqual(t.状態.状態署名,r.状態.状態署名)
        self.assertTrue(all(h.区分 in (認識区分.棄却,認識区分.失効) for h in t.状態.仮説))

    def test_確定予測なしの循環規則を無限展開しない(self):
        s,rules=rules_fixture()
        d=s.記憶.正本[0]
        cycle=HDS関係規則("自己循環",(HDS命題("?対象","原因A",True),),HDS命題("?対象","原因A",True),(d.出典(),))
        hs,_,_=関係から仮説を構成(s,rules+(cycle,))
        self.assertEqual(len(hs),2)


if __name__ == "__main__":
    unittest.main()

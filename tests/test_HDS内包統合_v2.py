"""HDS内包統合v2の機構試験。ベンチ正答率や一般知能の達成値ではない。"""
from __future__ import annotations
from dataclasses import replace
from pathlib import Path
from hashlib import sha256
import math
import subprocess
import os
import sys
import unittest

from minidora.HDS実行主体 import (
    HDS実行主体, HDS実行状態, HDS作用機会, HDS作用結果, HDS作用状態,
    HDS終端, HDS関数作用, 標準HDS作用選択器,
)
from minidora.HDS駆動コア import HDS駆動コア
from minidora.統合駆動_v2 import *
from minidora.統合駆動_v2.値 import 署名
from minidora.統合駆動_v2.状態更新 import 有効認識

F = frozenset


def 資料(ID="資料A", 版="1", 本文="値=2"):
    return HDS資料(ID, 版, 本文, "試験入力", "2026-09-17", ("対象", "値"))


def 確定(ID="a", 値=2, 元=None, **kw):
    元 = 元 or 資料()
    return HDS認識項目(ID, "対象", "値", 値, 認識区分.確定, 根拠=(元.出典(),), 検証契約="試験資料照合/v1", **kw)


def 成立(追加=(), **kw):
    return HDS作用結果(HDS作用状態.成立, 追加状態=F(追加), **kw)


def 単純作用(ID, 入力=(), 出力=(), **kw):
    return HDS関数作用(ID, lambda s: 成立(出力), 入力状態=入力, 出力状態=出力, **kw)


class 署名境界試験(unittest.TestCase):
    def test_写像順序非依存(self):
        self.assertEqual(署名({"a": 1, "b": 2}), 署名({"b": 2, "a": 1}))

    def test_写像鍵の型を保持(self):
        self.assertNotEqual(署名({1: "x"}), 署名({"1": "x"}))

    def test_集合順序非依存(self):
        self.assertEqual(署名(F(("あ", "z"))), 署名(F(("z", "あ"))))

    def test_非有限値を拒否(self):
        for n in (math.inf, -math.inf, math.nan):
            with self.subTest(n=n), self.assertRaises(ValueError):
                署名(n)

    def test_object住所を意味にしない(self):
        with self.assertRaises(TypeError):
            署名(object())

    def test_循環参照を拒否(self):
        d = {}; d["self"] = d
        with self.assertRaises(ValueError):
            署名(d)

    def test_欠落とNoneを区別(self):
        s = HDS実行状態(要求状態=F({"未達"}))
        t, d = HDS実行主体._状態更新(s, 成立(成果=(("x", None),)))
        self.assertTrue(d.変化有無)
        self.assertEqual(d.変更成果, ("x",))

    def test_認識に可変値を入れない(self):
        with self.assertRaises(TypeError):
            HDS認識項目("a", "対象", "関係", {"値": 1})

    def test_確定の無根拠昇格を拒否(self):
        with self.assertRaises(ValueError):
            HDS認識項目("a", "対象", "関係", 1, 認識区分.確定)

    def test_根拠の重複計上を拒否(self):
        e = 資料().出典()
        with self.assertRaises(ValueError):
            HDS認識項目("a", "対象", "関係", 1, 認識区分.暫定, 根拠=(e, e))

    def test_ハッシュ乱数種が異なっても署名同一(self):
        code = f"import sys; sys.path.insert(0, {str(Path(__file__).resolve().parents[1] / 'src')!r}); from minidora.統合駆動_v2.値 import 署名; print(署名(frozenset({{'a','b','日本語'}})))"
        out = []
        for seed in ("1", "987"):
            env = dict(os.environ, PYTHONHASHSEED=seed)
            out.append(subprocess.check_output([sys.executable, "-c", code], env=env, text=True))
        self.assertEqual(*out)


class 基本循環互換試験(unittest.TestCase):
    def test_局所成功と目的閉包を分離(self):
        r = HDS実行主体((単純作用("局所", 出力=("途中",), 目的依存=("状態:完了",)),)).実行(HDS実行状態(要求状態=F({"完了"})))
        self.assertEqual(r.終端, HDS終端.保留)
        self.assertIn("途中", r.状態.成立状態)

    def test_残差を高優先度装飾より優先(self):
        logs = []
        a = HDS関数作用("参照", lambda s: logs.append("参照") or 成立(("資料",), 解消残差=F({"不足"})), 出力状態=("資料",), 解消対象=("不足",))
        b = HDS関数作用("完了", lambda s: logs.append("完了") or 成立(("完了",)), 入力状態=("資料",), 出力状態=("完了",))
        c = HDS関数作用("装飾", lambda s: logs.append("装飾") or 成立(("装飾",)), 出力状態=("装飾",), 優先度=999)
        r = HDS実行主体((c, b, a)).実行(HDS実行状態(要求状態=F({"完了"}), 残差=F({"不足"})))
        self.assertEqual(r.終端, HDS終端.採用)
        self.assertEqual(logs, ["参照", "完了"])

    def test_同じ作用入力で無進展反復しない(self):
        calls = []
        a = HDS関数作用("無進展", lambda s: calls.append(1) or HDS作用結果(HDS作用状態.保留), 目的依存=("状態:未達",))
        r = HDS実行主体((a,), 最大作用回数=8).実行(HDS実行状態(要求状態=F({"未達"})))
        self.assertEqual(calls, [1])
        self.assertEqual(len(r.履歴), 1)
        self.assertEqual(r.状態.版, 0)

    def test_新状態差があれば同一作用を使える(self):
        def run(s):
            return 成立(("段階1" if "段階1" not in s.成立状態 else "段階2",))
        a = HDS関数作用("段階", run, 出力状態=("段階1", "段階2"), 機会判定=lambda s:"段階2" not in s.成立状態)
        r = HDS実行主体((a,), 最大作用回数=4).実行(HDS実行状態(要求状態=F({"段階2"})))
        self.assertEqual(r.終端, HDS終端.採用)
        self.assertEqual(len(r.履歴), 2)

    def test_主体状態更新(self):
        a = HDS関数作用("主体", lambda s: 成立(解消残差=F({"更新"}), 主体状態差分=(("現在目的", "検証"),)), 解消対象=("更新",))
        r = HDS実行主体((a,)).実行(HDS実行状態(残差=F({"更新"})))
        self.assertEqual(r.終端, HDS終端.採用)
        self.assertEqual(r.状態.主体辞書()["現在目的"], "検証")

    def test_公開コアは明示目的条件を要求(self):
        with self.assertRaises(ValueError):
            HDS駆動コア().実行("何かする", 目的=("回答",))

    def test_未知停止の理由を残す(self):
        a = HDS関数作用("壊れた", lambda s: (_ for _ in ()).throw(RuntimeError("未知の停止")), 目的依存=("状態:完了",))
        r = HDS実行主体((a,)).実行(HDS実行状態(要求状態=F({"完了"})))
        self.assertEqual(r.停止種別, 停止理由.未知失敗)
        self.assertEqual(r.終端, HDS終端.失敗)
        self.assertEqual(r.計装.作用実行数, 1)

    def test_失敗を目的達成へ昇格させない(self):
        a = HDS関数作用("不正", lambda s: HDS作用結果(HDS作用状態.失敗, 追加状態=F({"完了"})), 出力状態=("完了",))
        r = HDS実行主体((a,)).実行(HDS実行状態(要求状態=F({"完了"})))
        self.assertEqual(r.終端, HDS終端.失敗)
        self.assertNotIn("完了", r.状態.成立状態)

    def test_作用による入力直接変更を検知(self):
        def run(s):
            s.成果[0][1].append(2)
            return 成立(("完了",))
        initial = HDS実行状態(要求状態=F({"完了"}), 成果=(("値", [1]),))
        r = HDS実行主体((HDS関数作用("改変", run, 目的依存=("状態:完了",)),)).実行(initial)
        self.assertEqual(r.終端, HDS終端.失敗)
        self.assertEqual(initial.成果辞書()["値"], [1])
        self.assertEqual(r.状態.成果辞書()["値"], [1])


class 意味状態差試験(unittest.TestCase):
    def test_値が不変でも根拠変更を検出(self):
        x, y = 資料("x"), 資料("y")
        a = 確定(元=x)
        b = replace(a, 根拠=(y.出典(),))
        s = HDS実行状態(認識=(a,), 記憶=HDS記憶((x, y)))
        t, d = HDS実行主体._状態更新(s, 成立(認識更新=(b,)))
        self.assertTrue(d.変化有無)
        self.assertIn("根拠", d.認識差[0].変更項目)
        self.assertEqual(t.認識辞書()["a"].値, a.値)
        self.assertEqual(t.認識履歴, (a,))

    def test_全変化対象を保持(self):
        doc = 資料()
        old = tuple(確定(k, 元=doc) for k in ("A", "B", "C"))
        new = tuple(replace(x, 時点="更新後") for x in old)
        s = HDS実行状態(認識=old, 記憶=HDS記憶((doc,)))
        _, d = HDS実行主体._状態更新(s, 成立(認識更新=new))
        self.assertEqual({x.ID for x in d.認識差}, {"A", "B", "C"})

    def test_改訂番号だけで意味進展を捏造しない(self):
        doc = 資料(); a = 確定(元=doc)
        s = HDS実行状態(認識=(a,), 記憶=HDS記憶((doc,)))
        t, d = HDS実行主体._状態更新(s, 成立(認識更新=(replace(a, 改訂=9),)))
        self.assertFalse(d.変化有無)
        self.assertEqual(t.版, 0)

    def test_条件と範囲も状態差(self):
        a = HDS認識項目("a", "対象", "関係", 1, 認識区分.条件付き, 条件=("c",))
        b = replace(a, 条件=("d",), 範囲="限定")
        self.assertEqual(set(認識差分((a,), (b,))[0].変更項目), {"条件", "範囲"})

    def test_依存下流のみを失効(self):
        d1, d2 = 資料("x"), 資料("y")
        a = 確定("a", 元=d1)
        edges = (HDS依存辺("認識:a", "成果:中間"), HDS依存辺("成果:中間", "状態:完了"))
        s = HDS実行状態(要求状態=F({"完了"}), 成立状態=F({"完了", "無関係"}), 成果=(("中間", 5), ("無関係", 7)), 認識=(a,), 記憶=HDS記憶((d1, d2)), 依存=edges)
        t, delta = HDS実行主体._状態更新(s, 成立(認識更新=(replace(a, 根拠=(d2.出典(),)),)))
        self.assertNotIn("完了", t.成立状態)
        self.assertNotIn("中間", t.成果辞書())
        self.assertEqual(t.成果辞書()["無関係"], 7)
        self.assertIn("無関係", t.成立状態)
        self.assertEqual(set(delta.失効対象), {"成果:中間", "状態:完了"})

    def test_資料変更で古い確定認識を失効(self):
        old, new = 資料(), 資料(版="2", 本文="値=3")
        s = HDS実行状態(認識=(確定(元=old),), 記憶=HDS記憶((old,)), 要求認識=F({"a"}))
        t, d = HDS実行主体._状態更新(s, 成立(記憶更新=s.記憶.更新((new,))))
        self.assertEqual(t.認識辞書()["a"].区分, 認識区分.失効)
        self.assertFalse(t.閉包済み)
        self.assertIn("認識:a", d.失効対象)

    def test_失効した成果を古い署名で再採用できない(self):
        d1, d2 = 資料("x"), 資料("y")
        a = 確定("a", 元=d1)
        s = HDS実行状態(認識=(a,), 記憶=HDS記憶((d1,d2)), 成果=(("b", 1),), 依存=(HDS依存辺("認識:a","成果:b"),))
        t, _ = HDS実行主体._状態更新(s, 成立(認識更新=(replace(a,根拠=(d2.出典(),)),)))
        with self.assertRaises(ValueError):
            HDS実行主体._状態更新(t, 成立(成果=(("b", 1),),検証依存=(("認識:a",s.ノード署名("認識:a")),)))

    def test_新入力の再検証後にだけ成果を戻す(self):
        d1, d2 = 資料("x"), 資料("y"); a=確定("a",元=d1)
        s=HDS実行状態(認識=(a,),記憶=HDS記憶((d1,d2)),成果=(("b",1),),依存=(HDS依存辺("認識:a","成果:b"),))
        t,_=HDS実行主体._状態更新(s,成立(認識更新=(replace(a,根拠=(d2.出典(),)),)))
        u,_=HDS実行主体._状態更新(t,成立(成果=(("b",2),),検証依存=(("認識:a",t.ノード署名("認識:a")),)))
        self.assertEqual(u.成果辞書()["b"],2)
        self.assertNotIn("成果:b",u.再評価待ち)

    def test_依存周期は有限に処理し閉包根拠にしない(self):
        es=(HDS依存辺("状態:a","状態:b"),HDS依存辺("状態:b","状態:a"))
        self.assertEqual(下流集合(F({"状態:a"}),es),F({"状態:a","状態:b"}))
        s=HDS実行状態(要求状態=F({"a"}),成立状態=F({"a","b"}),依存=es)
        self.assertFalse(s.閉包済み)

    def test_原資料のない確定ラベルだけでは閉じない(self):
        s=HDS実行状態(要求認識=F({"a"}),認識=(確定(),))
        self.assertFalse(s.閉包済み)


class 観測仮説試験(unittest.TestCase):
    def test_未観測状態から問い合わせを生成して解消(self):
        calls=[]; doc=資料()
        def get(q,s):
            calls.append(q.問合せ)
            return HDS観測値(2,(doc.出典(),),q.対象,q.関係,資料群=(doc,))
        p=HDS観測器("資料読取",get,lambda q,v: v.値==2)
        r=HDS駆動コア(観測器=(p,)).実行("値を確認",要求認識=("a",),初期認識=(HDS認識項目("a","対象","値"),))
        self.assertEqual(r.終端,HDS終端.採用)
        self.assertEqual(r.状態.認識辞書()["a"].値,2)
        self.assertIn("対象=対象",calls[0])
        self.assertEqual(r.計装.観測実行数,1)

    def test_意味定義のないIDから検索内容を捏造しない(self):
        r=HDS駆動コア().実行("確認",要求認識=("未知ID",))
        self.assertEqual(r.終端,HDS終端.保留)
        self.assertEqual(r.観測待ち,())
        self.assertEqual(r.停止種別,停止理由.依存未閉包)

    def test_範囲の違う観測は採用しない(self):
        doc=資料()
        p=HDS観測器("観測",lambda q,s:HDS観測値(3,(doc.出典(),),q.対象,q.関係,"別範囲",資料群=(doc,)),lambda q,v:True)
        s=HDS実行状態(要求認識=F({"a"}),認識=(HDS認識項目("a","対象","値",範囲="範囲A"),))
        r=HDS実行主体((),観測器=(p,)).実行(s)
        self.assertEqual(r.停止種別,停止理由.観測不足)
        self.assertNotEqual(r.状態.認識辞書()["a"].区分,認識区分.確定)

    def test_競合値とそれぞれの根拠を残す(self):
        a,b=資料("A"),資料("B",本文="値=3")
        p=HDS観測器("観測",lambda q,s:(HDS観測値(2,(a.出典(),),q.対象,q.関係,資料群=(a,)),HDS観測値(3,(b.出典(),),q.対象,q.関係,資料群=(b,))),lambda q,v:True)
        r=HDS実行主体((),観測器=(p,)).実行(HDS実行状態(要求認識=F({"a"}),認識=(HDS認識項目("a","対象","値"),)))
        self.assertEqual(r.停止種別,停止理由.証拠競合)
        self.assertEqual(r.状態.認識辞書()["a"].区分,認識区分.競合)
        self.assertEqual(len([x for x in r.状態.認識 if "/候補/" in x.ID]),2)

    def test_仮説を最も分ける観測を選ぶ(self):
        hs=(HDS仮説("h1","仮説1",(HDS予測("x",1),HDS予測("y",2)),排他群="g"),
            HDS仮説("h2","仮説2",(HDS予測("x",1),HDS予測("y",3)),排他群="g"),
            HDS仮説("h3","仮説3",(HDS予測("x",1),HDS予測("y",4)),排他群="g"))
        req=必要観測を構成((HDS認識項目("x","x","値"),HDS認識項目("y","y","値")),F({"x","y"}),hs)
        self.assertEqual(req[0].ID,"y")
        self.assertEqual(識別対数(hs,"y"),3)
        self.assertEqual(識別対数(hs,"x"),0)

    def test_観測器失敗時は別観測器へ進む(self):
        calls=[];doc=資料()
        p=HDS観測器("A",lambda q,s:calls.append("A") or (),lambda q,v:True)
        q=HDS観測器("B",lambda q,s:calls.append("B") or HDS観測値(2,(doc.出典(),),q.対象,q.関係,資料群=(doc,)),lambda q,v:True)
        r=HDS実行主体((),観測器=(p,q)).実行(HDS実行状態(要求認識=F({"a"}),認識=(HDS認識項目("a","対象","値"),)))
        self.assertEqual(r.終端,HDS終端.採用)
        self.assertEqual(calls,["A","B"])

    def test_仮説生成は事実確定ではない(self):
        doc=資料();base=確定("根",元=doc)
        template=HDS仮説雛型("規則",("根",),(HDS仮説("h","未観測の仮説",(HDS予測("次",3),),排他群="g"),))
        r=HDS実行主体((),仮説雛型=(template,)).実行(HDS実行状態(要求認識=F({"次"}),認識=(base,),記憶=HDS記憶((doc,))))
        self.assertTrue(r.状態.仮説)
        self.assertEqual(r.状態.仮説[0].区分,認識区分.暫定)
        self.assertEqual(r.終端,HDS終端.保留)

    def test_反証から仮説を棄却(self):
        doc=資料();obs=確定("a",2,doc)
        h=HDS仮説("h","3である",(HDS予測("a",3),),排他群="g")
        s=HDS実行状態(認識=(obs,),仮説=(h,),記憶=HDS記憶((doc,)))
        from minidora.統合駆動_v2.作用 import 仮説再照合作用
        更新 = 仮説再照合作用().実行(s)
        t, _ = HDS実行主体._状態更新(s, 更新)
        self.assertEqual(t.仮説[0].区分,認識区分.棄却)

    def test_作業枝は仮定を落とさず合流(self):
        bs=(HDS作業枝("左",("前提A",),(HDS認識項目("x","対象","値",1,認識区分.暫定),)),
            HDS作業枝("右",("前提B",),(HDS認識項目("x","対象","値",2,認識区分.暫定),)))
        s=HDS実行状態(枝=bs)
        from minidora.統合駆動_v2.作用 import 枝合流作用
        更新 = 枝合流作用().実行(s)
        t, _ = HDS実行主体._状態更新(s, 更新)
        self.assertEqual(len(t.認識),2)
        self.assertEqual({x.区分 for x in t.認識},{認識区分.条件付き})
        self.assertEqual({x.値 for x in t.認識},{1,2})


class 記憶政策試験(unittest.TestCase):
    def test_索引は正本ポインタだけ(self):
        doc=資料();m=HDS記憶((doc,))
        idx=m.索引()[0]
        self.assertFalse(hasattr(idx,"本文"))
        self.assertEqual(m.正本[0].本文,doc.本文)

    def test_参照計画の有限再利用(self):
        doc=資料();m=HDS記憶((doc,))
        p=m.計画する("q",("対象",),"証拠",2)
        docs,p=p.消費("q",m,"証拠")
        self.assertEqual(docs,(doc,))
        docs,p=p.消費("q",m,"証拠")
        with self.assertRaises(ValueError):p.消費("q",m,"証拠")

    def test_正本改訂と問い変更で参照計画失効(self):
        m=HDS記憶((資料(),));p=m.計画する("q",("対象",),"e")
        self.assertFalse(p.再利用可能("q2",m,"e"))
        self.assertFalse(p.再利用可能("q",m,"e2"))
        self.assertFalse(p.再利用可能("q",m.更新((資料(版="2"),)),"e"))

    def test_同じ版の正本無言上書き拒否(self):
        with self.assertRaises(ValueError):
            HDS記憶((資料(),)).更新((資料(本文="違う"),))

    def test_圧縮から正本へ戻れる(self):
        doc=資料();c=HDS圧縮記憶("要約","抜粋",(doc.出典(),))
        self.assertEqual(c.正本へ戻る(HDS記憶((doc,))),(doc,))
        with self.assertRaises(ValueError):c.正本へ戻る(HDS記憶((資料(版="2"),)))

    def test_作用権限を迂回しない(self):
        a=単純作用("外部書込",出力=("完了",),必要権限=("書込",))
        r=HDS実行主体((a,)).実行(HDS実行状態(要求状態=F({"完了"})))
        self.assertEqual(r.停止種別,停止理由.権限制約)
        self.assertEqual(r.計装.作用実行数,0)
        with self.assertRaises(ValueError):HDS実行主体((a,)).再開(r)

    def test_禁止作用の代わりに許可された経路を使う(self):
        a=単純作用("禁止",出力=("完了",));b=単純作用("許可",出力=("完了",))
        r=HDS実行主体((a,b),政策=HDS運用政策(禁止作用=("禁止",))).実行(HDS実行状態(要求状態=F({"完了"})))
        self.assertEqual(r.終端,HDS終端.採用)
        self.assertEqual(r.履歴[0].作用ID,"許可")

    def test_資源上限を越えない(self):
        a=単純作用("大きな作用",出力=("完了",),資源負荷=5)
        r=HDS実行主体((a,),政策=HDS運用政策(最大資源=4)).実行(HDS実行状態(要求状態=F({"完了"})))
        self.assertEqual(r.停止種別,停止理由.予算枯渇)
        self.assertEqual(r.計装.消費資源,0)

    def test_真の進展でのみ軟予算を拡張(self):
        acts=tuple(単純作用(str(i),入力=(str(i-1),) if i else (),出力=(str(i),)) for i in range(4))
        r=HDS実行主体(acts,最大作用回数=4,政策=HDS運用政策(初期作用予算=1,予算増分=1)).実行(HDS実行状態(要求状態=F({"3"})))
        self.assertEqual(r.終端,HDS終端.採用)
        self.assertEqual(r.計装.予算拡張数,3)
        self.assertEqual(r.計装.目的進展数,4)
        self.assertTrue(all(x.目的進展 for x in r.履歴))


class 計画回復検証試験(unittest.TestCase):
    def test_未登録の三段作用列を構成(self):
        specs=(HDS作用仕様("読む",追加状態=F({"資料"})),HDS作用仕様("計算",入力状態=F({"資料"}),追加状態=F({"値"})),HDS作用仕様("検証",入力状態=F({"値"}),追加状態=F({"完了"})))
        p=作用列を構成(F(),F(),F({"完了"}),specs)
        self.assertEqual(p.作用列,("読む","計算","検証"))

    def test_削除効果を無視しない(self):
        specs=(HDS作用仕様("危険",入力状態=F({"a"}),追加状態=F({"b"}),削除状態=F({"a"})),)
        p=作用列を構成(F({"a"}),F(),F({"a","b"}),specs)
        self.assertFalse(p.成立)

    def test_有限探索の打切りを不可能証明にしない(self):
        specs=tuple(HDS作用仕様(str(i),入力状態=F({str(i-1)}) if i else F(),追加状態=F({str(i)})) for i in range(4))
        p=作用列を構成(F(),F(),F({"3"}),specs,最大深さ=2)
        self.assertFalse(p.成立)
        self.assertTrue(p.打切り)

    def test_失敗理由の不足状態から修復し再実行(self):
        calls=[]
        def need(s):
            calls.append("本処理")
            if "修復" not in s.成立状態:
                raise HDS作用失敗(HDS阻害(停止理由.外部作用失敗,"本処理","前提不足",True,必要状態=("修復",)))
            return 成立(("完了",))
        a=HDS関数作用("本処理",need,出力状態=("完了",))
        b=HDS関数作用("修復処理",lambda s:calls.append("修復") or 成立(("修復",)),出力状態=("修復",))
        r=HDS実行主体((a,b),最大作用回数=6).実行(HDS実行状態(要求状態=F({"完了"})))
        self.assertEqual(r.終端,HDS終端.採用)
        self.assertEqual(calls,["本処理","修復","本処理"])
        self.assertGreater(r.計装.修復選択数,0)

    def test_草案検証不成立では成果を出さない(self):
        d=HDS草案("d",(("回答","誤り"),),("確認",),追加状態=F({"回答済み"}))
        r=HDS実行主体((),検証器=(HDS検証器("確認",lambda s,d:False),)).実行(HDS実行状態(要求状態=F({"回答済み"}),草案=(d,)))
        self.assertEqual(r.終端,HDS終端.保留)
        self.assertNotIn("回答",r.状態.成果辞書())
        self.assertEqual(r.状態.草案[0].区分,"棄却")

    def test_草案は検証後に目的状態へ反映(self):
        d=HDS草案("d",(("回答",4),),("確認",),追加状態=F({"回答済み"}))
        r=HDS実行主体((),検証器=(HDS検証器("確認",lambda s,d:dict(d.成果)["回答"]==4),)).実行(HDS実行状態(要求状態=F({"回答済み"}),草案=(d,)))
        self.assertEqual(r.終端,HDS終端.採用)
        self.assertEqual(r.状態.成果辞書()["回答"],4)
        self.assertTrue(r.状態.検証票[0].合格)

    def test_古い前提の草案を採用しない(self):
        s=HDS実行状態(成果=(("値",2),))
        d=HDS草案("d",(("回答",4),),("確認",),依存署名=(("成果:値",s.ノード署名("成果:値")),),追加状態=F({"回答済み"}))
        s=replace(s,成果=(("値",3),),要求状態=F({"回答済み"}),草案=(d,))
        r=HDS実行主体((),検証器=(HDS検証器("確認",lambda s,d:True),)).実行(s)
        self.assertEqual(r.終端,HDS終端.保留)
        self.assertNotIn("回答",r.状態.成果辞書())

    def test_先行草案の最初の不成立以降を巻戻す(self):
        r=先行草案を検証((1,2,3,4),lambda prefix:sum(prefix)<=3)
        self.assertEqual(r.採用接頭部,(1,2))
        self.assertEqual(r.巻戻し部分,(3,4))
        self.assertFalse(r.全体成立)

    def test_正常閉包でも最終検証を省略しない(self):
        a=単純作用("答える",出力=("完了",))
        r=HDS実行主体((a,),最終検証器=(HDS検証器("全体",lambda s,d:False),),最大作用回数=5).実行(HDS実行状態(要求状態=F({"完了"})))
        self.assertEqual(r.終端,HDS終端.保留)
        self.assertIn("検証:全体",r.状態.残差)
        self.assertGreater(r.計装.検証失敗数,0)

    def test_再開で過去と同じ入力を再実行しない(self):
        calls=[]
        a=HDS関数作用("無進展",lambda s:calls.append(1) or HDS作用結果(HDS作用状態.保留),目的依存=("状態:未達",))
        実行主体=HDS実行主体((a,),最大作用回数=8)
        r=実行主体.実行(HDS実行状態(要求状態=F({"未達"})))
        t=実行主体.再開(r)
        self.assertEqual(len(t.履歴),1)
        self.assertEqual(calls,[1])

    def test_異種表象は適合器後の暫定情報として入る(self):
        rep=HDS異種表象("画像1","画像","adapter-v1",資料("img",本文="adapter出力"),(("個数","等しい",3),))
        r=HDS駆動コア().実行("入力を接続",要求状態=("異種入力接続済み",),異種表象=(rep,))
        self.assertEqual(r.終端,HDS終端.採用)
        self.assertEqual(r.状態.認識[0].区分,認識区分.暫定)
        self.assertIn("適合器検証:adapter-v1",r.状態.認識[0].条件)

    def test_形成は検証前に実行規則へ昇格しない(self):
        e=HDS経験("e",F({"入力"}),F({"完了"}),("読む","計算"),True,"実測1","文脈")
        r=経験から形成(e)
        self.assertFalse(r.使用可能)
        v=再実行で検証(r,replace(e,ID="e2",根拠署名="実測2"),"再現-v1")
        self.assertTrue(v.使用可能)
        bad=再実行で検証(v,replace(e,成功=False,失敗署名="反例1"),"再現-v1")
        self.assertFalse(bad.使用可能)
        self.assertIn("反例1",bad.反例)


if __name__ == '__main__':
    unittest.main()

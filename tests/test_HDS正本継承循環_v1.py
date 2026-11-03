from __future__ import annotations

import unittest
from unittest.mock import patch

from minidora.HDS構文化器_v1 import 公開HDSコンパイラ
from minidora.HDS駆動コア import HDS駆動コア, HDS駆動コア版, HDS継承基準版
from minidora.HDS実行主体 import HDS実行状態, HDS終端, HDS作用供給器, HDS関数作用, HDS作用結果, HDS作用状態
from minidora.HDS選択実行系 import HDS選択実行結果
from minidora.HDS選択継承循環 import (
    HDS選択継承供給,
    回答成果名,
    参照成果名,
    初回評価参照成果名,
    残差_証明不足,
    選択閉包状態,
    基準結果主体名,
    現行結果成果名,
    影結果成果名,
    非退行判定成果名,
    関係観測世代成果名,
    入力残差影成果名,
)
from minidora.参照 import 参照記録


class 固定追加参照:
    名称 = "試験追加参照"

    def __init__(self, 記録群):
        self.記録群 = tuple(記録群)
        self.呼出回数 = 0

    def 検索(self, 問合せ, 上限=8):
        self.呼出回数 += 1
        return self.記録群[:上限]


class 第二層追加参照:
    名称 = "第二層試験追加参照"

    def __init__(self):
        self.呼出: list[str] = []

    def 検索(self, 問合せ, 上限=8):
        query = " ".join(str(問合せ).split())
        self.呼出.append(query)
        if query.casefold() in {"enzyme x molecule a", "molecule a"}:
            return (証拠("Molecule A", 識別子="second-layer"),)
        return ()


class 第二層反証参照:
    名称 = "第二層反証試験参照"

    def __init__(self):
        self.呼出: list[str] = []

    def 検索(self, 問合せ, 上限=8):
        query = " ".join(str(問合せ).split())
        self.呼出.append(query)
        if query.casefold() in {"enzyme x molecule b", "molecule b"}:
            return (証拠("Molecule B", 否定=True, 識別子="second-layer-b"),)
        return ()


class 第二層完全参照:
    名称 = "第二層完全試験参照"

    def __init__(self):
        self.呼出: list[str] = []

    def 検索(self, 問合せ, 上限=8):
        query = " ".join(str(問合せ).split())
        self.呼出.append(query)
        if query.casefold() in {"enzyme x molecule a", "molecule a", "enzyme x molecule b", "molecule b"}:
            return (
                証拠("Molecule A", 識別子="second-layer-a"),
                証拠("Molecule B", 否定=True, 識別子="second-layer-b"),
            )[:上限]
        return ()


def 証拠(主体: str, *, 否定: bool = False, 識別子: str = "r1"):
    本文 = f"{主体} {'does not inhibit' if 否定 else 'inhibits'} Enzyme X."
    return 参照記録(
        識別子=識別子,
        対象=主体,
        内容=本文,
        由来="試験",
        供給器="試験資料",
        信頼=1.0,
    )


class HDS正本継承循環試験(unittest.TestCase):
    def setUp(self):
        self.構文化器 = 公開HDSコンパイラ()
        self.コア = HDS駆動コア(HDSコンパイラ=self.構文化器, 最大作用回数=24)
        self.問い = "Which molecule inhibits Enzyme X?"
        self.選択肢 = ("Molecule A", "Molecule B")

    def test_版はv7(self):
        self.assertEqual(HDS駆動コア版, "MINIDORA-HDS-FIRST-v7")
        self.assertEqual(HDS継承基準版, "HDS-MINIDORA-63d5d7e7")

    def test_一般非退行入口は基準承認済みなら拡張を起動しない(self):
        基準 = object()
        呼出 = []
        判定 = HDS駆動コア().非退行継承実行(
            "入力",
            基準実行=lambda: 基準,
            基準承認判定=lambda 値: 値 is 基準,
            拡張実行=lambda: 呼出.append("拡張"),
            拡張承認判定=lambda _値: True,
            拡張採用証明=lambda _旧, _新: True,
        )
        self.assertIs(判定.出力, 基準)
        self.assertTrue(判定.基準固定)
        self.assertFalse(判定.拡張採用)
        self.assertEqual(呼出, [])

    def test_一般非退行入口は証明なし拡張を昇格しない(self):
        基準 = {"状態": "SUSPEND"}
        拡張 = {"状態": "APPROVE"}
        判定 = HDS駆動コア().非退行継承実行(
            "入力",
            基準実行=lambda: 基準,
            基準承認判定=lambda _値: False,
            拡張実行=lambda: 拡張,
            拡張承認判定=lambda _値: True,
            拡張採用証明=lambda _旧, _新: False,
        )
        self.assertIs(判定.出力, 基準)
        self.assertTrue(判定.基準固定)
        self.assertFalse(判定.拡張採用)


    def test_駆動コアが要求単位の動的作用供給を保持(self):
        def 供給(状態):
            if "継承確認済み" in 状態.成立状態:
                return ()
            return (
                HDS関数作用(
                    "継承確認",
                    lambda _状態: HDS作用結果(
                        HDS作用状態.成立,
                        追加状態=frozenset({"継承確認済み"}),
                    ),
                    出力状態=("継承確認済み",),
                ),
            )

        結果 = HDS駆動コア().実行(
            "継承確認",
            要求状態=("継承確認済み",),
            追加作用供給器=(HDS作用供給器("継承確認供給", 供給),),
        )
        self.assertEqual(結果.終端, HDS終端.採用)
        self.assertEqual([x.作用ID for x in 結果.履歴], ["継承確認"])

    def test_未解共参照は入力正本に保持しAPPROVE時だけ非阻害化する(self):
        問い = "Which molecule inhibits this molecule?"
        選択肢 = ("Molecule A", "Molecule B")
        初期参照 = (
            参照記録(
                識別子="coref",
                対象="Molecule A",
                内容="Molecule A inhibits this molecule.",
                由来="試験",
                供給器="試験資料",
                信頼=1.0,
            ),
        )
        入力束 = self.構文化器.問題コア入力(問い, 選択肢)
        self.assertEqual([x.種別 for x in 入力束.残差], ["未解共参照"])

        結果 = self.コア.選択実行(問い, 選択肢, 初期参照=初期参照)
        self.assertEqual(結果.終端, HDS終端.採用, 結果.理由)
        成果 = 結果.状態.成果辞書()
        self.assertEqual(成果[回答成果名], "A")
        self.assertTrue(成果[入力残差影成果名])
        self.assertEqual([x.種別 for x in 成果["HDSコア入力"].残差], ["未解共参照"])
        self.assertFalse(any(x.startswith("HDS残差:未解共参照:") for x in 結果.状態.残差))

    def test_正式模型承認は追加観測を必須にせず基準を保持(self):
        provider = 固定追加参照((
            証拠("Molecule A", 識別子="late-a"),
            証拠("Molecule B", 否定=True, 識別子="late-b"),
        ))
        結果 = self.コア.選択実行(
            self.問い,
            self.選択肢,
            初期参照=(証拠("Molecule A", 識別子="base"),),
            参照供給器=provider,
        )
        self.assertEqual(結果.終端, HDS終端.採用, 結果.理由)
        成果 = 結果.状態.成果辞書()
        self.assertEqual(成果[回答成果名], "A")
        self.assertEqual(結果.状態.主体辞書()[基準結果主体名].回答ラベル, "A")
        self.assertEqual(provider.呼出回数, 0)
        履歴 = [x.作用ID for x in 結果.履歴]
        self.assertNotIn("HDS継承/候補関係観測", 履歴)
        self.assertIn("HDS継承/模型再評価", 履歴)

    @patch("minidora.HDS選択継承循環.HDS既存能力直接反証評価")
    def test_取得済みの2独立proofだけ承認基準を更新(self, 反証評価):
        # 成立済み回答に将来の検索を強制する試験ではない。
        # 既に新証拠を観測した状態から、証拠優越による更新を検査する。
        反証評価.return_value = HDS選択実行結果(
            "APPROVE", "B", "Molecule B", ("DIRECTED_関係_VERIFIED",),
            None, 0, 0, 0, 0, 2, 0,
        )
        初期 = (証拠("Molecule A", 識別子="base"),)
        kernel = self.構文化器.問題コンパイル束(self.問い, self.選択肢)
        供給 = HDS選択継承供給(kernel, self.構文化器, 初期)
        基準 = 供給._評価(初期)
        self.assertEqual(基準.回答ラベル, "A")
        追加 = (証拠("Molecule B", 識別子="proof-1"), 証拠("Molecule B", 識別子="proof-2"))
        状態 = HDS実行状態(
            目的=("選択回答",), 要求状態=frozenset({選択閉包状態}),
            成果=((参照成果名, (*初期, *追加)), (初回評価参照成果名, 初期)),
            主体状態=((基準結果主体名, 基準),),
        )
        結果 = 供給._評価作用(状態).実行(状態)
        成果 = dict(結果.成果)
        self.assertIn(選択閉包状態, 結果.追加状態)
        self.assertEqual(成果[回答成果名], "B")
        self.assertTrue(成果[非退行判定成果名].拡張採用)
        # 現行の主体辞書APIは内部値をdeepcopyして返す。参照同一性ではなく値と不変性を検査。
        self.assertEqual(状態.主体辞書()[基準結果主体名], 基準)
        self.assertIsNot(状態.主体辞書()[基準結果主体名], 基準)
        反証評価.assert_called_once()

    def test_第一観測層が0件でも第二層へ進み証拠を回収する(self):
        provider = 第二層追加参照()
        結果 = self.コア.選択実行(
            self.問い,
            self.選択肢,
            初期参照=(),
            参照供給器=provider,
        )
        self.assertEqual(結果.終端, HDS終端.採用, 結果.理由)
        self.assertTrue(any("inhibit" in q.casefold() for q in provider.呼出))
        self.assertTrue(any(q.casefold() in {"enzyme x molecule a", "molecule a"} for q in provider.呼出))
        self.assertEqual(結果.状態.成果辞書()[現行結果成果名].回答ラベル, "A")
        self.assertEqual(結果.状態.成果辞書()[回答成果名], "A")
        再評価 = [x for x in 結果.履歴 if x.作用ID == "HDS継承/模型再評価"]
        self.assertGreaterEqual(len(再評価), 2)

    def test_初回未閉包から候補関係観測を経て再評価で閉包(self):
        provider = 固定追加参照((
            証拠("Molecule A", 識別子="extra-a"),
            証拠("Molecule B", 否定=True, 識別子="extra-b"),
        ))
        結果 = self.コア.選択実行(
            self.問い,
            self.選択肢,
            初期参照=(),
            参照供給器=provider,
        )
        self.assertEqual(結果.終端, HDS終端.採用, 結果.理由)
        成果 = 結果.状態.成果辞書()
        self.assertEqual(成果[回答成果名], "A")
        self.assertGreaterEqual(provider.呼出回数, 1)
        self.assertGreaterEqual(int(成果[関係観測世代成果名]), 1)
        履歴 = [x.作用ID for x in 結果.履歴]
        self.assertIn("HDS継承/候補関係観測", 履歴)
        self.assertIn("HDS継承/模型再評価", 履歴)
        self.assertLess(履歴.index("HDS継承/模型再評価"), 履歴.index("HDS継承/候補関係観測"))

    def test_後続観測で閉包しても証明なし拡張は影結果に留める(self):
        provider = 第二層完全参照()
        結果 = self.コア.選択実行(
            self.問い,
            self.選択肢,
            初期参照=(),
            参照供給器=provider,
            拡張採用証明=lambda _前, _後: False,
        )
        self.assertEqual(結果.終端, HDS終端.保留)
        成果 = 結果.状態.成果辞書()
        self.assertNotIn(回答成果名, 成果)
        self.assertIn(影結果成果名, 成果)
        self.assertTrue(成果[非退行判定成果名].基準固定)
        self.assertFalse(成果[非退行判定成果名].拡張採用)

    def test_Kernel候補意味正本は再評価で再コンパイルしない(self):
        kernel = self.構文化器.問題コンパイル束(self.問い, self.選択肢)
        original = self.構文化器.コンパイル
        候補表層 = set(self.選択肢)

        def 候補再コンパイル禁止(入力, **kwargs):
            if str(入力) in 候補表層:
                raise AssertionError("Kernel形成済み候補を下流で再コンパイルした")
            return original(入力, **kwargs)

        self.構文化器.コンパイル = 候補再コンパイル禁止
        try:
            結果 = self.コア.選択実行(
                self.問い,
                self.選択肢,
                初期参照=(証拠("Molecule A", 識別子="candidate-kernel"),),
                カーネル正本=kernel,
            )
        finally:
            self.構文化器.コンパイル = original
        self.assertEqual(結果.終端, HDS終端.採用, 結果.理由)

    def test_外部で形成済みKernel正本は中核内で再コンパイルしない(self):
        kernel = self.構文化器.問題コンパイル束(self.問い, self.選択肢)
        original = self.構文化器.問題コンパイル束

        def 再コンパイル禁止(*_args, **_kwargs):
            raise AssertionError("形成済みKernel正本をCore内で再コンパイルした")

        self.構文化器.問題コンパイル束 = 再コンパイル禁止
        try:
            結果 = self.コア.選択実行(
                self.問い,
                self.選択肢,
                初期参照=(証拠("Molecule A", 識別子="kernel-fixed"),),
                カーネル正本=kernel,
            )
        finally:
            self.構文化器.問題コンパイル束 = original

        self.assertEqual(結果.終端, HDS終端.採用, 結果.理由)
        self.assertEqual(結果.状態.主体辞書()["HDSカーネル署名"], kernel.カーネル署名)

    def test_LIVE参照で根拠なしAPPROVEはCOMMITしない(self):
        provider = 固定追加参照(())
        根拠なし = HDS選択実行結果(
            "APPROVE", "A", "Molecule A", ("UNPROVEN_TEST_APPROVAL",),
            None, 2, 0, 0, 0, 0, 0,
        )
        with patch.object(HDS選択継承供給, "_評価", return_value=根拠なし):
            結果 = self.コア.選択実行(
                self.問い,
                self.選択肢,
                初期参照=(),
                参照供給器=provider,
                最大回復回数=2,
            )
        self.assertEqual(結果.終端, HDS終端.保留)
        self.assertNotIn(回答成果名, 結果.状態.成果辞書())
        self.assertIn(残差_証明不足, 結果.状態.残差)

    def test_追加参照なしは推測せず保留(self):
        結果 = self.コア.選択実行(self.問い, self.選択肢, 初期参照=())
        self.assertEqual(結果.終端, HDS終端.保留)
        成果 = 結果.状態.成果辞書()
        self.assertNotIn(回答成果名, 成果)
        self.assertEqual(結果.状態.主体辞書()[基準結果主体名].状態, "SUSPEND")


if __name__ == "__main__":
    unittest.main()

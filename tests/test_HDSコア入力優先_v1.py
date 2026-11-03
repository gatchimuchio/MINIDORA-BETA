from __future__ import annotations

import unittest

from minidora.HDSコア入力 import HDSコア入力束
from minidora.HDSコア入力射影 import HDSコア入力へ
from minidora.HDS中間表現 import HDSIR, HDS実行核, HDS座標, HDS関係
from minidora.HDS適合器 import HDS独立コア入力コンパイル
from minidora.HDS構文化作用 import HDS構文化作用
from minidora.HDS構文化器_v1 import 公開HDSコンパイラ
from minidora.HDS実行主体 import HDS実行状態, HDS作用状態, HDS終端
from minidora.HDS駆動コア import HDS駆動コア


class HDSコア入力優先試験(unittest.TestCase):
    def setUp(self) -> None:
        self.構文化器 = 公開HDSコンパイラ()

    def test_コア入力が構文化器の正本である(self):
        入力束 = self.構文化器.コア入力コンパイル("A causes B")
        self.assertIsInstance(入力束, HDSコア入力束)
        self.assertTrue(入力束.コア需要を検査())
        for 名 in ("手順", "計算計画", "作用差分構造", "意味作用履歴", "初期状態"):
            self.assertFalse(hasattr(入力束, 名), 名)

    def test_コア入力はKernel束の射影をそのまま返し別意味経路を持たない(self):
        束 = self.構文化器.コンパイル束("A causes B")

        class 計画禁止:
            def 計画(self, *args, **kwargs):
                raise AssertionError("Core入力取得で第二の計画経路を起動した")

        def 監査禁止(*args, **kwargs):
            raise AssertionError("Core入力取得で第二の監査経路を起動した")

        self.構文化器._計算計画器 = 計画禁止()
        self.構文化器._完成 = 監査禁止
        self.構文化器.コンパイル束 = lambda _入力, **_kwargs: 束

        入力束 = self.構文化器.コア入力コンパイル("A causes B")
        self.assertIs(入力束, 束.コア入力)
        self.assertEqual(入力束.意味署名, 束.コア入力.意味署名)

    def test_コンパイル束はコア入力を正本としてLegacy成果を分離する(self):
        束 = self.構文化器.コンパイル束("2+3")
        self.assertIs(束.正本, 束.コア入力)
        self.assertIsNone(束.意味IR.手順)
        self.assertTrue(束.計算計画.手順.命令列)
        self.assertFalse(hasattr(束.正本, "計算計画"))

    def test_監査座標はコア入力へ混入しない(self):
        詳細 = self.構文化器.詳細コンパイル("A causes B")
        self.assertTrue(any(x.種別.startswith("監査.") for x in 詳細.IR.座標))
        入力束 = self.構文化器.コア入力コンパイル("A causes B")
        self.assertFalse(any(x.種別.startswith(("監査.", "保持.", "暫定性.", "帰還.")) for x in 入力束.意味項目))

    def test_意味条件と関係制約を混同せず能力名へ変換しない(self):
        入力束 = self.構文化器.コア入力コンパイル("If A causes B, B increases C.")
        self.assertTrue(入力束.関係)
        self.assertTrue(入力束.条件)
        self.assertTrue(all(x.種別 for x in 入力束.条件))
        self.assertTrue(any(any(v.startswith("検索述語=") for v in x.制約) for x in 入力束.関係))
        self.assertFalse(any(any(v.startswith("検索述語=") for v in (str(x.内容),)) for x in 入力束.条件))
        self.assertEqual(入力束.作用要求, ())
        self.assertEqual(入力束.要求成果, ())

    def test_意味条件を関係へ明示bindingする(self):
        ir = HDSIR(
            原文="A causes B under hypoxia",
            正規化文="A causes B under hypoxia",
            認知世界ID="condition-binding",
            座標=(
                HDS座標("a", "対象.始点", "A"),
                HDS座標("b", "対象.終点", "B"),
                HDS座標("c", "条件.前提", "under hypoxia"),
            ),
            関係=(
                HDS関係(
                    "r", ("a",), ("b",), "因果",
                    条件=("検索述語=causes", "条件範囲=under hypoxia"),
                ),
            ),
            残差=(),
            意味作用履歴=(),
            実行核=HDS実行核(),
            入力言語="en",
        )
        入力束 = HDSコア入力へ(ir)
        self.assertEqual(入力束.関係[0].条件ID, ("c",))
        self.assertEqual(入力束.条件[0].適用先, ("r",))
        self.assertIn("検索述語=causes", 入力束.関係[0].制約)
        self.assertNotIn("検索述語", str(入力束.条件[0].内容))

    def test_選択候補を作用要求や完了条件へ昇格しない(self):
        入力束 = self.構文化器.問題コア入力("Which is correct?", ("A", "B", "C"))
        self.assertTrue(any(x.種別 == "候補" for x in 入力束.目的))
        self.assertEqual(入力束.作用要求, ())
        self.assertEqual(入力束.要求成果, ())

    def test_明示作用要求を能力名なしで供給する(self):
        入力束 = self.構文化器.コア入力コンパイル("本文を要約してください。")
        self.assertEqual(tuple(x.種別 for x in 入力束.作用要求), ("要約",))
        self.assertEqual(入力束.要求成果, ("要約結果",))
        self.assertFalse(any(hasattr(x, "能力") for x in 入力束.作用要求))

    def test_未知命令形を黙って捨てない(self):
        入力束 = self.構文化器.コア入力コンパイル("本文を未知操作してください。")
        self.assertTrue(any(x.種別 == "作用要求未構文化" for x in 入力束.残差))
        self.assertEqual(入力束.作用要求, ())

    def test_説明文中の英語動詞を要求へ誤認しない(self):
        入力束 = self.構文化器.コア入力コンパイル("We compare A and B.")
        self.assertEqual(入力束.作用要求, ())

    def test_独立コア入力コンパイルは会話文脈を混入しない(self):
        入力束 = HDS独立コア入力コンパイル(self.構文化器, "A inhibits B")
        self.assertIsInstance(入力束, HDSコア入力束)
        self.assertEqual(入力束.文脈引用, ())
        self.assertTrue(any(x.種別 == "阻害" for x in 入力束.関係))

    def test_構文化作用はコア正本入口を旧経路より先に使う(self):
        作用 = HDS構文化作用(self.構文化器, "A causes B")
        結果 = 作用.実行(HDS実行状態())
        self.assertEqual(結果.状態, HDS作用状態.成立)
        成果 = dict(結果.成果)
        self.assertIsInstance(成果["HDSコア入力"], HDSコア入力束)
        self.assertNotIn("HDS_IR", 成果)
        self.assertEqual(dict(結果.主体状態差分)["HDSコア入力署名"], 成果["HDSコア入力"].意味署名)

    def test_HDS駆動コアはコア入力を実行開始前に受け取る(self):
        コア = HDS駆動コア(HDSコンパイラ=self.構文化器, 最大作用回数=2)
        結果 = コア.実行(
            "A causes B",
            目的=("Core入力接続",),
            要求状態=("HDSコア入力済み",),
        )
        self.assertEqual(結果.終端, HDS終端.採用)
        self.assertEqual(結果.履歴, ())
        self.assertIn("HDSコア入力", 結果.状態.成果辞書())
        self.assertIn("HDSコア入力済み", 結果.状態.成立状態)

    def test_表現要求と実行制約をコア責任へ分離する(self):
        入力束 = self.構文化器.コア入力コンパイル(
            "提供資料だけで、簡潔に本文をJSONに変換してください。"
        )
        self.assertIn(("形式", "JSON"), {(x.種別, x.値) for x in 入力束.表現制約.要求})
        self.assertIn(("詳細度", "簡潔"), {(x.種別, x.値) for x in 入力束.表現制約.要求})
        self.assertIn(("資料範囲", "提供資料のみ"), {(x.種別, x.値) for x in 入力束.実行制約})

    def test_参照必須は表現ではなく実行制約である(self):
        入力束 = self.構文化器.問題コア入力("Which is correct?", ("A", "B", "C"))
        self.assertIn(("参照", "必須"), {(x.種別, x.値) for x in 入力束.実行制約})
        self.assertFalse(any(x.種別 == "参照" for x in 入力束.表現制約.要求))

    def test_出力言語競合残差にも検証要求が対応する(self):
        入力束 = self.構文化器.コア入力コンパイル(
            "日本語で英語で本文を要約してください。"
        )
        対象 = [x for x in 入力束.残差 if x.ID == "Core入力:出力言語競合"]
        self.assertEqual(len(対象), 1)
        self.assertIn(
            "検証:Core入力:出力言語競合",
            {x.ID for x in 入力束.検証要求},
        )

    def test_コア初期目的索引は任意内容の文字列表現へ依存しない(self):
        from minidora.HDSコア入力 import HDSコア目的, HDSコア表現制約

        class 固定コア入力構文化器:
            def コア入力コンパイル(self, 入力, **kwargs):
                return HDSコア入力束(
                    原文=入力,
                    認知世界ID="試験",
                    意味項目=(),
                    関係=(),
                    条件=(),
                    目的=(HDSコア目的("目的:任意", "比較", ("文字列化しない", 1)),),
                    作用要求=(),
                    要求成果=(),
                    残差=(),
                    検証要求=(),
                    実行制約=(),
                    表現制約=HDSコア表現制約("ja"),
                )

        コア = HDS駆動コア(HDSコンパイラ=固定コア入力構文化器(), 最大作用回数=2)
        結果 = コア.実行(
            "入力",
            目的=("試験",),
            要求状態=("HDSコア入力済み",),
        )
        self.assertEqual(結果.終端, HDS終端.採用)
        self.assertIn("HDS目的:目的:任意:比較", 結果.状態.目的)
        self.assertFalse(any("文字列化しない" in x for x in 結果.状態.目的))
        self.assertEqual(結果.状態.主体辞書()["HDS目的正本"], tuple(結果.状態.成果辞書()["HDSコア入力"].目的))
        self.assertEqual(結果.状態.主体辞書()["HDS目的正本"][0].内容, ("文字列化しない", 1))

    def test_コア入力からC7用の経験効果を捏造しない(self):
        入力束 = self.構文化器.コア入力コンパイル("A causes B")
        使用責任 = {責任 for _, 責任群 in 入力束.消費責任 for 責任 in 責任群}
        self.assertNotIn("C7", 使用責任)
        self.assertFalse(hasattr(入力束, "期待効果"))


if __name__ == "__main__":
    unittest.main()

from __future__ import annotations

import unittest

from minidora.HDSコア入力 import HDSコア入力束, HDSコア表現制約
from minidora.HDS中間表現 import HDSIR, HDS実行核, HDS座標, HDS関係, 値状態
from minidora.HDS駆動コア import HDS駆動コア版, HDS継承基準版
from minidora.実行系_HDS_v1 import HDS駆動ミニドラ
from minidora.実行系_v03 import 要求
from minidora.参照 import 参照記録
from minidora.採否 import 実行状態


def _ir(text, coords, relations=(), *, required=False):
    return HDSIR(
        原文=text, 正規化文=text, 認知世界ID="hds-v1-統合",
        座標=coords, 関係=relations, 残差=(), 意味作用履歴=(),
        実行核=HDS実行核("意味構造転送"), 参照必須=required,
        種別="knowledge_選択肢", 閉包状態="CLOSED_FOR_意味_TRANSFER",
        入力言語="en", 手順=None,
    )


def _question():
    return _ir(
        "What does Alpha use?",
        (
            HDS座標("alpha", "対象.実体", "Alpha", 原文範囲=(10, 15)),
            HDS座標("use", "関係.述語表層", "use", 原文範囲=(16, 19)),
            HDS座標("選択肢:A", "目的.候補", "engine"),
            HDS座標("選択肢:B", "目的.候補", "stone"),
            HDS座標("未知", "目的.未知終点", "entity", 値状態.未観測),
        ),
        (HDS関係(
            "question-use", ("alpha",), ("未知",), "使用",
            条件=("検索述語=use", "不足位置=終点", "英日意味射影=v0.5"),
            値状態=値状態.未観測,
        ),),
        required=True,
    )


def _候補(text):
    return _ir(text, (HDS座標("候補", "対象.実体", text, 原文範囲=(0, len(text))),))


def _資料():
    return _ir(
        "Alpha uses engine.",
        (
            HDS座標("alpha", "対象.実体", "Alpha", 原文範囲=(0, 5)),
            HDS座標("engine", "対象.実体", "engine", 原文範囲=(11, 17)),
        ),
        (HDS関係("use", ("alpha",), ("engine",), "使用", 条件=("検索述語=use",)),),
    )


class 構文化器:
    def コンパイル(self, 入力: str, **kwargs):
        if 入力 == "What does Alpha use?":
            return _question()
        if 入力 in {"engine", "stone"}:
            return _候補(入力)
        if 入力 == "Alpha uses engine.":
            return _資料()
        raise ValueError(入力)

    def 問題IR(self, 入力: str, 選択肢):
        if 入力 != "What does Alpha use?" or tuple(選択肢) != ("engine", "stone"):
            raise ValueError((入力, tuple(選択肢)))
        return _question()

    def 問題コア入力(self, 入力: str, 選択肢):
        if 入力 != "What does Alpha use?" or tuple(選択肢) != ("engine", "stone"):
            raise ValueError((入力, tuple(選択肢)))
        return HDSコア入力束(
            原文=入力,
            認知世界ID="hds-v1-統合",
            意味項目=(),
            関係=(),
            条件=(),
            目的=(),
            作用要求=(),
            要求成果=("候補選択",),
            残差=(),
            検証要求=(),
            実行制約=(),
            表現制約=HDSコア表現制約("en"),
        )


class Provider:
    名称 = "fixture-R"

    def 検索(self, 問合せ: str, 上限: int = 8):
        return (参照記録("doc:1", "Alpha", "Alpha uses engine.", "fixture://doc1", "fixture"),)


class 実行系HDSV1試験(unittest.TestCase):
    def test_実実行系がHDS正本継承循環でCOMMITする(self):
        実行系 = HDS駆動ミニドラ(Provider(), HDSコンパイラ_=構文化器())
        実行結果 = 実行系.実行(要求("What does Alpha use?"))

        self.assertEqual(実行結果.採否.状態, 実行状態.合格, 実行結果.採否.理由)
        self.assertEqual(実行結果.値, "engine")
        実行記録 = 実行結果.状態["HDS駆動コアRun"]
        self.assertEqual(実行記録["終端"], "COMMIT")
        self.assertEqual(実行記録["コア版"], HDS駆動コア版)
        self.assertEqual(実行記録["継承基準"], HDS継承基準版)
        作用履歴 = 実行記録["作用履歴"]
        self.assertIn("HDS継承/模型再評価", 作用履歴)
        if "HDS継承/候補関係観測" in 作用履歴:
            self.assertLess(
                作用履歴.index("HDS継承/候補関係観測"),
                作用履歴.index("HDS継承/模型再評価"),
            )
        self.assertIn("HDS_FIRST_CORE_COMMIT", 実行結果.採否.理由)
        self.assertEqual(実行結果.履歴[-1]["op"], "HDS_FIRST_CORE_INHERITANCE_RUN")

    def test_現行HDS実行系は旧prototype統合器をimportしない(self):
        import inspect
        import minidora.実行系_HDS_v1 as module

        原文 = inspect.getsource(module)
        self.assertNotIn("HDS統合実行系", 原文)
        self.assertNotIn("HDS駆動選択実行", 原文)
        self.assertNotIn("HDS判断主体Run", 原文)
        self.assertIn("HDS駆動コア", 原文)
        self.assertIn("選択実行", 原文)


if __name__ == "__main__":
    unittest.main()

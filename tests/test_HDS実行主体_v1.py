from __future__ import annotations

from dataclasses import dataclass
import unittest

from minidora.HDS実行主体 import (
    HDS作用結果,
    HDS作用状態,
    HDS実行主体,
    HDS実行状態,
    HDS終端,
    HDS関数作用,
)
from minidora.HDS駆動コア import HDS駆動コア
from minidora.HDS中間表現 import HDSIR, HDS実行核
from minidora.HDS計画作用 import HDS目的計画作用, HDS能力合成作用
from minidora.能力合成 import 能力合成器, 合成工程, 合成計画, 登録能力, 素材参照
from minidora.製品版.能力契約 import 能力文脈
from minidora.製品版.能力レジストリ import 能力レジストリ
from minidora.製品版.型 import 能力結果


class _能力:
    def __init__(self, 名前: str, 判定値: float, 優先度: int = 0) -> None:
        self.名前 = 名前
        self.版 = "v1"
        self.優先度 = 優先度
        self.判定値 = 判定値
        self.呼出回数 = 0

    def 判定(self, 文脈):
        return self.判定値

    def 実行(self, 文脈):
        self.呼出回数 += 1
        return 能力結果(True, f"{self.名前}:{文脈.入力文}:{文脈.直前応答}")


class _状態依存能力(_能力):
    def 判定(self, 文脈):
        return 1.0 if 文脈.入力文 == "状態から生成" else 0.0


class _構文化器:
    def コンパイル(self, 入力: str, **kwargs):
        return HDSIR(
            原文=入力,
            正規化文=入力,
            認知世界ID="試験",
            座標=(),
            関係=(),
            残差=(),
            意味作用履歴=(),
            実行核=HDS実行核(),
        )


@dataclass(frozen=True)
class _計画結果:
    状態: str
    計画: object
    資料: dict
    理由: str = ""

    @property
    def 成立(self):
        return self.状態 == "合格" and self.計画 is not None


class _計画器:
    def __init__(self, 計画, 資料):
        self.計画 = 計画
        self.資料 = 資料
        self.呼出回数 = 0

    def 計画する(self, 要求, *, 禁止作用=()):
        self.呼出回数 += 1
        return _計画結果("合格", self.計画, self.資料)


class HDS実行主体試験(unittest.TestCase):
    def test_局所作用成功は全体採用ではない(self):
        作用 = HDS関数作用(
            "局所処理",
            lambda 状態: HDS作用結果(
                HDS作用状態.成立,
                追加状態=frozenset({"局所成立"}),
            ),
            出力状態=("局所成立",),
            目的依存=("状態:最終成立",),
        )
        初期 = HDS実行状態(要求状態=frozenset({"最終成立"}))
        結果 = HDS実行主体((作用,), 最大作用回数=4).実行(初期)
        self.assertEqual(結果.終端, HDS終端.保留)
        self.assertIn("局所成立", 結果.状態.成立状態)
        self.assertNotIn("最終成立", 結果.状態.成立状態)

    def test_残差解消を無関係な高優先度作用より先に選ぶ(self):
        順序 = []
        無関係 = HDS関数作用(
            "高優先度だが無関係",
            lambda 状態: (
                順序.append("無関係")
                or HDS作用結果(HDS作用状態.成立, 追加状態=frozenset({"装飾"}))
            ),
            出力状態=("装飾",),
            優先度=100.0,
        )
        参照 = HDS関数作用(
            "参照取得",
            lambda 状態: (
                順序.append("参照")
                or HDS作用結果(
                    HDS作用状態.成立,
                    追加状態=frozenset({"資料あり"}),
                    解消残差=frozenset({"観測不足"}),
                )
            ),
            出力状態=("資料あり",),
            解消対象=("観測不足",),
        )
        完了 = HDS関数作用(
            "回答形成",
            lambda 状態: (
                順序.append("完了")
                or HDS作用結果(HDS作用状態.成立, 追加状態=frozenset({"回答済み"}))
            ),
            入力状態=("資料あり",),
            出力状態=("回答済み",),
        )
        初期 = HDS実行状態(
            要求状態=frozenset({"回答済み"}),
            残差=frozenset({"観測不足"}),
        )
        結果 = HDS実行主体((無関係, 参照, 完了), 最大作用回数=6).実行(初期)
        self.assertEqual(結果.終端, HDS終端.採用)
        self.assertEqual(順序[:2], ["参照", "完了"])
        self.assertNotIn("無関係", 順序)

    def test_同一作用同一入力を反復しない(self):
        呼出 = []
        作用 = HDS関数作用(
            "無進展",
            lambda 状態: 呼出.append(状態.状態署名) or HDS作用結果(HDS作用状態.保留),
            目的依存=("状態:未達",),
        )
        初期 = HDS実行状態(要求状態=frozenset({"未達"}))
        結果 = HDS実行主体((作用,), 最大作用回数=8).実行(初期)
        self.assertEqual(結果.終端, HDS終端.保留)
        self.assertEqual(len(呼出), 1)
        self.assertEqual(len(結果.履歴), 1)
        self.assertFalse(結果.履歴[0].状態差.変化有無)

    def test_実状態差後は同じ作用を再利用できる(self):
        def 実行(状態):
            if "段階1" not in 状態.成立状態:
                return HDS作用結果(HDS作用状態.成立, 追加状態=frozenset({"段階1"}))
            return HDS作用結果(HDS作用状態.成立, 追加状態=frozenset({"段階2"}))

        作用 = HDS関数作用(
            "段階作用",
            実行,
            出力状態=("段階1", "段階2"),
            機会判定=lambda 状態: "段階2" not in 状態.成立状態,
        )
        初期 = HDS実行状態(要求状態=frozenset({"段階2"}))
        結果 = HDS実行主体((作用,), 最大作用回数=4).実行(初期)
        self.assertEqual(結果.終端, HDS終端.採用)
        self.assertEqual([行.作用ID for 行 in 結果.履歴], ["段階作用", "段階作用"])
        self.assertTrue(all(行.状態差.変化有無 for 行 in 結果.履歴))

    def test_主体状態差をHDS状態が所有する(self):
        作用 = HDS関数作用(
            "主体更新",
            lambda 状態: HDS作用結果(
                HDS作用状態.成立,
                解消残差=frozenset({"主体更新待ち"}),
                主体状態差分=(("現在目的", "検証"),),
            ),
            解消対象=("主体更新待ち",),
        )
        初期 = HDS実行状態(
            残差=frozenset({"主体更新待ち"}),
            主体状態=(("現在目的", "未設定"),),
        )
        結果 = HDS実行主体((作用,), 最大作用回数=2).実行(初期)
        self.assertEqual(結果.終端, HDS終端.採用)
        self.assertEqual(結果.状態.主体辞書()["現在目的"], "検証")
        self.assertIn("現在目的", 結果.履歴[0].状態差.変更主体状態)

    def test_能力登録簿は全候補を公開し旧選択も維持する(self):
        能力A = _能力("A", 0.8, 1)
        能力B = _能力("B", 0.9, 0)
        登録簿 = 能力レジストリ((能力A, 能力B))
        文脈 = 能力文脈("入力", "試験")
        候補 = 登録簿.候補群(文脈)
        self.assertEqual([項.モジュール.名前 for 項 in 候補], ["B", "A"])
        選択 = 登録簿.選択(文脈)
        self.assertIsNotNone(選択)
        self.assertEqual(選択.モジュール.名前, "B")
        作用群 = 登録簿.HDS作用群(
            文脈,
            出力状態={"A": ("A完了",), "B": ("B完了",)},
        )
        self.assertEqual(len(作用群), 2)

    def test_能力成功をHDS目的閉包時だけ採用する(self):
        能力 = _能力("答える", 0.9)
        登録簿 = 能力レジストリ((能力,))
        文脈 = 能力文脈("質問", "試験")
        作用群 = 登録簿.HDS作用群(文脈, 出力状態={"答える": ("回答済み",)})
        初期 = HDS実行状態(要求状態=frozenset({"回答済み"}))
        結果 = HDS実行主体(作用群, 最大作用回数=4).実行(初期)
        self.assertEqual(結果.終端, HDS終端.採用)
        self.assertEqual(能力.呼出回数, 1)
        self.assertIn("能力結果:答える", 結果.状態.成果辞書())

    def test_HDS状態から能力文脈を動的生成する(self):
        能力 = _状態依存能力("状態能力", 0.0)
        登録簿 = 能力レジストリ((能力,))
        作用群 = 登録簿.HDS作用群(
            文脈生成=lambda 状態: 能力文脈(
                "状態から生成" if "前段完了" in 状態.成立状態 else "未成立",
                "試験",
            ),
            出力状態={"状態能力": ("後段完了",)},
            入力状態={"状態能力": ("前段完了",)},
        )
        前段 = HDS関数作用(
            "前段",
            lambda 状態: HDS作用結果(HDS作用状態.成立, 追加状態=frozenset({"前段完了"})),
            出力状態=("前段完了",),
        )
        初期 = HDS実行状態(要求状態=frozenset({"後段完了"}))
        結果 = HDS実行主体((前段, *作用群), 最大作用回数=4).実行(初期)
        self.assertEqual(結果.終端, HDS終端.採用)
        self.assertEqual(能力.呼出回数, 1)

    def test_HDS駆動コアは構文化を知覚作用として扱う(self):
        コア = HDS駆動コア(HDSコンパイラ=_構文化器(), 最大作用回数=3)
        結果 = コア.実行(
            "入力",
            目的=("意味構文化",),
            要求状態=("HDS意味構文化済み",),
        )
        self.assertEqual(結果.終端, HDS終端.採用)
        self.assertEqual([行.作用ID for 行 in 結果.履歴], ["HDS構文化"])
        self.assertIn("HDS_IR", 結果.状態.成果辞書())

    def test_HDS駆動コアは完了条件なしを拒否する(self):
        コア = HDS駆動コア(HDSコンパイラ=_構文化器(), 最大作用回数=3)
        with self.assertRaises(ValueError):
            コア.実行("入力", 目的=("回答",))

    def test_目的計画作用の署名は計画器objectの住所へ依存しない(self):
        計画 = 合成計画((合成工程("工程", ("複写",), "指示"),), ("工程",))
        資料 = {"指示": 能力結果(True, "指示")}
        左 = HDS目的計画作用(_計画器(計画, 資料), "要求")
        右 = HDS目的計画作用(_計画器(計画, 資料), "要求")
        状態 = HDS実行状態(要求状態=frozenset({"目的計画済み"}))
        self.assertEqual(左.機会(状態).作用入力署名, 右.機会(状態).作用入力署名)

    def test_目的計画と能力合成をHDS途中作用として連結する(self):
        能力 = _能力("複写", 1.0)
        合成器 = 能力合成器((登録能力(能力),))
        計画 = 合成計画(
            (
                合成工程(
                    "工程1",
                    ("複写",),
                    "指示",
                    (素材参照("入力", "本文"),),
                ),
            ),
            ("工程1",),
        )
        資料 = {
            "指示": 能力結果(True, "そのまま返す"),
            "本文": 能力結果(True, "材料"),
        }
        計画器 = _計画器(計画, 資料)
        計画作用 = HDS目的計画作用(計画器, "要求")
        合成作用 = HDS能力合成作用(合成器)
        初期 = HDS実行状態(
            目的=("計画して実行",),
            要求状態=frozenset({"能力合成済み"}),
            残差=frozenset({"計画未形成", "実行未完了"}),
        )
        結果 = HDS実行主体((計画作用, 合成作用), 最大作用回数=4).実行(初期)
        self.assertEqual(結果.終端, HDS終端.採用)
        self.assertEqual([行.作用ID for 行 in 結果.履歴], ["目的計画", "能力合成"])
        self.assertEqual(計画器.呼出回数, 1)
        self.assertEqual(能力.呼出回数, 1)
        self.assertIn("能力合成結果", 結果.状態.成果辞書())
        self.assertIn("合成出力:工程1", 結果.状態.成果辞書())


if __name__ == "__main__":
    unittest.main()

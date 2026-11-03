from __future__ import annotations

from dataclasses import dataclass
import unittest

from minidora.コア import (
    束縛を試す,
    具体化する,
    意味座標,
    条件項,
    条件状態,
    条件集合を判定,
    前方閉包,
    条件付き前方閉包,
    内容単位,
    内容計画,
    内容計画を検査,
    内容計画を表現,
    検証器群を実行,
    標準能力境界,
    能力境界を取得,
    能力分類集計,
    状態を読む,
    状態差を受理,
)
from minidora.HDS実行主体 import (
    HDS実行状態,
    HDS作用結果,
    HDS作用状態,
    HDS関数作用,
    HDS作用機会,
    標準HDS作用選択器,
)
from minidora.統合駆動_v2.適応記憶 import HDS適応記憶
from minidora.統合駆動_v2.計画 import HDS作用仕様
from minidora.統合駆動_v2.循環 import _期待を計画仕様へ反映
from minidora.コア.効果 import 期待効果


@dataclass(frozen=True)
class 命題:
    対象: str
    関係: str
    値: object
    範囲: str = "未指定"
    時点: str = "未指定"


@dataclass(frozen=True)
class 規則:
    前提: tuple[命題, ...]
    結論: 命題


class MINIDORAコア完成試験(unittest.TestCase):
    def test_01_コアは下位統合パッケージを先に初期化せずimportできる(self):
        # import成功そのものが依存方向の回帰試験。
        self.assertEqual(意味座標("A", "状態").対象, "A")

    def test_02_標準67入口が全て境界分類される(self):
        self.assertEqual(len(標準能力境界), 67)
        self.assertEqual(sum(n for _, n in 能力分類集計()), 67)
        self.assertEqual(len({x.名前 for x in 標準能力境界}), 67)

    def test_03_外部読取4入口だけを外部境界として識別する(self):
        actual = {x.名前 for x in 標準能力境界 if x.外部読取}
        self.assertEqual(actual, {"知識取得", "ブラウザ閲覧", "会話取得報告", "関係不足資料取得"})

    def test_04_未分類能力を黙ってモジュール扱いしない(self):
        with self.assertRaises(ValueError):
            能力境界を取得("未知能力")

    def test_05_対象関係値範囲時点を共通束縛する(self):
        pattern = 命題("?対象", "?関係", "?値", "?範囲", "?時点")
        actual = 命題("装置A", "状態", "稼働", "設備", "現在")
        env = 束縛を試す(pattern, actual)
        self.assertEqual(env["?対象"], "装置A")
        self.assertEqual(具体化する(pattern, env), actual)

    def test_06_矛盾する再束縛を拒否する(self):
        pattern = 命題("?x", "接続", "?x")
        self.assertIsNone(束縛を試す(pattern, 命題("A", "接続", "B")))

    def test_07_未束縛変数を勝手に補完しない(self):
        self.assertIsNone(具体化する(命題("?x", "状態", "?v"), {"?x": "A"}))

    def test_08_条件は三値で未記載を偽にしない(self):
        item = 条件項("c1", 命題("A", "有効", True))
        self.assertEqual(条件集合を判定((item,), (), ()).状態, 条件状態.未確定)
        self.assertEqual(条件集合を判定((item,), (item.命題,), ()).状態, 条件状態.成立)
        self.assertEqual(条件集合を判定((item,), (), (item.命題,)).状態, 条件状態.不成立)

    def test_09_否定条件も閉世界仮定を使わない(self):
        item = 条件項("c1", 命題("A", "故障", True), 期待=False)
        self.assertEqual(条件集合を判定((item,), (), ()).状態, 条件状態.未確定)
        self.assertEqual(条件集合を判定((item,), (), (item.命題,)).状態, 条件状態.成立)

    def test_10_共通関係閉包は前向きだけを有限に導出する(self):
        a = 命題("A", "有効", True)
        b = 命題("A", "稼働", True)
        rule = 規則((命題("?x", "有効", True),), 命題("?x", "稼働", True))
        world = 前方閉包((a,), (rule,), 最大事実数=8)
        self.assertIn(b, world)
        reverse = 前方閉包((b,), (rule,), 最大事実数=8)
        self.assertNotIn(a, reverse)

    def test_11_関係閉包は予算超過を切捨て成功にしない(self):
        rules = tuple(規則((命題("?x", f"r{i}", True),), 命題("?x", f"r{i+1}", True)) for i in range(12))
        with self.assertRaises(ValueError):
            前方閉包((命題("A", "r0", True),), rules, 最大事実数=4, 最大照合数=200)

    def test_12_内容計画は根拠条件留保を表現前に保持する(self):
        計画値 = 内容計画((内容単位("結論", "Aです。", ("成果:1",), ("条件：C",)),), ("留保：D",), ("資料X",))
        self.assertTrue(内容計画を検査(計画値, 許可根拠={"成果:1"}))
        self.assertFalse(内容計画を検査(計画値, 許可根拠={"成果:2"}))
        表示文 = 内容計画を表現(計画値, 詳細=True)
        self.assertIn("条件：C", 表示文)
        self.assertIn("留保：D", 表示文)
        self.assertIn("資料X", 表示文)

    def test_13_検証器は対象も書換えられない(self):
        class V:
            ID = "v1"
            版 = "1"
            def 検証(self, 状態, 対象):
                対象["x"] = 2
                return True
        with self.assertRaises(ValueError):
            検証器群を実行(HDS実行状態(), (V(),), {"x": 1})

    def test_14_全体状態はコア境界から読み状態差を受理する(self):
        状態 = HDS実行状態(目的=("回答",), 要求状態=frozenset({"完了"}), 残差=frozenset({"未完"}))
        状態表示 = 状態を読む(状態)
        self.assertEqual(状態表示.未達状態, frozenset({"完了"}))
        新状態, 状態差 = 状態差を受理(状態, HDS作用結果(HDS作用状態.成立, 追加状態=frozenset({"完了"}), 解消残差=frozenset({"未完"})))
        self.assertIn("完了", 新状態.成立状態)
        self.assertTrue(状態差.変化有無)

    def test_15_動的呼出住所と作用定義を分ける(self):
        left = HDS関数作用("運用能力/p1/s1/数量比較", lambda s: HDS作用結果(HDS作用状態.保留))
        right = HDS関数作用("運用能力/p2/s9/数量比較", lambda s: HDS作用結果(HDS作用状態.保留))
        self.assertNotEqual(left.作用ID, right.作用ID)
        self.assertEqual(left.作用定義ID, right.作用定義ID)
        self.assertEqual(left.作用定義ID, "運用能力/数量比較")

    def test_16_同一定義でも意味入力が違えば経験を共有しない(self):
        状態1 = HDS実行状態(成果=(("資料", {"値": 1}),))
        状態2 = HDS実行状態(成果=(("資料", {"値": 2}),))
        a1 = HDS関数作用("運用能力/p1/s1/数量比較", lambda s: HDS作用結果(HDS作用状態.保留), 読取成果=("資料",))
        a2 = HDS関数作用("運用能力/p2/s9/数量比較", lambda s: HDS作用結果(HDS作用状態.保留), 読取成果=("資料",))
        self.assertEqual(a1.機会(状態1).意味入力署名, a2.機会(状態1).意味入力署名)
        self.assertNotEqual(a1.機会(状態1).意味入力署名, a2.機会(状態2).意味入力署名)

    def test_17_期待効果は契約効果を書換えない(self):
        機会 = HDS作用機会("call", "i1", 出力状態=frozenset({"契約"}), 解消対象=frozenset({"契約残差"}),
                         作用定義ID="def", 意味入力署名="m1")
        適応庫 = HDS適応記憶()
        前状態 = HDS実行状態(残差=frozenset({"観測残差"}))
        作用結果 = HDS作用結果(HDS作用状態.成立, 追加状態=frozenset({"観測"}), 解消残差=frozenset({"観測残差"}))
        後状態, 状態差 = 状態差を受理(前状態, 作用結果)
        適応庫.結果を受け取る(機会, 作用結果, 状態差, 前状態)
        adjusted = 適応庫.機会を補正(機会)
        self.assertEqual(adjusted.出力状態, frozenset({"契約"}))
        self.assertEqual(adjusted.解消対象, frozenset({"契約残差"}))
        self.assertEqual(adjusted.期待.追加状態, frozenset({"観測"}))
        self.assertEqual(adjusted.期待.解消残差, frozenset({"観測残差"}))

    def test_18_同一意味入力の失敗で旧期待を撤回する(self):
        機会 = HDS作用機会("call", "i1", 作用定義ID="def", 意味入力署名="m1")
        適応庫 = HDS適応記憶()
        前状態 = HDS実行状態()
        成功結果 = HDS作用結果(HDS作用状態.成立, 追加状態=frozenset({"X"}))
        後状態, 状態差 = 状態差を受理(前状態, 成功結果)
        適応庫.結果を受け取る(機会, 成功結果, 状態差, 前状態)
        self.assertFalse(適応庫.機会を補正(機会).期待.空)
        失敗結果 = HDS作用結果(HDS作用状態.失敗)
        同状態, 状態差2 = 状態差を受理(前状態, 失敗結果)
        適応庫.結果を受け取る(機会, 失敗結果, 状態差2, 前状態)
        self.assertTrue(適応庫.機会を補正(機会).期待.空)

    def test_19_期待効果は計画へだけ合成する(self):
        仕様 = HDS作用仕様("call", 追加状態=frozenset({"契約"}), 解消残差=frozenset({"契約残差"}))
        機会 = HDS作用機会("call", "i1", 出力状態=frozenset({"契約"}), 解消対象=frozenset({"契約残差"}),
                         期待=期待効果(frozenset({"経験"}), frozenset(), frozenset({"経験残差"}), frozenset(), 1))
        計画仕様 = _期待を計画仕様へ反映(仕様, 機会)
        self.assertEqual(仕様.追加状態, frozenset({"契約"}))
        self.assertEqual(機会.出力状態, frozenset({"契約"}))
        self.assertEqual(計画仕様.追加状態, frozenset({"契約", "経験"}))
        self.assertEqual(計画仕様.解消残差, frozenset({"契約残差", "経験残差"}))

    def test_20_選択器は期待効果を現在状態へ先取りしない(self):
        状態 = HDS実行状態(要求状態=frozenset({"目的"}))
        期待機会 = HDS作用機会("A", "1", 期待=期待効果(frozenset({"目的"}), frozenset(), frozenset(), frozenset(), 1))
        ノイズ機会 = HDS作用機会("B", "2", 出力状態=frozenset({"無関係"}), 優先度=100.0)
        選択結果 = 標準HDS作用選択器().選択(状態, (ノイズ機会, 期待機会))
        self.assertEqual(選択結果.作用ID, "A")
        self.assertNotIn("目的", 状態.成立状態)

    def test_21_別適応記憶インスタンスへ期待を共有しない(self):
        機会 = HDS作用機会("call", "i1", 作用定義ID="def", 意味入力署名="m1")
        旧適応庫 = HDS適応記憶()
        前状態 = HDS実行状態()
        作用結果 = HDS作用結果(HDS作用状態.成立, 追加状態=frozenset({"X"}))
        _, 状態差 = 状態差を受理(前状態, 作用結果)
        旧適応庫.結果を受け取る(機会, 作用結果, 状態差, 前状態)
        self.assertFalse(旧適応庫.機会を補正(機会).期待.空)
        新適応庫 = HDS適応記憶()
        self.assertTrue(新適応庫.機会を補正(機会).期待.空)

    def test_22_全67入口に三者責任記述がある(self):
        for row in 標準能力境界:
            with self.subTest(能力=row.名前):
                self.assertTrue(row.入力変換責任.strip())
                self.assertTrue(row.コア責任.strip())
                self.assertTrue(row.局所部品責任.strip())
                self.assertTrue(row.移行方針.strip())

    def test_23_未確定条件では関係導出を先取りしない(self):
        a = 命題("A", "有効", True)
        b = 命題("A", "稼働", True)
        rule = 規則((命題("?x", "有効", True),), 命題("?x", "稼働", True))
        条件 = 条件項("c", 命題("A", "故障", False))
        導出群, 判定結果 = 条件付き前方閉包((a,), (rule,), (条件,), 真集合=(), 偽集合=())
        self.assertEqual(判定結果.状態, 条件状態.未確定)
        self.assertNotIn(b, 導出群)
        導出群2, 判定結果2 = 条件付き前方閉包((a,), (rule,), (条件,), 真集合=(条件.命題,), 偽集合=())
        self.assertEqual(判定結果2.状態, 条件状態.成立)
        self.assertIn(b, 導出群2)

    def test_24_検証器は読取専用で全体状態を書換えられない(self):
        class V:
            ID = "v"
            版 = "1"
            @staticmethod
            def 検証(状態, 対象):
                object.__setattr__(状態, "版", 状態.版 + 1)
                return True
        with self.assertRaises(ValueError):
            検証器群を実行(HDS実行状態(), (V(),), None)

    def test_25_コアパッケージは専門実装をimportしない(self):
        import ast
        from pathlib import Path
        root = Path(__file__).parents[1] / "src" / "minidora" / "コア"
        forbidden = ("HDS運用", "製品版", "構文化", "科学", "数学", "コード", "会話", "関係言語")
        for path in root.glob("*.py"):
            tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
            for node in ast.walk(tree):
                if isinstance(node, ast.ImportFrom):
                    name = node.module or ""
                    self.assertFalse(any(x in name for x in forbidden), f"{path.name}: {name}")
                elif isinstance(node, ast.Import):
                    for alias in node.names:
                        self.assertFalse(any(x in alias.name for x in forbidden), f"{path.name}: {alias.name}")

    def test_26_共通署名の互換入口は同じ結果を返す(self):
        from minidora.コア.値 import 署名 as core_sig
        from minidora.統合駆動_v2.値 import 署名 as legacy_sig
        value = {"a": (1, 2), "b": frozenset({"x", "y"})}
        self.assertEqual(core_sig(value), legacy_sig(value))

    def test_27_別呼出住所でも同一定義同一意味入力なら期待を共有(self):
        第一機会 = HDS作用機会("運用能力/p1/s1/X", "i1", 作用定義ID="運用能力/X", 意味入力署名="同状態")
        第二機会 = HDS作用機会("運用能力/p2/s9/X", "i2", 作用定義ID="運用能力/X", 意味入力署名="同状態")
        適応庫 = HDS適応記憶()
        前状態 = HDS実行状態()
        作用結果 = HDS作用結果(HDS作用状態.成立, 追加状態=frozenset({"観測効果"}))
        _, 状態差 = 状態差を受理(前状態, 作用結果)
        適応庫.結果を受け取る(第一機会, 作用結果, 状態差, 前状態)
        self.assertEqual(適応庫.機会を補正(第二機会).期待.追加状態, frozenset({"観測効果"}))

    def test_28_既に契約効果成立済みの無変化は反証にしない(self):
        機会 = HDS作用機会("call", "i1", 出力状態=frozenset({"X"}), 作用定義ID="def", 意味入力署名="同状態")
        適応庫 = HDS適応記憶()
        空状態 = HDS実行状態()
        成功結果 = HDS作用結果(HDS作用状態.成立, 追加状態=frozenset({"X"}))
        状態X, 状態差 = 状態差を受理(空状態, 成功結果)
        適応庫.結果を受け取る(機会, 成功結果, 状態差, 空状態)
        無変化結果 = HDS作用結果(HDS作用状態.成立)
        _, 状態差2 = 状態差を受理(状態X, 無変化結果)
        適応庫.結果を受け取る(機会, 無変化結果, 状態差2, 状態X)
        self.assertEqual(適応庫.機会を補正(機会).期待.追加状態, frozenset({"X"}))


if __name__ == "__main__":
    unittest.main()

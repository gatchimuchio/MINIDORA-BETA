from __future__ import annotations

import ast
from dataclasses import dataclass
from pathlib import Path
import unittest

from minidora.コア import (
    コア責任,
    コア責任閉包を検査,
    標準能力境界,
    標準能力登録を閉包,
    条件項,
    条件状態,
    条件集合を判定,
    内容計画を構成,
    内容計画を検査,
    内容計画を表現,
    検証器群を実行,
)


@dataclass(frozen=True)
class 命題:
    対象: str
    関係: str
    値: object
    範囲: str = "未指定"
    時点: str = "未指定"


class MINIDORAコア閉包試験(unittest.TestCase):
    def test_01_初期整理の八責任だけを正本化する(self):
        self.assertEqual(tuple(x.ID for x in コア責任), tuple(f"C{i}" for i in range(1, 9)))
        self.assertTrue(all(x.所有 and x.非所有 for x in コア責任))
        self.assertFalse(next(x for x in コア責任 if x.ID == "C7").能力入口対応)
        self.assertTrue(all("C7" not in x.関連コア責任ID for x in 標準能力境界))
        self.assertTrue(コア責任閉包を検査(標準能力境界))

    def test_02_標準67登録は未知入口をfilterせず拒否する(self):
        names = tuple(x.名前 for x in 標準能力境界)
        self.assertEqual(len(names), 67)
        self.assertEqual(len(標準能力登録を閉包(names)), 67)
        with self.assertRaises(ValueError):
            標準能力登録を閉包(names + ("新しい組込標準",))
        with self.assertRaises(ValueError):
            標準能力登録を閉包(names[:-1])

    def test_03_拡張能力は標準境界から分離し標準名を上書きできない(self):
        names = tuple(x.名前 for x in 標準能力境界)
        self.assertEqual(len(標準能力登録を閉包(names, ("利用者拡張",))), 67)
        with self.assertRaises(ValueError):
            標準能力登録を閉包(names, (names[0],))

    def test_04_条件は未観測と反証を分ける(self):
        p = 命題("装置A", "故障", True)
        item = 条件項("c", p)
        self.assertEqual(条件集合を判定((item,), (), ()).状態, 条件状態.未確定)
        self.assertEqual(条件集合を判定((item,), (), (p,)).状態, 条件状態.不成立)
        self.assertEqual(条件集合を判定((item,), (p,), ()).状態, 条件状態.成立)

    def test_05_内容計画は局所本文を発明せずそのまま通す(self):
        plan = 内容計画を構成("結論", 種別="資料内容構成", 根拠=("r1",), 由来=("参照:x",))
        self.assertTrue(内容計画を検査(plan, 許可根拠={"r1"}))
        self.assertEqual(内容計画を表現(plan), "結論")

    def test_06_検証管理は別台帳を作らず読取専用境界だけ持つ(self):
        import minidora.コア.検証管理 as verification
        self.assertFalse(hasattr(verification, "検証記録"))
        self.assertFalse(hasattr(verification, "検証契約を照合"))
        class V:
            ID = "A"
            版 = "1"
            def 検証(self, 状態, 対象):
                対象["x"] = 2
                return True
        with self.assertRaises(ValueError):
            検証器群を実行({"s": 1}, (V(),), {"x": 1})

    def test_07_実行経路は標準能力境界と内容計画を実際に通す(self):
        root = Path(__file__).parents[1] / "src" / "minidora"
        process = (root / "HDS運用" / "工程.py").read_text(encoding="utf-8")
        catalog = (root / "HDS運用" / "能力.py").read_text(encoding="utf-8")
        self.assertIn("境界 = self.目録.境界契約(name)", process)
        self.assertIn("内容計画を構成", process)
        self.assertIn('"C8" in 境界.関連コア責任ID', process)
        self.assertIn("標準能力登録を閉包", catalog)
        self.assertNotIn("if r.モジュール.名前 in 標準名", catalog)

    def test_08_関係仮説経路は共通条件判定を使う(self):
        root = Path(__file__).parents[1] / "src" / "minidora"
        原文 = (root / "統合駆動_v2" / "意味構成.py").read_text(encoding="utf-8")
        tree = ast.parse(原文)
        names = {node.func.id for node in ast.walk(tree) if isinstance(node, ast.Call) and isinstance(node.func, ast.Name)}
        self.assertIn("_共通条件判定", names)
        self.assertIn("_共通条件項", names)

    def test_09_コアは専門層をimportしない(self):
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

    def test_10_横断責任C6_C7は通常循環へ実配線される(self):
        root = Path(__file__).parents[1] / "src" / "minidora"
        cycle = (root / "統合駆動_v2" / "循環.py").read_text(encoding="utf-8")
        self.assertIn("検証器群を実行(現在, 主体.最終検証器, None)", cycle)
        actor = (root / "HDS実行主体.py").read_text(encoding="utf-8")
        self.assertIn("HDS適応記憶", actor)
        self.assertIn("適応記憶 = 主体.適応記憶", cycle)
        self.assertIn("適応記憶.結果を受け取る", cycle)
        self.assertIn("適応記憶.機会を補正", cycle)
        self.assertIn("_期待を計画仕様へ反映", cycle)


if __name__ == "__main__":
    unittest.main()

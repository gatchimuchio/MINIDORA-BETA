from __future__ import annotations

import unittest

from minidora.HDS実行主体 import (
    HDS作用結果,
    HDS作用状態,
    HDS実行主体,
    HDS実行状態,
    HDS終端,
    HDS関数作用,
    HDS作用機会,
)
from minidora.統合駆動_v2.適応記憶 import HDS適応記憶
from minidora.コア.状態操作 import 状態差を受理


class HDS適応記憶試験(unittest.TestCase):
    def test_成功した状態差を同じ作用文脈の後続選択へ反映する(self):
        順序 = []
        動的 = HDS関数作用(
            "動的再取得",
            lambda 状態: (
                順序.append("動的再取得")
                or HDS作用結果(
                    HDS作用状態.成立,
                    追加状態=frozenset({"初回済み"}),
                    解消残差=frozenset({"観測不足"}),
                )
            ),
            機会判定=lambda 状態: "観測不足" in 状態.残差,
            解消対象=("観測不足",),
        )
        利用 = HDS関数作用(
            "資料利用",
            lambda 状態: (
                順序.append("資料利用")
                or HDS作用結果(
                    HDS作用状態.成立,
                    追加状態=frozenset({"利用済み"}),
                    追加残差=frozenset({"観測不足"}),
                )
            ),
            入力状態=("初回済み",),
            出力状態=("利用済み",),
        )
        ノイズ = HDS関数作用(
            "高優先度ノイズ",
            lambda 状態: (
                順序.append("高優先度ノイズ")
                or HDS作用結果(HDS作用状態.成立, 追加状態=frozenset({"ノイズ"}))
            ),
            入力状態=("利用済み",),
            出力状態=("ノイズ",),
            優先度=100.0,
        )
        初期 = HDS実行状態(
            要求状態=frozenset({"利用済み"}),
            残差=frozenset({"観測不足"}),
        )
        結果 = HDS実行主体((動的, 利用, ノイズ), 最大作用回数=6).実行(初期)
        self.assertEqual(結果.終端, HDS終端.採用)
        self.assertEqual(順序, ["動的再取得", "資料利用", "動的再取得"])

    def test_同一主体では実行を跨いで適応を継続する(self):
        順序 = []
        動的 = HDS関数作用(
            "動的再取得",
            lambda 状態: (
                順序.append("動的再取得")
                or HDS作用結果(HDS作用状態.成立, 追加状態=frozenset({"資料あり"}))
            ),
            機会判定=lambda 状態: "資料あり" not in 状態.成立状態,
            出力状態=("資料あり",),
        )
        ノイズ = HDS関数作用(
            "高優先度ノイズ",
            lambda 状態: (
                順序.append("高優先度ノイズ")
                or HDS作用結果(HDS作用状態.成立, 追加状態=frozenset({"ノイズ"}))
            ),
            入力状態=("利用済み",),
            出力状態=("ノイズ",),
            優先度=100.0,
        )
        主体 = HDS実行主体((動的, ノイズ), 最大作用回数=4)
        初期 = HDS実行状態(
            要求状態=frozenset({"資料あり"}),
            成立状態=frozenset({"利用済み"}),
        )
        self.assertEqual(主体.実行(初期).終端, HDS終端.採用)

        順序.clear()
        self.assertEqual(主体.実行(初期).終端, HDS終端.採用)
        self.assertEqual(順序[0], "動的再取得")
        self.assertGreaterEqual(主体.適応記憶.経験数, 2)

    def test_別適応記憶インスタンスへ経験を共有しない(self):
        機会 = HDS作用機会("作用A", "入力", 作用定義ID="定義A", 意味入力署名="意味A")
        前 = HDS実行状態()
        結果 = HDS作用結果(HDS作用状態.成立, 追加状態=frozenset({"成立"}))
        _, 差 = 状態差を受理(前, 結果)

        第一 = HDS適応記憶()
        第一.結果を受け取る(機会, 結果, 差, 前)
        self.assertFalse(第一.機会を補正(機会).期待.空)

        第二 = HDS適応記憶()
        self.assertTrue(第二.機会を補正(機会).期待.空)

    def test_複数成功経験から同型新入力で成功経路を静的優先度より上位へ置く(self):
        from types import SimpleNamespace
        from minidora.HDS実行主体 import 標準HDS作用選択器

        記憶 = HDS適応記憶()
        差 = SimpleNamespace(変化有無=True)

        def 経路記録(署名, 支持=True):
            行 = SimpleNamespace(
                作用ID="経験経路",
                対象残差=("不足",),
                対象未達状態=("完了",),
                前状態署名=署名,
                作用入力署名="同一run内で変化してよい",
                作用状態=HDS作用状態.成立 if 支持 else HDS作用状態.失敗,
                目的進展=支持,
                進展根拠=("残差:不足",) if 支持 else (),
                状態差=差,
            )
            結果 = SimpleNamespace(
                終端=HDS終端.採用 if 支持 else HDS終端.保留,
                状態=SimpleNamespace(閉包済み=支持),
                履歴=(行,),
            )
            記憶.実行結果を受け取る(結果)

        経路記録("入力一")
        経路記録("入力二")
        self.assertEqual(記憶.経路経験数, 2)

        状態 = HDS実行状態(
            要求状態=frozenset({"完了"}),
            残差=frozenset({"不足"}),
        )
        学習候補 = HDS作用機会(
            "経験経路", "入力三",
            出力状態=frozenset({"完了"}),
            解消対象=frozenset({"不足"}),
            優先度=0.0,
        )
        静的候補 = HDS作用機会(
            "静的高優先度", "入力三",
            出力状態=frozenset({"完了"}),
            解消対象=frozenset({"不足"}),
            優先度=100.0,
        )
        未学習選択 = 標準HDS作用選択器(HDS適応記憶()).選択(状態, (学習候補, 静的候補))
        self.assertEqual(未学習選択.作用ID, "静的高優先度")
        学習選択 = 標準HDS作用選択器(記憶).選択(状態, (学習候補, 静的候補))
        self.assertEqual(学習選択.作用ID, "経験経路")

        経路記録("入力四", 支持=False)
        経路記録("入力五", 支持=False)
        反証後 = 標準HDS作用選択器(記憶).選択(状態, (学習候補, 静的候補))
        self.assertEqual(反証後.作用ID, "静的高優先度")

    def test_異なる呼出住所でも同じ作用定義の成功経路を継承する(self):
        from types import SimpleNamespace

        記憶 = HDS適応記憶()
        差 = SimpleNamespace(変化有無=True)

        for 作用ID, 経験署名 in (
            ("案件一/探索/参照", "経験一"),
            ("案件二/探索/参照", "経験二"),
        ):
            行 = SimpleNamespace(
                作用ID=作用ID,
                作用定義ID="運用能力/参照",
                対象残差=("不足",),
                対象未達状態=("完了",),
                前状態署名=経験署名,
                作用状態=HDS作用状態.成立,
                目的進展=True,
                進展根拠=("残差:不足",),
                状態差=差,
            )
            記憶.実行結果を受け取る(SimpleNamespace(
                終端=HDS終端.採用,
                状態=SimpleNamespace(閉包済み=True),
                履歴=(行,),
            ))

        状態 = HDS実行状態(
            要求状態=frozenset({"完了"}),
            残差=frozenset({"不足"}),
        )
        新しい呼出 = HDS作用機会(
            "案件三/探索/参照", "入力三",
            出力状態=frozenset({"完了"}),
            解消対象=frozenset({"不足"}),
            作用定義ID="運用能力/参照",
        )
        別能力 = HDS作用機会(
            "案件三/探索/別能力", "入力三",
            出力状態=frozenset({"完了"}),
            解消対象=frozenset({"不足"}),
            作用定義ID="運用能力/別能力",
        )
        self.assertEqual(記憶.作用経路得点(状態, 新しい呼出), 2)
        self.assertEqual(記憶.作用経路得点(状態, 別能力), 0)

    def test_未閉包runの中間進展も経路反例として数える(self):
        from types import SimpleNamespace

        記憶 = HDS適応記憶()
        差 = SimpleNamespace(変化有無=True)
        for 経験署名 in ("成功一", "成功二"):
            行 = SimpleNamespace(
                作用ID="経験探索", 作用定義ID="運用能力/探索",
                対象残差=("不足",), 対象未達状態=("完了",),
                前状態署名=経験署名, 作用状態=HDS作用状態.成立,
                目的進展=True, 進展根拠=("残差:不足",), 状態差=差,
            )
            記憶.実行結果を受け取る(SimpleNamespace(
                終端=HDS終端.採用,
                状態=SimpleNamespace(閉包済み=True),
                履歴=(行,),
            ))

        失敗行 = SimpleNamespace(
            作用ID="経験探索", 作用定義ID="運用能力/探索",
            対象残差=("不足",), 対象未達状態=("完了",),
            前状態署名="失敗一", 作用状態=HDS作用状態.成立,
            目的進展=True, 進展根拠=("残差:不足",), 状態差=差,
        )
        記憶.実行結果を受け取る(SimpleNamespace(
            終端=HDS終端.保留,
            状態=SimpleNamespace(閉包済み=False),
            履歴=(失敗行,),
        ))
        状態 = HDS実行状態(
            要求状態=frozenset({"完了"}), 残差=frozenset({"不足"}),
        )
        機会 = HDS作用機会(
            "別住所/探索", "新入力",
            出力状態=frozenset({"完了"}), 解消対象=frozenset({"不足"}),
            作用定義ID="運用能力/探索",
        )
        self.assertEqual(記憶.作用経路得点(状態, 機会), 1)

        失敗行二 = SimpleNamespace(**{**失敗行.__dict__, "前状態署名": "失敗二"})
        記憶.実行結果を受け取る(SimpleNamespace(
            終端=HDS終端.保留,
            状態=SimpleNamespace(閉包済み=False),
            履歴=(失敗行二,),
        ))
        self.assertEqual(記憶.作用経路得点(状態, 機会), 0)

    def test_同支持なら短く閉じた作用経路を優先する(self):
        from types import SimpleNamespace
        from minidora.HDS実行主体 import 標準HDS作用選択器

        記憶 = HDS適応記憶()
        差 = SimpleNamespace(変化有無=True)

        def 成功経験(能力, 経験署名, 経路長):
            対象 = SimpleNamespace(
                作用ID=能力 + "/呼出",
                作用定義ID=能力,
                対象残差=("不足",), 対象未達状態=("完了",),
                前状態署名=経験署名, 作用状態=HDS作用状態.成立,
                目的進展=True, 進展根拠=("残差:不足",), 状態差=差,
            )
            補助 = tuple(
                SimpleNamespace(
                    作用ID=f"補助/{能力}/{i}", 作用定義ID=f"補助/{能力}/{i}",
                    対象残差=(), 対象未達状態=(),
                    前状態署名=経験署名, 作用状態=HDS作用状態.成立,
                    目的進展=True, 進展根拠=("状態:中間",), 状態差=差,
                )
                for i in range(max(0, 経路長 - 1))
            )
            記憶.実行結果を受け取る(SimpleNamespace(
                終端=HDS終端.採用,
                状態=SimpleNamespace(閉包済み=True),
                履歴=(対象, *補助),
            ))

        for 経験 in ("短一", "短二"):
            成功経験("能力/短", 経験, 3)
        for 経験 in ("長一", "長二"):
            成功経験("能力/長", 経験, 15)

        状態 = HDS実行状態(
            要求状態=frozenset({"完了"}), 残差=frozenset({"不足"}),
        )
        短 = HDS作用機会(
            "現在/短", "入力",
            出力状態=frozenset({"完了"}), 解消対象=frozenset({"不足"}),
            作用定義ID="能力/短",
        )
        長 = HDS作用機会(
            "現在/長", "入力",
            出力状態=frozenset({"完了"}), 解消対象=frozenset({"不足"}),
            作用定義ID="能力/長",
        )
        self.assertEqual(記憶.作用経路評価(状態, 短), (2, 3))
        self.assertEqual(記憶.作用経路評価(状態, 長), (2, 15))
        self.assertEqual(
            標準HDS作用選択器(記憶).選択(状態, (長, 短)).作用ID,
            "現在/短",
        )

    def test_反例優勢の作用経路を隔離し別経路へ退避する(self):
        from types import SimpleNamespace
        from minidora.HDS実行主体 import 標準HDS作用選択器

        記憶 = HDS適応記憶()
        差 = SimpleNamespace(変化有無=True)
        for 経験署名 in ("失敗一", "失敗二"):
            行 = SimpleNamespace(
                作用ID="失敗経路/呼出", 作用定義ID="能力/失敗経路",
                対象残差=("不足",), 対象未達状態=("完了",),
                前状態署名=経験署名, 作用状態=HDS作用状態.成立,
                目的進展=True, 進展根拠=("残差:不足",), 状態差=差,
            )
            記憶.実行結果を受け取る(SimpleNamespace(
                終端=HDS終端.保留,
                状態=SimpleNamespace(閉包済み=False),
                履歴=(行,),
            ))

        状態 = HDS実行状態(
            要求状態=frozenset({"完了"}), 残差=frozenset({"不足"}),
        )
        失敗候補 = HDS作用機会(
            "新規/失敗経路", "入力",
            出力状態=frozenset({"完了"}), 解消対象=frozenset({"不足"}),
            優先度=100.0, 作用定義ID="能力/失敗経路",
        )
        未学習候補 = HDS作用機会(
            "新規/未学習経路", "入力",
            出力状態=frozenset({"完了"}), 解消対象=frozenset({"不足"}),
            優先度=0.0, 作用定義ID="能力/未学習経路",
        )
        self.assertEqual(記憶.作用経路評価(状態, 失敗候補), (-2, 0))
        self.assertEqual(
            標準HDS作用選択器(記憶).選択(状態, (失敗候補, 未学習候補)).作用ID,
            "新規/未学習経路",
        )

    def test_隔離経路は新しい支持が反例を上回れば復帰する(self):
        from types import SimpleNamespace

        記憶 = HDS適応記憶()
        差 = SimpleNamespace(変化有無=True)

        def 経験を入れる(経験署名, 成功):
            行 = SimpleNamespace(
                作用ID="経路/呼出", 作用定義ID="能力/経路",
                対象残差=("不足",), 対象未達状態=("完了",),
                前状態署名=経験署名, 作用状態=HDS作用状態.成立,
                目的進展=True, 進展根拠=("残差:不足",), 状態差=差,
            )
            記憶.実行結果を受け取る(SimpleNamespace(
                終端=HDS終端.採用 if 成功 else HDS終端.保留,
                状態=SimpleNamespace(閉包済み=成功),
                履歴=(行,),
            ))

        for 経験署名 in ("失敗一", "失敗二"):
            経験を入れる(経験署名, False)
        for 経験署名 in ("成功一", "成功二", "成功三"):
            経験を入れる(経験署名, True)

        状態 = HDS実行状態(
            要求状態=frozenset({"完了"}), 残差=frozenset({"不足"}),
        )
        機会 = HDS作用機会(
            "別住所/経路", "新入力",
            出力状態=frozenset({"完了"}), 解消対象=frozenset({"不足"}),
            作用定義ID="能力/経路",
        )
        self.assertEqual(記憶.作用経路評価(状態, 機会), (1, 1))

    def test_学習済み経路が通常循環の誤探索停止を回避する(self):
        from minidora.統合駆動_v2.計画 import HDS探索契約

        成功探索 = HDS探索契約(
            "成功探索", "不足を調べる", ("成果:成功証拠",), ("状態:完了",),
            最大試行=2, 最大資源=8, 再観測=True,
        )
        誤探索 = HDS探索契約(
            "誤探索", "不足を調べる", ("成果:誤証拠",), ("状態:完了",),
            最大試行=2, 最大資源=8, 再観測=True,
        )
        成功作用 = HDS関数作用(
            "経験探索",
            lambda 状態: HDS作用結果(
                HDS作用状態.成立, 追加状態=frozenset({"完了"}),
                解消残差=frozenset({"不足"}), 成果=(("成功証拠", "成立"),),
            ),
            出力状態=("完了",), 解消対象=("不足",), 優先度=0.0,
            入力署名=lambda 状態: str(状態.主体辞書().get("試行", "")),
            生成成果=("成功証拠",), 探索=成功探索, 純粋作用=True,
        )
        誤作用 = HDS関数作用(
            "静的高優先探索",
            lambda 状態: HDS作用結果(
                HDS作用状態.保留, 追加残差=frozenset({"袋小路"}),
                成果=(("誤証拠", "不成立"),), 停止要求=True, 理由=("誤探索",),
            ),
            出力状態=("完了",), 解消対象=("不足",), 優先度=100.0,
            入力署名=lambda 状態: str(状態.主体辞書().get("試行", "")),
            生成成果=("誤証拠",), 探索=誤探索, 純粋作用=True,
        )

        def 初期(試行):
            return HDS実行状態(
                要求状態=frozenset({"完了"}), 残差=frozenset({"不足"}),
                主体状態=(("試行", 試行),),
            )

        記憶 = HDS適応記憶()
        for 試行 in ("一", "二"):
            学習 = HDS実行主体((成功作用,), 最大作用回数=4, 適応記憶=記憶).実行(初期(試行))
            self.assertEqual(学習.終端, HDS終端.採用)

        未学習 = HDS実行主体(
            (成功作用, 誤作用), 最大作用回数=4, 適応記憶=HDS適応記憶()
        ).実行(初期("三"))
        self.assertEqual(未学習.終端, HDS終端.保留)
        self.assertEqual(未学習.履歴[0].作用ID, "静的高優先探索")

        学習済み = HDS実行主体(
            (成功作用, 誤作用), 最大作用回数=4, 適応記憶=記憶
        ).実行(初期("三"))
        self.assertEqual(学習済み.終端, HDS終端.採用)
        self.assertEqual(学習済み.履歴[0].作用ID, "経験探索")

    def test_失敗反証で同一文脈の旧期待を撤回する(self):
        機会 = HDS作用機会("作用A", "入力", 作用定義ID="定義A", 意味入力署名="意味A")
        前 = HDS実行状態()
        成功 = HDS作用結果(HDS作用状態.成立, 追加状態=frozenset({"成立"}))
        _, 成功差 = 状態差を受理(前, 成功)
        記憶 = HDS適応記憶()
        記憶.結果を受け取る(機会, 成功, 成功差, 前)
        self.assertFalse(記憶.機会を補正(機会).期待.空)

        失敗 = HDS作用結果(HDS作用状態.失敗)
        _, 失敗差 = 状態差を受理(前, 失敗)
        記憶.結果を受け取る(機会, 失敗, 失敗差, 前)
        self.assertTrue(記憶.機会を補正(機会).期待.空)



    def test_動的計画も強い学習証拠がある時だけ成功経路へ補正する(self):
        def 初期(試行):
            return HDS実行状態(
                要求状態=frozenset({"完了"}),
                残差=frozenset({"不足"}),
                主体状態=(("試行", 試行),),
            )

        成功作用 = HDS関数作用(
            "成功経路/呼出",
            lambda 状態: HDS作用結果(
                HDS作用状態.成立,
                追加状態=frozenset({"完了"}),
                解消残差=frozenset({"不足"}),
            ),
            出力状態=("完了",), 解消対象=("不足",),
            資源負荷=2, 優先度=0.0,
            入力署名=lambda 状態: str(状態.主体辞書().get("試行", "")),
            純粋作用=True, 作用定義ID="能力/成功経路",
        )
        誤作用 = HDS関数作用(
            "誤経路/呼出",
            lambda 状態: HDS作用結果(
                HDS作用状態.保留,
                追加残差=frozenset({"袋小路"}),
                停止要求=True,
                理由=("誤経路",),
            ),
            出力状態=("完了",), 解消対象=("不足",),
            資源負荷=1, 優先度=100.0,
            入力署名=lambda 状態: str(状態.主体辞書().get("試行", "")),
            純粋作用=True, 作用定義ID="能力/誤経路",
        )

        未学習 = HDS実行主体(
            (誤作用, 成功作用), 最大作用回数=4, 適応記憶=HDS適応記憶()
        ).実行(初期("三"))
        self.assertEqual(未学習.終端, HDS終端.保留)
        self.assertEqual(未学習.履歴[0].作用ID, "誤経路/呼出")
        self.assertEqual(未学習.計装.学習計画補正数, 0)

        記憶 = HDS適応記憶()
        for 試行 in ("一", "二"):
            学習 = HDS実行主体(
                (成功作用,), 最大作用回数=4, 適応記憶=記憶
            ).実行(初期(試行))
            self.assertEqual(学習.終端, HDS終端.採用)

        学習済み = HDS実行主体(
            (誤作用, 成功作用), 最大作用回数=4, 適応記憶=記憶
        ).実行(初期("三"))
        self.assertEqual(学習済み.終端, HDS終端.採用)
        self.assertEqual(学習済み.履歴[0].作用ID, "成功経路/呼出")
        self.assertEqual(学習済み.計装.学習計画補正数, 1)

if __name__ == "__main__":
    unittest.main()

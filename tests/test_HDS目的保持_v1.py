from __future__ import annotations

import unittest

from minidora.HDS実行主体 import HDS実行主体, HDS実行状態, HDS関数作用, HDS作用結果, HDS作用状態, HDS終端
from minidora.統合駆動_v2.政策 import HDS運用政策, 停止理由


class HDS目的保持試験(unittest.TestCase):
    def test_内部状態更新だけでは軟予算を延長しない(self):
        def 学習だけする(状態):
            回数 = int(状態.主体辞書().get("学習反復", 0))
            return HDS作用結果(
                HDS作用状態.成立,
                主体状態差分=(("学習反復", 回数 + 1),),
            )

        作用 = HDS関数作用(
            "学習だけ",
            学習だけする,
            入力署名=lambda 状態: str(状態.主体辞書().get("学習反復", 0)),
        )
        結果 = HDS実行主体(
            (作用,),
            最大作用回数=8,
            政策=HDS運用政策(初期作用予算=1, 予算増分=1),
        ).実行(HDS実行状態(目的=("問いへ回答する",), 要求状態=frozenset({"回答完了"})))

        self.assertEqual(結果.終端, HDS終端.保留)
        self.assertEqual(結果.停止種別, 停止理由.作用不足)
        self.assertEqual(結果.計装.作用実行数, 0)
        self.assertEqual(結果.計装.予算拡張数, 0)
        self.assertEqual(結果.計装.目的進展数, 0)
        self.assertEqual(結果.計装.目的無進展数, 0)
        self.assertEqual(結果.履歴, ())

    def test_目的から逆算した中間工程は軟予算を延長する(self):
        作用群 = tuple(
            HDS関数作用(
                str(i),
                lambda 状態, i=i: HDS作用結果(HDS作用状態.成立, 追加状態=frozenset({str(i)})),
                入力状態=(str(i - 1),) if i else (),
                出力状態=(str(i),),
            )
            for i in range(4)
        )
        結果 = HDS実行主体(
            作用群,
            最大作用回数=4,
            政策=HDS運用政策(初期作用予算=1, 予算増分=1),
        ).実行(HDS実行状態(目的=("3を成立させる",), 要求状態=frozenset({"3"})))

        self.assertEqual(結果.終端, HDS終端.採用)
        self.assertEqual(結果.計装.予算拡張数, 3)
        self.assertEqual(結果.計装.目的進展数, 4)
        self.assertTrue(all(x.目的進展 for x in 結果.履歴))

    def test_目的と無関係な再評価解消は進展にしない(self):
        from minidora.統合駆動_v2.目的保持 import 目的を観測, 目的進展を判定
        from minidora.統合駆動_v2.状態更新 import 有効認識

        前 = HDS実行状態(
            目的=("問いへ回答する",),
            要求状態=frozenset({"回答完了"}),
            再評価待ち=frozenset({"成果:雑音"}),
        )
        後 = HDS実行状態(
            目的=("問いへ回答する",),
            要求状態=frozenset({"回答完了"}),
        )
        前観測 = 目的を観測(前, 有効認識)
        後観測 = 目的を観測(後, 有効認識, 契約署名=前観測.契約署名)
        進展, _ = 目的進展を判定(
            前観測=前観測,
            後観測=後観測,
            最良直接尺度=前観測.直接尺度,
            計画最良残数={},
        )
        self.assertFalse(進展)

    def test_目的正本を書き換える作用は契約違反になる(self):
        作用 = HDS関数作用(
            "目的改変",
            lambda 状態: HDS作用結果(
                HDS作用状態.成立,
                主体状態差分=(("HDS目的正本", ("別目的",)),),
            ),
            出力状態=("回答完了",),
        )
        初期 = HDS実行状態(
            目的=("問いへ回答する",),
            要求状態=frozenset({"回答完了"}),
            主体状態=(("HDS目的正本", ("元目的",)),),
        )
        結果 = HDS実行主体((作用,), 最大作用回数=2).実行(初期)
        self.assertEqual(結果.終端, HDS終端.失敗)
        self.assertEqual(結果.停止種別, 停止理由.契約違反)
        self.assertEqual(結果.状態.主体辞書()["HDS目的正本"], ("元目的",))


class HDS目的経路拘束試験(unittest.TestCase):
    def test_目的経路外の学習作用は一度も起動しない(self):
        呼出 = []
        def 学習だけする(状態):
            呼出.append(1)
            回数 = int(状態.主体辞書().get("学習反復", 0))
            return HDS作用結果(HDS作用状態.成立, 主体状態差分=(("学習反復", 回数 + 1),))
        作用 = HDS関数作用("学習だけ", 学習だけする, 入力署名=lambda 状態: str(状態.主体辞書().get("学習反復", 0)), 優先度=100)
        結果 = HDS実行主体((作用,), 最大作用回数=8, 政策=HDS運用政策(初期作用予算=8, 予算増分=1)).実行(
            HDS実行状態(目的=("問いへ回答する",), 要求状態=frozenset({"回答完了"})))
        self.assertEqual(呼出, [])
        self.assertEqual(結果.計装.作用実行数, 0)
        self.assertEqual(結果.終端, HDS終端.保留)
        self.assertEqual(結果.停止種別, 停止理由.作用不足)

    def test_明示した目的依存の前処理は通す(self):
        呼出 = []
        def 準備(状態):
            呼出.append("準備")
            return HDS作用結果(HDS作用状態.成立, 主体状態差分=(("準備済み", True),))
        def 解消(状態):
            呼出.append("解消")
            return HDS作用結果(HDS作用状態.成立, 解消残差=frozenset({"不足"}))
        前処理 = HDS関数作用("前処理", 準備, 機会判定=lambda 状態: not bool(状態.主体辞書().get("準備済み", False)), 目的依存=("残差:不足",))
        解消作用 = HDS関数作用("不足解消", 解消, 解消対象=("不足",), 機会判定=lambda 状態: bool(状態.主体辞書().get("準備済み", False)))
        結果 = HDS実行主体((前処理, 解消作用), 最大作用回数=4).実行(HDS実行状態(目的=("不足を閉じる",), 残差=frozenset({"不足"})))
        self.assertEqual(結果.終端, HDS終端.採用, 結果.理由)
        self.assertEqual(呼出, ["準備", "解消"])

    def test_目的依存は修飾名を要求する(self):
        with self.assertRaisesRegex(ValueError, "作用目的依存"):
            HDS関数作用("不正", lambda _s: HDS作用結果(HDS作用状態.成立), 目的依存=("不足",))


if __name__ == "__main__":
    unittest.main()

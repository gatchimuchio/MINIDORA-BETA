from __future__ import annotations

from dataclasses import replace
import unittest
from unittest.mock import patch

from minidora.HDS構文化器_v1 import 公開HDSコンパイラ
from minidora.HDS実行主体 import HDS関数作用, HDS作用結果, HDS作用状態, HDS終端
from minidora.統合駆動_v2.認識 import HDS認識項目, 認識区分
from minidora.HDS選択継承循環 import HDS選択継承供給, 初回評価参照成果名
from minidora.HDS駆動コア import HDS駆動コア
from minidora.参照 import 参照記録
from minidora.統合駆動_v2.記憶 import HDS資料, HDS記憶


class _空参照供給器:
    名称 = "学習機械空参照"
    並列安全 = False

    def __init__(self) -> None:
        self.問合せ: list[str] = []

    def 検索(self, 問合せ, 上限=8):
        self.問合せ.append(" ".join(str(問合せ).split()))
        return ()


class HDS学習機械循環V2試験(unittest.TestCase):
    def _作業認識(self, ID, 対象, 関係, 値, 資料, 依存=()):
        return HDS認識項目(
            ID, 対象, 関係, 値, 認識区分.確定,
            根拠=(資料.出典(),), 依存=依存, 検証契約="試験検証/v1",
        )

    def test_継続認識は現在問に関係する作業集合と依存鎖だけを投入する(self) -> None:
        扉資料 = HDS資料("扉資料", "1", "扉は施錠", "試験")
        天気資料 = HDS資料("天気資料", "1", "晴天", "試験")
        種資料 = HDS資料("種資料", "1", "晴天観測", "試験")
        扉 = self._作業認識("扉認識", "扉", "施錠", True, 扉資料)
        種 = self._作業認識("種認識", "観測", "晴天", True, 種資料)
        天気 = self._作業認識("天気認識", "天気", "晴天", True, 天気資料, ("種認識",))

        作業 = HDS駆動コア._認識作業集合("天気を確認", (扉, 種, 天気))
        self.assertEqual({x.ID for x in 作業}, {"種認識", "天気認識"})

    def test_作業外認識も正本として保持する(self) -> None:
        扉資料 = HDS資料("扉資料", "1", "扉は施錠", "試験")
        天気資料 = HDS資料("天気資料", "1", "晴天", "試験")
        扉 = self._作業認識("扉認識", "扉", "施錠", True, 扉資料)
        天気 = self._作業認識("天気認識", "天気", "晴天", True, 天気資料)
        記憶 = HDS記憶().更新((扉資料, 天気資料))

        統合 = HDS駆動コア._継続認識を統合(
            (扉, 天気), frozenset({"扉認識"}), (扉,), 記憶,
        )
        self.assertEqual({x.ID for x in 統合}, {"扉認識", "天気認識"})

    def test_作業外認識も根拠資料改訂時は失効する(self) -> None:
        扉資料 = HDS資料("扉資料", "1", "扉は施錠", "試験")
        天気旧 = HDS資料("天気資料", "1", "晴天", "試験")
        天気新 = HDS資料("天気資料", "2", "雨天", "試験")
        扉 = self._作業認識("扉認識", "扉", "施錠", True, 扉資料)
        天気 = self._作業認識("天気認識", "天気", "晴天", True, 天気旧)
        記憶 = HDS記憶().更新((扉資料, 天気旧)).更新((天気新,))

        統合 = HDS駆動コア._継続認識を統合(
            (扉, 天気), frozenset({"扉認識"}), (扉,), 記憶,
        )
        self.assertEqual({x.ID for x in 統合}, {"扉認識"})

    def test_現在材料を評価してから不足時だけ候補関係観測が発火する(self) -> None:
        構文化器 = 公開HDSコンパイラ()
        中核 = HDS駆動コア(HDSコンパイラ=構文化器, 最大作用回数=40)
        供給器 = _空参照供給器()
        元評価 = HDS選択継承供給._評価
        評価時問合せ数: list[int] = []

        def 評価を監査(自己, 参照群):
            評価時問合せ数.append(len(供給器.問合せ))
            return 元評価(自己, 参照群)

        with patch.object(HDS選択継承供給, "_評価", new=評価を監査):
            中核.選択実行(
                "Which molecule inhibits Enzyme X?",
                ("Molecule A", "Molecule B"),
                初期参照=(),
                参照供給器=供給器,
                最大回復回数=2,
            )

        self.assertTrue(評価時問合せ数)
        self.assertEqual(評価時問合せ数[0], 0)
        self.assertGreater(len(供給器.問合せ), 0)

    def test_実観測参照は同一中核の次処理へ継承される(self) -> None:
        構文化器 = 公開HDSコンパイラ()
        中核 = HDS駆動コア(HDSコンパイラ=構文化器, 最大作用回数=40)
        問い = "Which molecule inhibits Enzyme X?"
        選択肢 = ("Molecule A", "Molecule B")
        観測 = 参照記録(
            識別子="memory-a",
            対象="Molecule A",
            内容="Molecule A inhibits Enzyme X.",
            由来="試験観測",
            供給器="試験",
            信頼=1.0,
        )

        中核.選択実行(問い, 選択肢, 初期参照=(観測,))
        self.assertGreaterEqual(中核.継続参照件数, 1)

        次 = 中核.選択実行(問い, 選択肢, 初期参照=())
        初回参照 = 次.状態.成果辞書().get(初回評価参照成果名, ())
        self.assertTrue(any(x.識別子 == "memory-a" for x in 初回参照))

    def test_継続状態は別中核へ漏れない(self) -> None:
        構文化器 = 公開HDSコンパイラ()
        問い = "Which molecule inhibits Enzyme X?"
        選択肢 = ("Molecule A", "Molecule B")
        観測 = 参照記録(
            識別子="memory-a",
            対象="Molecule A",
            内容="Molecule A inhibits Enzyme X.",
            由来="試験観測",
            供給器="試験",
            信頼=1.0,
        )
        第一 = HDS駆動コア(HDSコンパイラ=構文化器, 最大作用回数=40)
        第一.選択実行(問い, 選択肢, 初期参照=(観測,))
        self.assertGreaterEqual(第一.継続参照件数, 1)

        第二 = HDS駆動コア(HDSコンパイラ=公開HDSコンパイラ(), 最大作用回数=40)
        結果 = 第二.選択実行(問い, 選択肢, 初期参照=())
        初回参照 = 結果.状態.成果辞書().get(初回評価参照成果名, ())
        self.assertEqual(初回参照, ())
        self.assertEqual(第二.継続参照件数, 0)

    def test_確定認識が次実行の推論前提へ継承される(self) -> None:
        中核 = HDS駆動コア(最大作用回数=16)
        資料 = HDS資料("扉観測", "1", "扉は施錠されている", "試験")
        導出 = HDS認識項目(
            "扉施錠",
            "扉",
            "施錠",
            True,
            認識区分.確定,
            根拠=(資料.出典(),),
            検証契約="試験検証/v1",
        )

        初回 = 中核.実行(
            "観測から施錠状態を導出する",
            要求状態=("導出済み",),
            追加作用=(HDS関数作用(
                "導出",
                lambda 状態: HDS作用結果(
                    HDS作用状態.成立,
                    追加状態=frozenset({"導出済み"}),
                    記憶更新=状態.記憶.更新((資料,)),
                    認識更新=(導出,),
                ),
                出力状態=("導出済み",),
            ),),
        )
        self.assertEqual(初回.終端, HDS終端.採用)
        self.assertEqual(中核.継続認識件数, 1)

        def 前提を使う(状態):
            項目 = 状態.認識辞書().get("扉施錠")
            if 項目 is None or 項目.値 is not True:
                return HDS作用結果(HDS作用状態.保留)
            return HDS作用結果(HDS作用状態.成立, 追加状態=frozenset({"事前確認必要"}))

        次回 = 中核.実行(
            "施錠経験から事前確認を行う",
            要求状態=("事前確認必要",),
            追加作用=(HDS関数作用("前提利用", 前提を使う, 出力状態=("事前確認必要",)),),
        )
        self.assertEqual(次回.終端, HDS終端.採用)

    def test_根拠更新で旧認識を次実行前提から再開放する(self) -> None:
        中核 = HDS駆動コア(最大作用回数=16)
        旧資料 = HDS資料("扉観測", "1", "扉は施錠されている", "試験")
        新資料 = HDS資料("扉観測", "2", "扉は解錠されている", "試験")
        導出 = HDS認識項目(
            "扉施錠",
            "扉",
            "施錠",
            True,
            認識区分.確定,
            根拠=(旧資料.出典(),),
            検証契約="試験検証/v1",
        )
        中核.実行(
            "施錠認識を形成する",
            要求状態=("形成済み",),
            追加作用=(HDS関数作用(
                "形成",
                lambda 状態: HDS作用結果(
                    HDS作用状態.成立,
                    追加状態=frozenset({"形成済み"}),
                    記憶更新=状態.記憶.更新((旧資料,)),
                    認識更新=(導出,),
                ),
                出力状態=("形成済み",),
            ),),
        )
        self.assertEqual(中核.継続認識件数, 1)

        更新 = 中核.実行(
            "反証資料へ更新する",
            要求状態=("更新済み",),
            追加作用=(HDS関数作用(
                "資料更新",
                lambda 状態: HDS作用結果(
                    HDS作用状態.成立,
                    追加状態=frozenset({"更新済み"}),
                    記憶更新=状態.記憶.更新((新資料,)),
                ),
                出力状態=("更新済み",),
            ),),
        )
        self.assertEqual(更新.終端, HDS終端.採用)
        self.assertEqual(中核.継続認識件数, 0)

    def test_HDS記憶は次実行の処理前提になる(self) -> None:
        中核 = HDS駆動コア(最大作用回数=16)
        資料 = HDS資料("経験資料", "1", "扉は施錠されていた", "試験")

        def 記憶する(状態):
            return HDS作用結果(
                HDS作用状態.成立,
                追加状態=frozenset({"記憶済み"}),
                記憶更新=状態.記憶.更新((資料,)),
            )

        初回 = 中核.実行(
            "経験を保持する",
            要求状態=("記憶済み",),
            追加作用=(HDS関数作用("経験保持", 記憶する, 出力状態=("記憶済み",)),),
        )
        self.assertEqual(初回.終端, HDS終端.採用)

        def 記憶を使う(状態):
            if "経験資料" not in 状態.記憶.正本辞書():
                return HDS作用結果(HDS作用状態.保留)
            return HDS作用結果(HDS作用状態.成立, 追加状態=frozenset({"経験利用済み"}))

        次回 = 中核.実行(
            "保持経験を後続処理で使う",
            要求状態=("経験利用済み",),
            追加作用=(HDS関数作用("経験利用", 記憶を使う, 出力状態=("経験利用済み",)),),
        )
        self.assertEqual(次回.終端, HDS終端.採用)

    def test_実行経験から形成した適応が同一中核の次実行へ継承される(self) -> None:
        中核 = HDS駆動コア(最大作用回数=24)
        作用群 = (
            HDS関数作用(
                "段1",
                lambda 状態: HDS作用結果(HDS作用状態.成立, 追加状態=frozenset({"途中"})),
                出力状態=("途中",),
                純粋作用=True,
            ),
            HDS関数作用(
                "段2",
                lambda 状態: HDS作用結果(HDS作用状態.成立, 追加状態=frozenset({"完了"})),
                入力状態=("途中",),
                出力状態=("完了",),
                純粋作用=True,
            ),
        )

        初回 = 中核.実行("手順経験", 要求状態=("完了",), 追加作用=作用群)
        self.assertEqual(初回.終端, HDS終端.採用)
        self.assertTrue(初回.状態.形成関係)

        次回 = 中核.実行("手順経験", 要求状態=("完了",), 追加作用=作用群)
        self.assertEqual(次回.終端, HDS終端.採用)
        self.assertGreater(次回.計装.形成再利用数, 0)

    def test_問固有の検索経路印は次経験へ持ち越さない(self) -> None:
        中核 = HDS駆動コア(HDSコンパイラ=公開HDSコンパイラ(), 最大作用回数=40)
        観測 = 参照記録(
            "r-route",
            "Molecule A",
            "Molecule A inhibits Enzyme X.",
            "試験",
            "試験",
            1.0,
            条件=(("hds_query_選択肢", "A"), ("hds_observation_id", "old"), ("領域条件", "保持")),
        )
        中核.選択実行(
            "Which molecule inhibits Enzyme X?",
            ("Molecule A", "Molecule B"),
            初期参照=(観測,),
        )
        記憶 = next(x for x in 中核.継続参照記憶 if x.識別子 == "r-route")
        self.assertNotIn(("hds_query_選択肢", "A"), 記憶.条件)
        self.assertNotIn(("hds_observation_id", "old"), 記憶.条件)
        self.assertIn(("領域条件", "保持"), 記憶.条件)

    def test_継続状態を明示初期化できる(self) -> None:
        中核 = HDS駆動コア(HDSコンパイラ=公開HDSコンパイラ(), 最大作用回数=40)
        観測 = 参照記録("r", "Molecule A", "Molecule A inhibits Enzyme X.", "試験", "試験", 1.0)
        中核.選択実行(
            "Which molecule inhibits Enzyme X?",
            ("Molecule A", "Molecule B"),
            初期参照=(観測,),
        )
        self.assertGreater(中核.継続参照件数, 0)
        中核.継続状態を初期化()
        self.assertEqual(中核.継続記憶資料件数, 0)
        self.assertEqual(中核.継続認識件数, 0)
        self.assertEqual(中核.継続形成関係件数, 0)
        self.assertEqual(中核.適応経験数, 0)
        self.assertEqual(中核.継続参照件数, 0)


if __name__ == "__main__":
    unittest.main()

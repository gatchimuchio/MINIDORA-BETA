"""HDSの実不足・再計画・終端照合を人工通信で検査する。正式評価ではない。"""
from copy import deepcopy
from dataclasses import replace
import unittest

from minidora.HDS運用.セッション import HDS運用セッション
from minidora.HDS運用.工程 import 工程供給, 現行計画
from minidora.HDS運用.資料補充 import 資料補充を検査
from minidora.HDS運用.資料回答 import 資料内容不足, 資料内容不足を読む
from minidora.HDS運用.意味資料 import 資料意味を選ぶ
from minidora.HDS運用.内容構成 import 内容仕様を構成
from minidora.HDS運用.一般依頼 import 一般計画を作る, _一般会話計画を作る
from minidora.HDS運用.製品 import HDS製品ミニドラ
from minidora.HDS運用.値 import 結果を復元, 指紋
from minidora.知識取得 import 知識取得器, _記録印
from minidora.知識取得継続 import 継続束を読む, 継続不成立を検査
from minidora.取得実行制御 import 公開取得上限
from minidora.製品版.型 import 能力結果, 参照資料
from test_知識取得継続 import 人工検索, 人工本文
from test_基本チャット内容 import 要求


def 応答する(本文群=None, 上限=None):
    本文 = 人工本文(本文群)
    検索 = 人工検索(len(本文.本文群))
    会話 = HDS運用セッション("補充", 外部読取許可=True, 手順形成=False,
                     取得器=知識取得器(検索, 本文), 取得上限=上限 or 公開取得上限(最大検索回数=1))
    応答 = 会話.応答("試料甲とは何ですか", 外部読取許可=True)
    return 会話, 検索, 本文, 応答


class 再探索検索(人工検索):
    def __init__(self, *, 重複=False, 初回定義=False):
        super().__init__(6)
        self.重複, self.初回定義 = 重複, 初回定義

    def 検索(self, 語, limit=5):
        self.回数.append(語)
        群 = ([0, 1, 2, 3, 4, 5] if self.重複 else [6, 7, 8, 9, 10, 11]) if 語 == "試料甲 とは" or self.初回定義 else list(range(6))
        return tuple(参照資料(str(i), "試料甲", "人工候補", "https://example.com/" + str(i),
                         本文="試料甲とは誤ったスニペット999。") for i in 群)[:limit]


def 再探索する(*, 上限=None, 重複=False, 初回定義=False):
    本文 = 人工本文(["試料甲の方式は断熱収納。", "試料甲の素材は樹脂。",
                   "試料甲の形状は円筒。", "試料甲の厚さは二センチ。",
                   "試料甲の重さは軽い。", "試料甲の色は青。",
                   "試料甲とは熱の移動を抑える容器である。"])
    検索 = 再探索検索(重複=重複, 初回定義=初回定義)
    会話 = HDS運用セッション("再探索", 外部読取許可=True, 手順形成=False,
                     取得器=知識取得器(検索, 本文), 取得上限=上限 or 公開取得上限(最大候補数=12))
    応答 = 会話.応答("試料甲とは何ですか", 外部読取許可=True)
    return 会話, 検索, 本文, 応答


class HDS資料補充試験(unittest.TestCase):
    def test_初回六候補と旧モードを保ちMVP既定の候補総数を有限にする(self):
        入力 = {"原文": "開発用要求", "外部許可": True, "資料": {}, "前回有効": False}
        定義 = 要求(主題=("試料甲",), 関係焦点=("定義",))
        for 設定, 配分 in ((定義, [6]),
                         (要求(主題=("試料甲",)), [6]),
                         (要求(操作="理由", 主題=("試料甲",), 関係焦点=("作動する",)), [6]),
                         (要求(操作="対照", 比較対象=("試料甲", "試料乙")), [6, 6])):
            with self.subTest(設定=設定):
                計画, 素材, _ = 一般計画を作る({"方式": "一般資料", "要求": 設定}, 入力)
                self.assertEqual([素材[x.設定参照].データ["最大候補数"]
                                  for x in 計画.工程 if x.能力候補 == ("知識取得",)], 配分)
        # 目的束の子が呼ぶ既存入口は、単独定義でも配分を変更しない。
        _, 素材, _ = _一般会話計画を作る(定義, 入力)
        self.assertEqual(素材["一般:取得設定:0"].データ["最大候補数"], 6)
        self.assertIsNone(HDS製品ミニドラ()._取得上限)
        既定 = HDS製品ミニドラ(基本チャット=True, 公開取得方針="off")._取得上限
        self.assertEqual((既定.最大検索回数, 既定.最大候補数, 既定.最大本文取得数), (2, 12, 12))
        明示 = 公開取得上限(最大候補数=6)
        self.assertIs(HDS製品ミニドラ(基本チャット=True, 公開取得方針="off", 取得上限=明示)._取得上限, 明示)

    def test_定義不足から追加検索し旧取得を保持して終端照合する(self):
        会話, 検索, 本文, 応答 = 再探索する()
        self.assertTrue(応答.成立, 応答.本文)
        self.assertIn("熱の移動を抑える容器", 応答.本文)
        self.assertNotIn("999", 応答.本文)
        self.assertEqual(検索.回数, ["試料甲 定義", "試料甲 とは"])
        self.assertEqual(len(本文.回数), 4)
        状態 = 応答.HDS結果.状態
        値 = dict(状態.成果)
        鍵, 包 = 現行計画(状態)
        self.assertEqual(鍵, "運用計画:3")
        self.assertTrue(資料補充を検査(鍵, 包, 値, 会話.目録, 4))
        self.assertTrue(工程供給(会話.目録).最終検証(状態, None))
        元鍵 = 包["資料補充"]["前計画鍵"]
        旧工程 = 包["資料補充"]["取得工程"]
        self.assertEqual(値[元鍵 + "/成果/" + 旧工程], 値[鍵 + "/成果/" + 旧工程])
        新工程 = 包["資料補充"]["再検索工程"]
        self.assertEqual(結果を復元(値[鍵 + "/成果/" + 新工程]).データ["資料数"], 1)
        self.assertEqual(応答.追跡["公開取得資源"]["使用"]["検索回数"], 2)
        self.assertEqual(応答.追跡["公開取得資源"]["使用"]["候補数"], 12)
        for 改変 in ("検索語", "旧成果", "当回", "検索数", "候補未計上", "候補超過", "候補真偽値", "本文数", "証明"):
            with self.subTest(改変=改変):
                変更 = deepcopy(値)
                if 改変 == "検索語":
                    変更[鍵]["資料"][包["資料補充"]["素材名"]]["内容"]["データ"]["検索語"] = "別の主題 とは"
                elif 改変 == "旧成果":
                    変更[鍵 + "/成果/" + 旧工程]["内容"]["本文"] = "旧取得を置換"
                elif 改変 == "証明":
                    del 変更[鍵]["資料補充"]["再検索工程"]
                else:
                    d = 変更[鍵 + "/成果/" + 新工程]["内容"]["データ"]
                    if 改変 == "当回":
                        d["資源制御"]["当回ID"] = "別の当回"
                    elif 改変 == "検索数":
                        d["資源制御"]["使用"]["検索回数"] = 1
                    elif 改変 == "候補未計上":
                        d["資源制御"]["使用"]["候補数"] = 6
                    elif 改変 == "候補超過":
                        d["資源制御"]["使用"]["候補数"] = 13
                    elif 改変 == "候補真偽値":
                        d["資源制御"]["使用"]["候補数"] = True
                    else:
                        d["資源制御"]["使用"]["本文取得数"] = 5
                    d["記録SHA256"] = _記録印({k: v for k, v in d.items() if k != "記録SHA256"})
                変更["運用採用対応"]["計画印"] = 指紋(変更[鍵])
                with self.assertRaises((ValueError, KeyError)):
                    資料補充を検査(鍵, 変更[鍵], 変更, 会話.目録, 4)
                self.assertFalse(工程供給(会話.目録).最終検証(replace(状態, 成果=tuple(変更.items())), None))

    def test_初回内容成立と検索資源枯渇では追加検索しない(self):
        for 設定, 期待 in (({"初回定義": True}, True),
                         ({"上限": 公開取得上限(最大検索回数=1)}, False)):
            with self.subTest(設定=設定):
                _, 検索, _, 応答 = 再探索する(**設定)
                self.assertEqual(応答.成立, 期待, 応答.本文)
                self.assertEqual(len(検索.回数), 1)

    def test_追加検索の重複原本文を内容進展として採用しない(self):
        会話, 検索, 本文, 応答 = 再探索する(重複=True)
        self.assertEqual(応答.状態, "SUSPEND", 応答.本文)
        self.assertEqual(len(検索.回数), 2)
        self.assertEqual(len(本文.回数), 4)
        self.assertFalse(会話.状態()["前回有効"])

    def test_初回取得不成立は既存探索の上限で止まり内容不足を偽装しない(self):
        検索 = 再探索検索()
        本文 = 人工本文(["主題未掲載の資料。"] * 6)
        会話 = HDS運用セッション("未掲載", 外部読取許可=True, 手順形成=False,
                     取得器=知識取得器(検索, 本文), 取得上限=公開取得上限())
        応答 = 会話.応答("試料甲とは何ですか", 外部読取許可=True)
        self.assertEqual(応答.状態, "SUSPEND", 応答.本文)
        # 6本文の取得枠を使い切るので、次検索を始めず停止する。
        self.assertEqual(len(検索.回数), 1)
        self.assertNotIn("試料甲 とは", 検索.回数)
        self.assertEqual(応答.追跡["公開取得資源"]["使用"]["候補数"], 6)
        self.assertEqual(len(本文.回数), 6)
        self.assertFalse(会話.状態()["前回有効"])

    def test_先頭三本文が不成立でも第四候補の旧成立を失わない(self):
        検索 = 再探索検索()
        本文 = 人工本文(["主題未掲載の資料。"] * 3 +
                      ["試料甲とは熱の移動を抑える容器である。"])
        会話 = HDS運用セッション("第四候補", 外部読取許可=True, 手順形成=False,
                     取得器=知識取得器(検索, 本文), 取得上限=公開取得上限())
        応答 = 会話.応答("試料甲とは何ですか", 外部読取許可=True)
        self.assertTrue(応答.成立, 応答.本文)
        self.assertIn("熱の移動を抑える容器", 応答.本文)
        self.assertEqual(len(検索.回数), 1)
        self.assertEqual(len(本文.回数), 4)

    def test_内容不足から同じHDSが次本文を読み終端まで照合する(self):
        会話, 検索, 本文, 応答 = 応答する(["試料甲の方式は断熱収納。", "試料甲の素材は樹脂。",
                                "試料甲とは熱の移動を抑える容器である。"])
        self.assertTrue(応答.成立, 応答.本文)
        self.assertIn("熱の移動を抑える容器", 応答.本文)
        self.assertNotIn("999", 応答.本文)
        self.assertEqual(len(検索.回数), 1)
        self.assertEqual(len(本文.回数), 3)
        状態 = 応答.HDS結果.状態
        値 = dict(状態.成果)
        鍵, 包 = 現行計画(状態)
        self.assertEqual(鍵, "運用計画:2")
        self.assertTrue(資料補充を検査(鍵, 包, 値, 会話.目録, 4))
        自己検証 = 工程供給(会話.目録)
        self.assertTrue(自己検証.最終検証(状態, None))
        旧不足 = 結果を復元(値["運用計画:0/試行/一般内容構成/0"]["不成立成果"])
        self.assertFalse(旧不足.成立)
        self.assertEqual(旧不足.データ["診断"]["不足役割"][0]["種別"], "定義・種別関係")
        self.assertEqual(応答.追跡["公開取得資源"]["使用"]["検索回数"], 1)
        self.assertEqual(応答.追跡["公開取得資源"]["使用"]["候補数"], 3)
        # 新packetだけを再封印した候補差替え・不足診断改変も旧実成果から拒否する。
        for 改変 in ("候補", "診断", "対象外", "証明削除", "途中切断", "継続への包み直し"):
            with self.subTest(改変=改変):
                変更 = deepcopy(値)
                if 改変 == "候補":
                    元 = 変更[鍵]["資料"][包["資料補充"]["素材名"]]["内容"]["データ"]
                    元["継続束"]["候補"][1]["URL"] = "https://example.com/changed"
                    元["記録SHA256"] = _記録印({k: v for k, v in 元.items() if k != "記録SHA256"})
                elif 改変 == "診断":
                    変更["運用計画:0/試行/一般内容構成/0"]["不成立成果"]["内容"]["データ"]["診断"]["対象"] = ["別の主題"]
                elif 改変 == "対象外":
                    変更[鍵]["原文"] = "別の問い"
                elif 改変 == "証明削除":
                    del 変更[鍵]["資料補充"]
                elif 改変 == "途中切断":
                    del 変更["運用計画:1"]["資料補充"]
                else:
                    del 変更[鍵]["資料補充"]
                    変更[鍵]["解釈"] = {"方式": "継続", "目的": 変更[鍵]["解釈"]}
                変更["運用採用対応"]["計画印"] = 指紋(変更[鍵])
                with self.assertRaises((ValueError, KeyError)):
                    資料補充を検査(鍵, 変更[鍵], 変更, 会話.目録, 4)
                偽状態 = replace(状態, 成果=tuple(変更.items()))
                self.assertFalse(自己検証.最終検証(偽状態, None))

    def test_意味不足と候補枯渇は保留して採用原本を残さない(self):
        会話, 検索, 本文, 応答 = 応答する(["試料甲の方式は断熱収納。", "試料甲の素材は樹脂。"])
        self.assertEqual(応答.状態, "SUSPEND", 応答.本文)
        self.assertFalse(会話.状態()["前回有効"])
        self.assertEqual(len(検索.回数), 1)
        self.assertEqual(len(本文.回数), 2)
        self.assertNotIn("運用応答", dict(応答.HDS結果.状態.成果))

    def test_共有資源不足では追加取得も採用もしない(self):
        会話, 検索, 本文, 応答 = 応答する(上限=公開取得上限(最大検索回数=1, 最大本文取得数=1))
        self.assertEqual(応答.状態, "SUSPEND", 応答.本文)
        self.assertFalse(会話.状態()["前回有効"])
        self.assertEqual(len(検索.回数), 1)
        self.assertEqual(len(本文.回数), 1)
        self.assertEqual(応答.追跡["公開取得資源"]["使用"]["本文取得数"], 1)

    def test_旧候補の実不成立から再検索し原不足と旧成功を保持する(self):
        検索 = 再探索検索()
        本文 = 人工本文(["試料甲の方式は断熱収納。"] + ["主題未掲載の資料。"] * 5 +
                      ["試料甲とは熱の移動を抑える容器である。"])
        会話 = HDS運用セッション("不成立から再探索", 外部読取許可=True, 手順形成=False,
                     取得器=知識取得器(検索, 本文),
                     取得上限=公開取得上限(最大候補数=12, 最大本文取得数=12))
        応答 = 会話.応答("試料甲とは何ですか", 外部読取許可=True)
        self.assertTrue(応答.成立, 応答.本文)
        self.assertIn("熱の移動を抑える容器", 応答.本文)
        self.assertEqual(検索.回数, ["試料甲 定義", "試料甲 とは"])
        self.assertEqual(本文.回数, ["https://example.com/" + str(i) for i in range(7)])
        状態, 値 = 応答.HDS結果.状態, dict(応答.HDS結果.状態.成果)
        鍵, 包 = 現行計画(状態)
        self.assertEqual(鍵, "運用計画:2")
        証明 = 包["資料補充"]
        失敗鍵 = 証明["取得失敗後再検索"]["不成立試行鍵"]
        失敗 = 結果を復元(値[失敗鍵]["不成立成果"])
        元鍵 = 証明["成果復帰"][証明["取得工程"]]
        元 = 結果を復元(値[元鍵])
        要求設定 = 元.データ["要求"]
        from minidora.知識取得接続 import 知識取得モジュール
        from minidora.製品版.能力契約 import 能力文脈
        要求_ = 知識取得モジュール._要求(能力文脈("", "", 補助={"合成設定": 要求設定}))
        self.assertFalse(失敗.成立)
        self.assertEqual(失敗.本文, "")
        self.assertEqual(失敗.保留理由, "未消費候補がありません")
        self.assertEqual((元.データ["本文取得数"], 失敗.データ["本文取得数"]), (1, 6))
        self.assertTrue(継続不成立を検査(元, 失敗, 要求_))
        with self.assertRaises(ValueError):
            継続束を読む(失敗, 要求_)
        self.assertEqual(値[元鍵], 値[鍵 + "/成果/" + 証明["取得工程"]])
        self.assertEqual(元.データ["資源制御"]["使用"]["本文取得数"], 1)
        self.assertEqual(失敗.データ["資源制御"]["使用"]["本文取得数"], 6)
        self.assertEqual(応答.追跡["公開取得資源"]["使用"]["本文取得数"], 7)
        self.assertTrue(資料補充を検査(鍵, 包, 値, 会話.目録, 4))
        self.assertTrue(工程供給(会話.目録).最終検証(状態, None))
        for 改変 in ("復帰元", "元不足", "失敗状態", "親記録", "消費位置", "旧本文", "検索数", "本文数", "本文真偽値"):
            with self.subTest(改変=改変):
                変更 = deepcopy(値)
                if 改変 == "復帰元":
                    変更[鍵]["資料補充"]["成果復帰"][証明["取得工程"]] = 鍵 + "/成果/" + 証明["再検索工程"]
                elif 改変 == "元不足":
                    変更[証明["不足試行鍵"]]["不成立成果"]["内容"]["データ"]["診断"]["対象"] = ["別主題"]
                else:
                    保存 = 変更[失敗鍵]["不成立成果"]
                    d = 保存["内容"]["データ"]
                    if 改変 == "失敗状態": d["状態"] = "合格"
                    elif 改変 == "親記録": d["継続束"]["親記録SHA256"] = "0" * 64
                    elif 改変 == "消費位置": d["継続束"]["消費位置"] = 5
                    elif 改変 == "旧本文": d["資料"][0]["題名"] = "原値変更"
                    elif 改変 == "検索数": d["資源制御"]["使用"]["検索回数"] = 2
                    elif 改変 == "本文数": d["資源制御"]["使用"]["本文取得数"] = 5
                    else: d["資源制御"]["使用"]["本文取得数"] = True
                    d["記録SHA256"] = _記録印({k: v for k, v in d.items() if k != "記録SHA256"})
                    変更[失敗鍵]["出力印"] = 指紋(保存)
                変更["運用採用対応"]["計画印"] = 指紋(変更[鍵])
                with self.assertRaises((ValueError, KeyError)):
                    資料補充を検査(鍵, 変更[鍵], 変更, 会話.目録, 4)
                self.assertFalse(工程供給(会話.目録).最終検証(replace(状態, 成果=tuple(変更.items())), None))

    def test_不成立の旧候補を再試行せず第二検索の不足は留保する(self):
        for 総本文枠, 重複, 期待検索, 期待本文 in ((12, True, 2, 7), (6, False, 1, 6)):
            with self.subTest(総本文枠=総本文枠):
                検索 = 再探索検索(重複=重複)
                本文 = 人工本文(["試料甲の方式は断熱収納。"] + ["主題未掲載の資料。"] * 5 +
                              ["試料甲とは熱の移動を抑える容器である。"])
                会話 = HDS運用セッション("消費済み候補", 外部読取許可=True, 手順形成=False,
                             取得器=知識取得器(検索, 本文),
                             取得上限=公開取得上限(最大候補数=12, 最大本文取得数=総本文枠))
                応答 = 会話.応答("試料甲とは何ですか", 外部読取許可=True)
                self.assertEqual(応答.状態, "SUSPEND", 応答.本文)
                self.assertEqual(len(検索.回数), 期待検索)
                self.assertEqual(len(本文.回数), 期待本文)
                self.assertFalse(会話.状態()["前回有効"])
                self.assertEqual(本文.回数.count("https://example.com/1"), 1)

    def test_重複候補不成立から再検索しても通信とバイト消費を巻き戻さない(self):
        class 重複検索(再探索検索):
            def 検索(self, 語, limit=5):
                候補 = super().検索(語, limit=limit)
                return 候補 if 語 == "試料甲 とは" else 候補[:1] * 6
        class 計数本文(人工本文):
            def 制御を束縛(self, 制御):
                self.制御 = 制御
                return self

            def 取得(self, URL):
                self.制御.通信を開始()
                文書 = super().取得(URL)
                self.制御.受信を記録(文書.転送バイト数)
                return 文書
        本文 = 計数本文(["試料甲の方式は断熱収納。"] * 6 +
                      ["試料甲とは熱の移動を抑える容器である。"])
        検索 = 重複検索()
        会話 = HDS運用セッション("重複消費", 外部読取許可=True, 手順形成=False,
                     取得器=知識取得器(検索, 本文),
                     取得上限=公開取得上限(最大候補数=12, 最大本文取得数=12))
        応答 = 会話.応答("試料甲とは何ですか", 外部読取許可=True)
        self.assertTrue(応答.成立, 応答.本文)
        self.assertEqual(検索.回数, ["試料甲 定義", "試料甲 とは"])
        self.assertEqual(本文.回数, ["https://example.com/0", "https://example.com/6"])
        状態 = 応答.HDS結果.状態
        値 = dict(状態.成果)
        鍵, 包 = 現行計画(状態)
        証明 = 包["資料補充"]
        失敗鍵 = 証明["取得失敗後再検索"]["不成立試行鍵"]
        失敗 = 結果を復元(値[失敗鍵]["不成立成果"])
        使用 = 失敗.データ["資源制御"]["使用"]
        self.assertEqual((使用["本文取得数"], 使用["HTTP開始数"]), (1, 1))
        self.assertGreater(使用["総バイト数"], 0)
        self.assertTrue(資料補充を検査(鍵, 包, 値, 会話.目録, 4))
        self.assertTrue(工程供給(会話.目録).最終検証(状態, None))
        for 項目 in ("HTTP開始数", "総バイト数"):
            with self.subTest(項目=項目):
                変更 = deepcopy(値)
                保存 = 変更[失敗鍵]["不成立成果"]
                d = 保存["内容"]["データ"]
                d["資源制御"]["使用"][項目] = 0
                d["記録SHA256"] = _記録印({k: v for k, v in d.items() if k != "記録SHA256"})
                変更[失敗鍵]["出力印"] = 指紋(保存)
                with self.assertRaisesRegex(ValueError, "実消費と共有資源"):
                    資料補充を検査(鍵, 包, 変更, 会話.目録, 4)
                self.assertFalse(工程供給(会話.目録).最終検証(replace(状態, 成果=tuple(変更.items())), None))

    def test_定義不足の診断を元要求と原作用成果から再導出する(self):
        構造 = 資料意味を選ぶ({"人工資料": 能力結果(True, "試料甲の方式は断熱収納。")},
                    要求(主題=("試料甲",), 関係焦点=("定義",)))
        with self.assertRaises(資料内容不足) as 捕捉:
            内容仕様を構成(構造)
        不足 = 捕捉.exception
        self.assertEqual(資料内容不足を読む(不足.診断, 構造), 不足.診断)
        self.assertEqual(不足.診断["要求"], 構造["要求"])
        self.assertTrue(不足.診断["資料作用成果"])
        偽 = deepcopy(不足.診断)
        偽["必要確認"] = []
        with self.assertRaises(ValueError):
            資料内容不足を読む(偽, 構造)


if __name__ == "__main__":
    unittest.main()

"""会話作用・原文対応の開発契約試験。最終初見受入の題材ではない。"""
from copy import deepcopy
import unittest
from minidora.HDS構文化器_v1 import 公開HDSコンパイラ
from minidora.HDS適合器 import HDS文脈
from minidora.HDS会話意味射影 import 会話意味欄を読む, 会話射影由来
from minidora.HDS運用.一般依頼 import 一般依頼を読む, 一般要求を検査, 一般計画を作る, 一般会話要求版
from minidora.HDS運用.意味資料 import 資料意味を選ぶ
from minidora.HDS運用.内容構成 import _文章, 資料文章を照合
from minidora.HDS運用.値 import 結果を保存


class 会話意味契約試験(unittest.TestCase):
    def test_疑問の終助詞を述部や否定方向へ混入しない(self):
        for 述部, 方向 in (("作動しない", "否定"), ("少ない", "肯定"), ("危ない", "肯定")):
            with self.subTest(述部=述部):
                _, 読解 = self.読む("なぜ装置ネリスは" + 述部 + "の？")
                self.assertEqual(読解["要求"]["関係焦点"], [述部])
                self.assertEqual(読解["要求"]["肯否"], 方向)

    def 読む(self, 文, 前=None, *, 有効=True):
        束 = 公開HDSコンパイラ().コンパイル束(文, 文脈=HDS文脈(現在焦点=前))
        return 束, 一般依頼を読む({"原文": 文, "前回目的": 前, "前回有効": 有効}, 意味IR=束.意味IR)

    def test_未知の名詞と語尾は同じ開いた主題へ(self):
        for 名 in ("架空装置甲", "リュミエル石", "おとぎ話", "未知材料乙"):
            for 文 in (名 + "とは何ですか？", 名 + "って何？", 名 + "について教えてください", 名 + "を説明して"):
                with self.subTest(文=文):
                    束, 読解 = self.読む(文)
                    self.assertEqual(読解["要求"]["主題"], [名])
                    self.assertEqual(読解["要求"]["操作"], ["説明"])
                    self.assertEqual(読解["要求"]["版"], 一般会話要求版)
                    self.assertEqual(束.意味IR.原文, 文)

    def test_引用した主題を接続詞で分断しない(self):
        _, 読解 = self.読む('「おとぎ話」について説明して')
        self.assertEqual(読解["要求"]["主題"], ["おとぎ話"])

    def test_比較の対象順と観点を保持(self):
        for 左, 右 in (("試作器甲", "試作器乙"), ("試作器乙", "試作器甲"), ("未知材料一", "未知材料二")):
            for 文 in (左 + "と" + 右 + "の違いを教えて", 左 + "と" + 右 + "はどう違う？",
                       左 + "と" + 右 + "はどのように違いますか？"):
                _, 読解 = self.読む(文)
                self.assertEqual(読解["要求"]["比較対象"], [左, 右])
                self.assertEqual(読解["要求"]["操作"], ["対照"])
        _, 読解 = self.読む("試作器甲と試作器乙を保守性の観点で比較して")
        self.assertEqual(読解["要求"]["観点"], ["保守性"])
        _, 読解 = self.読む("試作器甲と試作器乙を保守性で比較して")
        self.assertEqual(読解["要求"]["比較対象"], ["試作器甲", "試作器乙"])
        self.assertEqual(読解["要求"]["観点"], ["保守性"])

    def test_属性疑問と比較疑問を定義に取り違えない(self):
        _, 読解 = self.読む("未知材料乙の特徴は何ですか？")
        self.assertEqual(読解["要求"]["主題"], ["未知材料乙"])
        self.assertEqual(読解["要求"]["関係焦点"], ["特徴", "何"])
        for 文 in ("試作器甲と試作器乙は何が違いますか？", "試作器甲と試作器乙の違いは何ですか？"):
            _, 読解 = self.読む(文)
            self.assertEqual(読解["要求"]["比較対象"], ["試作器甲", "試作器乙"])
            self.assertEqual(読解["要求"]["操作"], ["対照"])

    def test_既知形式と要点を主題や本文へ混入しない(self):
        for 文 in ("試作器甲を箇条書きで説明してください", "箇条書きで試作器甲を説明して"):
            _, 読解 = self.読む(文)
            self.assertEqual(読解["要求"]["主題"], ["試作器甲"])
            self.assertEqual(読解["要求"]["形式"], "箇条書き")
        本文 = "試作器甲は人工物である。"
        for 制御, 操作, 形式 in (("この文章の要点を教えて", "要約", "文章"),
                                  ("次の文を箇条書きで説明してください", "説明", "箇条書き")):
            _, 読解 = self.読む(制御 + ":\n" + 本文)
            self.assertEqual(読解["要求"]["主題"], [])
            self.assertEqual(読解["要求"]["操作"], [操作])
            self.assertEqual(読解["要求"]["形式"], 形式)
            self.assertEqual(読解["要求"]["提示本文"], 本文)
        with self.assertRaisesRegex(ValueError, "競合"):
            self.読む("試作器甲を箇条書きで説明して、文章にしてください")

    def test_引用内の句境界と制御語は主題として保持(self):
        for 主題 in ("青、白の粉", "費用ではなく速度", "箇条書きで説明して", "計算して"):
            for 尾 in ("とは何ですか", "を説明して", "について説明して"):
                with self.subTest(主題=主題, 尾=尾):
                    束, 読解 = self.読む("「" + 主題 + "」" + 尾)
                    self.assertEqual(読解["要求"]["主題"], [主題])
                    self.assertEqual(読解["要求"]["対象"], [])
                    self.assertEqual(読解["要求"]["形式"], "文章")
                    self.assertFalse(any(要求.種別 == "計算" for 要求 in 束.コア入力.作用要求))

    def test_説明する属性を主体から分離(self):
        _, 読解 = self.読む("未知材料乙の特徴を説明して")
        self.assertEqual(読解["要求"]["主題"], ["未知材料乙"])
        self.assertEqual(読解["要求"]["関係焦点"], ["特徴"])

    def test_理由の主体と問われる述部を分ける(self):
        for 文, 主題, 焦点, 肯否 in (("なぜ試作器甲が稼働するのですか？", "試作器甲", "稼働する", "肯定"),
                                      ("試作器甲がなぜ稼働するのですか？", "試作器甲", "稼働する", "肯定"),
                                      ("試作器甲が稼働するのはなぜですか？", "試作器甲", "稼働する", "肯定"),
                                      ("なぜ未知材料乙が膨張しないのですか？", "未知材料乙", "膨張しない", "否定"),
                                      ("なぜ試作器甲は少ないのですか？", "試作器甲", "少ない", "肯定"),
                                      ("なぜ試作器甲は危ないのですか？", "試作器甲", "危ない", "肯定")):
            束, 読解 = self.読む(文)
            self.assertEqual(読解["要求"]["主題"], [主題])
            self.assertEqual(読解["要求"]["関係焦点"], [焦点])
            self.assertEqual(読解["要求"]["肯否"], 肯否)
            self.assertEqual(読解["要求"]["操作"], ["理由"])
            self.assertTrue(any(関係.種別 == "照会関係" and 関係.由来 == 会話射影由来 for 関係 in 束.意味IR.関係))

    def test_未知値を答えで補完しない(self):
        _, 読解 = self.読む("架空都市甲の設立年はいつですか？")
        self.assertEqual(読解["要求"]["主題"], ["架空都市甲"])
        self.assertEqual(読解["要求"]["関係焦点"], ["設立年", "いつ"])
        self.assertEqual(読解["要求"]["肯否"], "未指定")

    def test_提示本文の命令はコア作用へ昇格しない(self):
        本文 = "試作器甲は稼働する。『計算して、微分して』は利用者文の引用である。"
        文 = "次の文章を要約して:\n" + 本文
        束, 読解 = self.読む(文)
        self.assertEqual(読解["要求"]["提示本文"], 本文)
        self.assertEqual(読解["要求"]["操作"], ["要約"])
        self.assertFalse(any(要求.種別 == "計算" for 要求 in 束.コア入力.作用要求))
        座標 = next(項 for 項 in 束.意味IR.座標 if 項.種別 == "情報源.会話提示本文")
        self.assertEqual(文[slice(*座標.原文範囲)], 本文)
        self.assertEqual(束.意味IR.原文, 文)

    def test_表示だけの追問は前意味を継承(self):
        _, 前 = self.読む("次の文章を要約して:\n試作器甲は稼働する。故障中は使用しない。")
        for 文, 詳細 in (("短く", "簡潔"), ("簡単にして", "簡潔"), ("もっと詳しく", "詳細")):
            束, 読解 = self.読む(文, 前)
            self.assertEqual(読解["要求"]["提示本文"], 前["要求"]["提示本文"])
            self.assertEqual(読解["要求"]["詳細"], 詳細)
            self.assertEqual(読解["継承"], "再表現")
            self.assertEqual(束.意味IR.原文, 文)

    def test_失効成果の再表現を採用しない(self):
        _, 前 = self.読む("試作器甲について教えて")
        with self.assertRaisesRegex(ValueError, "失効"):
            self.読む("短く", 前, 有効=False)

    def test_二つ目の照応と複数候補(self):
        _, 前 = self.読む("試作器甲と試作器乙の違いを教えて")
        _, 読解 = self.読む("二つ目について教えて", 前)
        self.assertEqual(読解["要求"]["主題"], ["試作器乙"])
        self.assertEqual(読解["継承"], "照応")
        with self.assertRaisesRegex(ValueError, "一意"):
            self.読む("それについて説明して", 前)
        with self.assertRaisesRegex(ValueError, "照応"):
            self.読む("二つ目について教えて")

    def test_指示語の対象絞込で省略観点を解除しない(self):
        _, 前 = self.読む("容器甲と容器乙を費用で比較して")
        _, 訂正 = self.読む("費用ではなく保守性で比較して", 前)
        訂正前 = deepcopy(訂正)
        _, 読解 = self.読む("後者を詳しく説明して", 訂正)
        self.assertEqual(読解["要求"]["主題"], ["容器乙"])
        self.assertEqual(読解["要求"]["操作"], ["説明"])
        self.assertEqual(読解["要求"]["観点"], ["保守性"])
        self.assertEqual(読解["要求"]["詳細"], "詳細")
        self.assertEqual(訂正, 訂正前)
        _, 明示 = self.読む("後者の特徴を教えて", 訂正)
        self.assertEqual(明示["要求"]["観点"], [])
        self.assertEqual(明示["要求"]["関係焦点"], ["特徴"])
        _, 理由 = self.読む("なぜ試作器甲は停止しないのですか")
        _, 追問 = self.読む("それを詳しく説明して", 理由)
        self.assertEqual(追問["要求"]["関係焦点"], ["停止しない"])
        self.assertEqual(追問["要求"]["肯否"], "否定")

    def test_否定訂正は対象と観点を実変更し旧値を保持(self):
        _, 前 = self.読む("試作器甲と試作器乙を費用の観点で比較して")
        保存 = deepcopy(前)
        for 文, 欄, 期待 in (("費用ではなく保守性", "観点", ["保守性"]),
                              ("試作器甲ではなく試作器丙", "比較対象", ["試作器丙", "試作器乙"])):
            束, 読解 = self.読む(文, 前)
            self.assertEqual(読解["要求"][欄], 期待)
            self.assertEqual(読解["継承"], "訂正")
            self.assertTrue(読解["訂正関係"])
            self.assertTrue(any(関係.種別 == "目的条件置換" for 関係 in 束.意味IR.関係))
            self.assertEqual(前, 保存)
        with self.assertRaisesRegex(ValueError, "訂正前"):
            self.読む("寿命ではなく保守性", 前)
        for 文 in ("費用ではなく保守性、メールで送って", "費用ではなく保守性を必ず保証して"):
            with self.subTest(文=文), self.assertRaisesRegex(ValueError, "未消費"):
                self.読む(文, 前)

    def test_訂正候補の対象と観点が同語なら黙って両方変えない(self):
        _, 前 = self.読む("価格と試作器乙を価格の観点で比較して")
        with self.assertRaisesRegex(ValueError, "候補が複数"):
            self.読む("価格ではなく重量で比較して", 前)
        self.assertEqual(前["要求"]["観点"], ["価格"])
        self.assertEqual(前["要求"]["比較対象"], ["価格", "試作器乙"])

    def test_敬体と代わりの置換は同じ観点訂正へ(self):
        _, 前 = self.読む("試作器甲と試作器乙を費用で比較して")
        for 文 in ("費用ではなく保守性で比較してください", "費用の代わりに保守性で比較してください"):
            束, 読解 = self.読む(文, 前)
            self.assertEqual(読解["要求"]["観点"], ["保守性"])
            self.assertEqual(読解["要求"]["比較対象"], ["試作器甲", "試作器乙"])
            self.assertEqual(読解["継承"], "訂正")
            for 座標 in 束.意味IR.座標:
                if 座標.種別 in ("条件.会話訂正前", "条件.会話訂正後"):
                    self.assertEqual(文[slice(*座標.原文範囲)], 座標.内容)

    def test_新話題が前回の資料と観点を引き継がない(self):
        _, 前 = self.読む("次の文章を要約して:\n試作器甲は稼働する。")
        for 文 in ("別の話、未知材料乙とは何ですか？", "別の話です。未知材料乙とは何ですか？",
                   "別の話題ですが、未知材料乙とは何ですか？"):
            _, 読解 = self.読む(文, 前)
            self.assertEqual(読解["要求"]["主題"], ["未知材料乙"])
            self.assertEqual(読解["要求"]["提示本文"], "")
            self.assertIsNone(読解["継承"])

    def test_未知条件と競合を黙って消さない(self):
        for 文 in ("試作器甲とは何ですか、メールで送って", "試作器甲について説明して、例外を省いて",
                  "試作器甲について教えて、箇条書きにして、文章にして",
                  "試作器甲について小学生向けに説明して", "試作器甲を小学生向けに説明して",
                  "試作器甲の例外を省いて説明して", "試作器甲を安全性を保証して説明して"):
            with self.subTest(文=文), self.assertRaises(ValueError):
                self.読む(文)

    def test_未処理限定を主題や述部として消費済みにしない(self):
        for 文 in ("試作器甲の利点だけを教えて", "どうして試作器甲は条件に関係なく回転するのですか"):
            束 = 公開HDSコンパイラ().コンパイル束(文)
            残差 = [項 for 項 in 束.意味IR.残差 if 項.種別 == "会話未消費"]
            self.assertTrue(残差)
            self.assertTrue(any("限定" in 項.理由 for 項 in 残差))
            self.assertTrue(all(項.原文 in 文 for 項 in 残差))
            with self.assertRaises(ValueError):
                一般依頼を読む({"原文": 文}, 意味IR=束.意味IR)

    def test_算術と数量比較の既存責任へ一般射影を混在させない(self):
        for 文 in ("2+3", "2+3を説明して", "2+3の答えは", '「x^2」を微分して詳しく説明して',
                   '資料「A」と「B」の売上を比較して', '資料「A」の2020年と2021年の売上を比較して'):
            with self.subTest(文=文):
                束, 読解 = self.読む(文)
                self.assertIsNone(読解)
                self.assertIsNone(会話意味欄を読む(束.意味IR))
        for 文 in ("微分とは何ですか？", "積分とは何ですか？", "計算とは何ですか？"):
            _, 読解 = self.読む(文)
            self.assertEqual(読解["要求"]["主題"], [文.split("とは")[0]])

    def test_単一Kernelの原文と意味欄の対応を検査(self):
        束, _ = self.読む("試作器甲とは何ですか？")
        self.assertEqual(会話意味欄を読む(束.意味IR)["主題"], ["試作器甲"])
        with self.assertRaisesRegex(ValueError, "原文"):
            一般依頼を読む({"原文": "別の問い"}, 意味IR=束.意味IR)
        for 座標 in 束.意味IR.座標:
            if 座標.由来 == 会話射影由来 and 座標.原文範囲 and type(座標.内容) is str:
                if 座標.種別 != "目的.会話被覆":
                    self.assertEqual(束.意味IR.原文[slice(*座標.原文範囲)], 座標.内容)

    def test_公開取得は比較主題ごとに分け元要求を保持(self):
        _, 読解 = self.読む("試作器甲と試作器乙を保守性の観点で比較して")
        元 = deepcopy(読解["要求"])
        計画, 素材, _ = 一般計画を作る(読解, {"原文": 読解["原文"], "資料": {}, "外部許可": True})
        取得工程 = [工程 for 工程 in 計画.工程 if "知識取得" in 工程.能力候補]
        self.assertEqual(len(取得工程), 2)
        for 番号, 主題 in enumerate(("試作器甲", "試作器乙")):
            設定 = 素材["一般:取得設定:" + str(番号)].データ
            self.assertEqual(設定["必要語"], [主題])
            self.assertEqual(設定["検索語"], 主題 + " 保守性")
        self.assertEqual(素材["一般:要求"].データ, 元)
        self.assertEqual(読解["要求"], 元)

    def test_定義の要求を検索へ渡して主題と原要求を保持(self):
        for 名 in ("未観測素材甲", "未知機構乙", "おとぎ話", "試験概念丙"):
            _, 読解 = self.読む(名 + "とは何ですか？")
            元 = deepcopy(読解["要求"])
            _, 素材, _ = 一般計画を作る(読解, {"原文": 読解["原文"], "資料": {}, "外部許可": True})
            self.assertEqual(素材["一般:取得設定:0"].データ["検索語"], 名 + " 定義")
            self.assertEqual(素材["一般:取得設定:0"].データ["必要語"], [名])
            self.assertEqual(読解["要求"], 元)
            self.assertEqual(元["関係焦点"], ["定義"])
        _, 読解 = self.読む("なぜ未知機構乙が回転するのですか？")
        _, 素材, _ = 一般計画を作る(読解, {"原文": 読解["原文"], "資料": {}, "外部許可": True})
        self.assertEqual(素材["一般:取得設定:0"].データ["検索語"], "未知機構乙 回転する")

    def test_未許可の外部取得を計画しない(self):
        _, 読解 = self.読む("試作器甲とは何ですか？")
        with self.assertRaisesRegex(ValueError, "検索を許可"):
            一般計画を作る(読解, {"原文": 読解["原文"], "資料": {}, "外部許可": False})

    def test_提示本文を能力素材へ渡し原要求を改変しない(self):
        _, 読解 = self.読む("以下の文を要約して:\n試作器甲は稼働する。故障中は使用しない。")
        元 = deepcopy(読解["要求"])
        計画, 素材, _ = 一般計画を作る(読解, {"原文": 読解["原文"], "資料": {}, "外部許可": False})
        self.assertEqual(素材["一般:資料:0"].本文, 元["提示本文"])
        self.assertEqual(素材["一般:要求"].データ, {"要求": 元, "資料名": ["提示本文"]})
        self.assertFalse(any("知識取得" in 工程.能力候補 for 工程 in 計画.工程))
        self.assertEqual(読解["要求"], 元)

    def test_前回原資料は新目的の根拠としてのみ再利用(self):
        原文 = "以下の文章を要約して:\n提案甲の費用は30円です。提案乙の費用は50円です。"
        _, 前目的 = self.読む(原文)
        _, 素材, _ = 一般計画を作る(前目的, {"原文": 原文, "資料": {}, "外部許可": False})
        原資料 = 素材["一般:資料:0"]
        構造 = 資料意味を選ぶ({"提示本文": 原資料}, 前目的["要求"])
        前成果 = 結果を保存(資料文章を照合(構造, _文章(構造)))
        文 = "提案甲と提案乙を費用で比較して"
        入力 = {"原文": 文, "前回目的": 前目的, "前回結果": 前成果, "前回有効": True,
                "資料": {}, "外部許可": False}
        束 = 公開HDSコンパイラ().コンパイル束(文, 文脈=HDS文脈(現在焦点=前目的))
        読解 = 一般依頼を読む(入力, 意味IR=束.意味IR)
        self.assertIsNone(読解["継承"])
        self.assertTrue(読解["根拠継承"])
        self.assertEqual(読解["要求"]["提示本文"], "")
        self.assertEqual(読解["要求"]["操作"], ["対照"])
        計画, 素材, _ = 一般計画を作る(読解, 入力)
        self.assertEqual(結果を保存(素材["一般:資料:0"]), 結果を保存(原資料))
        self.assertEqual(素材["一般:束縛"].データ["実範囲"], "前回実資料")
        self.assertFalse(any("知識取得" in 工程.能力候補 for 工程 in 計画.工程))
        for 文, 有効 in (("提案丙とは何ですか？", True), ("別の話です。提案甲とは何ですか？", True),
                         ("提案甲とは何ですか？", False)):
            束 = 公開HDSコンパイラ().コンパイル束(文, 文脈=HDS文脈(現在焦点=前目的))
            読解 = 一般依頼を読む({**入力, "原文": 文, "前回有効": 有効}, 意味IR=束.意味IR)
            self.assertFalse(読解["根拠継承"])

    def test_v1の厳密な欄と動作を維持(self):
        文 = '資料「A」と「B」を寿命と費用の観点で比較し、確認事項を提案して'
        従来 = 一般依頼を読む({"原文": 文})
        _, Kernel経由 = self.読む(文)
        self.assertEqual(Kernel経由, 従来)
        self.assertEqual(set(従来["要求"]), {"版", "対象", "主題", "操作", "形式", "範囲", "最大文字数"})
        with self.assertRaises(ValueError):
            一般要求を検査({**従来["要求"], "問い": 文})

    def test_v2の型と上限を厳密に検査(self):
        _, 読解 = self.読む("試作器甲とは何ですか？")
        for 差分 in ({"最大文字数": True}, {"肯否": "確定"}, {"主題": [["不正型"]]},
                     {"詳細": "保証"}, {"提示本文": None}, {"勝手な欄": True}):
            with self.subTest(差分=差分), self.assertRaises(ValueError):
                一般要求を検査({**読解["要求"], **差分})

    def test_主文と後続句の詳細競合を確認待ちにする(self):
        for 文 in ("短く装置甲を詳しく説明して", "装置甲を詳しく説明して、短く",
                   "詳しく装置甲を短く説明して", "短く装置甲を説明して、詳細に"):
            with self.subTest(文=文):
                束 = 公開HDSコンパイラ().コンパイル束(文)
                残差 = [項 for 項 in 束.意味IR.残差 if 項.種別 == "会話未消費"]
                self.assertTrue(any("詳細指定が競合" in 項.理由 for 項 in 残差))
                with self.assertRaisesRegex(ValueError, "競合"):
                    一般依頼を読む({"原文": 文}, 意味IR=束.意味IR)
        for 文 in ("短く装置甲を簡潔に説明して", "装置甲を詳しく説明して、詳細に"):
            _, 読解 = self.読む(文)
            self.assertEqual(読解["要求"]["主題"], ["装置甲"])

    def test_資料名と比較対象を別欄へ保持(self):
        for 資料句, 名前 in (('資料「設計記録」', ["設計記録"]),
                              ('資料「記録甲」と「記録乙」', ["記録甲", "記録乙"])):
            with self.subTest(資料句=資料句):
                _, 読解 = self.読む(資料句 + "をもとに装置甲と装置乙を比較して")
                要求 = 読解["要求"]
                self.assertEqual(要求["対象"], 名前)
                self.assertEqual(要求["比較対象"], ["装置甲", "装置乙"])
                self.assertEqual(要求["主題"], ["装置甲", "装置乙"])
                self.assertEqual(要求["範囲"], "指定資料")

    def test_資料付き比較の後者は対象を絞り資料群を保持(self):
        _, 前 = self.読む('資料「記録甲」と「記録乙」をもとに装置甲と装置乙を保守性で比較して')
        保存 = deepcopy(前)
        _, 読解 = self.読む("後者を詳しく説明して", 前)
        self.assertEqual(読解["要求"]["主題"], ["装置乙"])
        self.assertEqual(読解["要求"]["対象"], ["記録甲", "記録乙"])
        self.assertEqual(読解["要求"]["比較対象"], [])
        self.assertEqual(読解["要求"]["観点"], ["保守性"])
        self.assertEqual(前, 保存)

    def test_資料と別の比較対象が内容構成まで届く(self):
        from minidora.製品版.型 import 能力結果
        for 資料句, 資料群 in (
            ('資料「設計記録」', {"設計記録": 能力結果(True, "装置甲の長さは4m。装置乙の長さは9m。")}),
            ('資料「記録甲」と「記録乙」', {"記録甲": 能力結果(True, "装置甲の長さは4m。"),
                                        "記録乙": 能力結果(True, "装置乙の長さは9m。")}),
        ):
            with self.subTest(資料句=資料句):
                _, 読解 = self.読む(資料句 + "をもとに装置甲と装置乙を長さで比較して")
                構造 = 資料意味を選ぶ(資料群, 読解["要求"])
                成果 = 資料文章を照合(構造, _文章(構造))
                self.assertIn("装置甲", 成果.本文)
                self.assertIn("装置乙", 成果.本文)
                self.assertIn("5", 成果.本文)
                self.assertEqual(成果.データ["構造"]["要求"], 読解["要求"])

    def test_改行だけで後続制約を資料へ移さない(self):
        for 文 in ("装置甲を説明して\nただし例外は省いて",
                   '「以下の装置」を説明して\nただし例外は省いて'):
            with self.subTest(文=文):
                束 = 公開HDSコンパイラ().コンパイル束(文)
                意味 = 会話意味欄を読む(束.意味IR)
                self.assertNotIn("提示本文", 意味)
                残差 = [項 for 項 in 束.意味IR.残差 if 項.種別 == "会話未消費"]
                self.assertTrue(any("ただし例外は省いて" in 項.原文 for 項 in 残差))
                with self.assertRaises(ValueError):
                    一般依頼を読む({"原文": 文}, 意味IR=束.意味IR)
        _, 読解 = self.読む("装置甲を説明して\n短く")
        self.assertEqual(読解["要求"]["主題"], ["装置甲"])
        self.assertEqual(読解["要求"]["詳細"], "簡潔")

    def test_明示区切りと提示句の本文は非実行データを保持(self):
        本文 = "装置甲は回転する。計算して、微分しては資料の引用である。"
        for 制御 in ("次の文章を説明して", "この文章の要点を教えて:", "要約して:"):
            with self.subTest(制御=制御):
                文 = 制御 + "\n" + 本文
                束, 読解 = self.読む(文)
                self.assertEqual(読解["要求"]["提示本文"], 本文)
                self.assertFalse(any(項.種別 == "計算" for 項 in 束.コア入力.作用要求))
                座標 = next(項 for 項 in 束.意味IR.座標 if 項.種別 == "情報源.会話提示本文")
                self.assertEqual(文[slice(*座標.原文範囲)], 本文)

    def test_未知の副詞様式を主題名へ吸収しない(self):
        for 文 in ("装置甲をやさしく説明して", "装置甲を手早く説明して", "装置甲を平易な文体で説明して"):
            with self.subTest(文=文):
                束 = 公開HDSコンパイラ().コンパイル束(文)
                残差 = [項 for 項 in 束.意味IR.残差 if 項.種別 == "会話未消費"]
                self.assertTrue(any("表示又は除外条件" in 項.理由 for 項 in 残差))
                self.assertEqual(会話意味欄を読む(束.意味IR)["主題"], ["装置甲"])
                with self.assertRaises(ValueError):
                    一般依頼を読む({"原文": 文}, 意味IR=束.意味IR)
        for 名 in ("やさしく", "手早く", "夢をかなえる装置"):
            _, 読解 = self.読む("「" + 名 + "」を説明して")
            self.assertEqual(読解["要求"]["主題"], [名])

    def test_合成操作の原文範囲を引用対象や本文へ誤束縛しない(self):
        for 文 in ('「説明装置」とは何ですか', '説明装置とは何ですか',
                   '説明してくれる装置とは何ですか',
                   'この文章の要点を教えて:\n説明と要約は文章の中の語です。'):
            with self.subTest(文=文):
                束, _ = self.読む(文)
                操作 = [項 for 項 in 束.意味IR.座標
                        if 項.種別 == "目的.会話操作" and 項.由来 == 会話射影由来]
                self.assertTrue(操作)
                self.assertTrue(all(項.原文範囲 is None for 項 in 操作))
                形式 = next(項 for 項 in 束.意味IR.座標
                            if 項.種別 == "表現.会話形式" and 項.由来 == 会話射影由来)
                self.assertIsNone(形式.原文範囲)
        文 = '「説明装置」を説明して'
        束, _ = self.読む(文)
        操作 = next(項 for 項 in 束.意味IR.座標
                    if 項.種別 == "目的.会話操作" and 項.由来 == 会話射影由来)
        self.assertEqual(操作.原文範囲, (文.rindex("説明"), 文.rindex("説明") + 2))

    def test_引用した詳細語を対象から除去しない(self):
        for 名 in ("短く動く装置", "詳しく調べる装置"):
            _, 読解 = self.読む("「" + 名 + "」を短く説明して")
            self.assertEqual(読解["要求"]["主題"], [名])
            self.assertEqual(読解["要求"]["詳細"], "簡潔")


if __name__ == "__main__":
    unittest.main()

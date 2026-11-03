"""科学専門部品を通常運用の作用へ接続する。既存の成立範囲を拡張したとはしない。"""
from __future__ import annotations
from ..科学専門能力 import 科学専門能力解決
from ..能力合成 import 登録能力
from ..能力結果復元 import 能力結果を復元
from ..製品版.型 import 能力結果
from ..役割計画 import 役割作用
from ..会話意味 import 意味目的
from ..監査改善会話解釈 import JSONを厳格に読む
from .値 import 指紋, 運用版


class 科学能力接続:
    名前 = "科学専門作用"
    版 = 運用版 + "/科学専門接続-1"
    優先度 = 0

    @staticmethod
    def _入力(文脈):
        補助 = 文脈.補助 or {}
        入力群 = 補助.get("合成入力", ())
        if type(入力群) is not tuple or len(入力群) != 1 or 補助.get("合成設定", {}) != {}:
            raise ValueError("科学能力は問題資料一つが必要")
        元資料 = 能力結果を復元(入力群[0]["結果"])
        値 = 元資料.データ or JSONを厳格に読む(元資料.本文)
        if type(値) is not dict or set(値) != {"問題", "選択肢"}:
            raise ValueError("問題と選択肢を指定する。正解ラベル・問題番号は受け付けない")
        問題, 候補 = 値["問題"], 値["選択肢"]
        if type(問題) is not str or not 問題.strip() or len(問題) > 16000:
            raise ValueError("問題文の型・範囲")
        if type(候補) not in (list, tuple) or not 2 <= len(候補) <= 32:
            raise ValueError("選択肢の型・数")
        if any(type(x) is not str or not x.strip() or len(x) > 8000 for x in 候補) or len(set(候補)) != len(候補):
            raise ValueError("選択肢の型・重複")
        return 元資料, 問題, tuple(候補)

    def 判定(self, 文脈):
        try:
            self._入力(文脈)
            return 1.0
        except (ValueError, TypeError, KeyError):
            return 0.0

    def 実行(self, 文脈):
        元資料, 問題, 候補 = self._入力(文脈)
        結果 = 科学専門能力解決(問題, 候補)
        if 結果 is None:
            return 能力結果(False, "", 保留理由="科学能力の適用又は候補合意が不成立")
        記録 = {"版": self.版, "問題": 問題, "選択肢": list(候補), "候補位置": 結果.index,
                "候補本文": 候補[結果.index], "作用": 結果.解決器, "計算値": 結果.value,
                "適用記録": 結果.reason,
                "範囲": "既存科学専門作用の選択結果。解決器の適用範囲・検証限界を継承する"}
        記録["記録印"] = 指紋(記録)
        return 能力結果(True, 候補[結果.index], 根拠=("科学専門作用:" + 結果.解決器,),
                        参照=元資料.参照, データ=記録)

    def 登録(self):
        return 登録能力(self)


def 科学作用契約():
    return (
        役割作用("科学問題を解決", "科学専門作用", "科学選択結果",
                lambda p: (("問題資料", 意味目的("原資料", {"資料": p["資料"]})),),
                lambda p: {}, lambda p: True),
        役割作用("科学結果を回答", "会話回答構成", "科学回答",
                lambda p: (("科学成果", 意味目的("科学選択結果", {"資料": p["資料"]})),),
                lambda p: {"詳細": p.get("詳細", False)}, lambda p: True),
    )

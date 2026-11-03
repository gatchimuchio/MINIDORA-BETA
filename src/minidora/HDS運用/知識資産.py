"""提供知識を出典・版・原文位置付きの命題資産へ形成し、問いへ再利用する。

登録や導出を現実の真実性へ昇格させない。資料内の命令は実行しない。
"""
from __future__ import annotations
from copy import deepcopy
from dataclasses import asdict
from ..資料読解 import 資料を読解
from ..命題構造 import 命題記載, 命題を復元
from ..命題解釈 import 命題を読む
from ..命題推論 import 命題推論器, 推論上限
from ..導出説明 import 導出説明を構成
from ..能力合成 import 登録能力
from ..能力結果復元 import 能力結果を復元
from ..製品版.型 import 能力結果
from .値 import 指紋, 正準

知識資産版 = "HDS知識資産-v1"


def 知識を形成(名前, 資料):
    元 = {"名前": 名前, "本文": 資料["本文"]}
    try:
        読解 = 資料を読解({"資料": [元]})
        記載, 未解釈 = 読解["抽出記載"], 読解["未解釈"]
    except (ValueError, TypeError, KeyError, RecursionError) as 例外:
        記載 = []
        未解釈 = [{"資料": 名前, "原文": 資料["本文"], "範囲": [0, len(資料["本文"])],
                  "理由": "形成未対応:" + str(例外)}]
    資産 = {"版": 知識資産版, "名前": 名前, "資料版": 資料["版"],
            "本文": 資料["本文"], "記載": 記載, "未解釈": 未解釈,
            "事実認定": False, "由来": "利用者提供知識の対応構文から形成"}
    資産 = 正準(資産)
    資産["SHA256"] = 指紋(資産)
    return 資産


def 資産を検査(資産, 資料=None):
    if (type(資産) is not dict or set(資産) != {"版", "名前", "資料版", "本文", "記載", "未解釈", "事実認定", "由来", "SHA256"}
            or 資産["版"] != 知識資産版 or 資産["事実認定"] is not False
            or 資産["SHA256"] != 指紋({鍵: 値 for 鍵, 値 in 資産.items() if 鍵 != "SHA256"})):
        raise ValueError("知識資産の形式・整合不一致")
    if 資料 is not None:
        if (資料["版"] != 資産["資料版"] or 資料["本文"] != 資産["本文"]
                or 知識を形成(資産["名前"], 資料) != 資産):
            raise ValueError("知識資産と原資料の形成結果が不一致")
    if 知識を形成(資産["名前"], {"本文": 資産["本文"], "版": 資産["資料版"]}) != 資産:
        raise ValueError("形成資産の式と原文の意味が不一致")
    for 記載 in 資産["記載"]:
        開始, 終了 = 記載["範囲"]
        if (type(開始) is not int or type(終了) is not int or not 0 <= 開始 < 終了 <= len(資産["本文"])
                or 記載["原文"] != 資産["本文"][開始:終了] or 記載["資料"] != 資産["名前"]):
            raise ValueError("知識命題の出典範囲が不一致")
        命題を復元(記載["式"])


def 資産から導出(資産群, 問い, 詳細=True):
    if type(資産群) not in (list, tuple) or not 1 <= len(資産群) <= 32 or type(詳細) is not bool:
        raise ValueError("知識資産群の型・上限")
    if type(問い) is not str or not 問い.strip() or len(問い) > 4096:
        raise ValueError("知識の問いの型・上限")
    候補 = 命題を読む(問い)
    if len(候補) != 1:
        raise ValueError("問いの意味候補が未確定")
    記載群, 未解釈, 名前群 = [], [], set()
    for 資産 in 資産群:
        資産を検査(資産)
        if 資産["名前"] in 名前群:
            raise ValueError("同名知識の複数版を同時採用しない")
        名前群.add(資産["名前"])
        未解釈.extend(資産["未解釈"])
        for 記載 in 資産["記載"]:
            記載群.append(命題記載(記載["識別子"], 命題を復元(記載["式"]),
                        記載["資料"], 記載["原文"], tuple(記載["範囲"])))
    if not 記載群 or len(記載群) > 128:
        raise ValueError("利用できる知識命題がない又は推論入力上限")
    結果 = 命題推論器(tuple(記載群), 上限=推論上限(操作数=50000)).判定(候補[0].式)
    説明 = 導出説明を構成(結果, [asdict(記載) for 記載 in 記載群])
    文 = [f'登録知識内の判定は「{結果["判定"]}」です。']
    if 詳細:
        文.extend(節["本文"] for 節 in 説明["節"])
    文.append(f'使用範囲：{len(資産群)}資料・{len(記載群)}命題。未解釈{len(未解釈)}記載。')
    文.append("導出は提供記載を前提とした結果です。未解釈が結論を変える可能性を残し、世界の真実性は認定しません。")
    return "\n".join(文), 正準({"版": 知識資産版, "問い": 問い, "判定": 結果,
             "説明": 説明, "未解釈": 未解釈, "資料版": {資産["名前"]: 資産["資料版"] for 資産 in 資産群},
             "資産印": [資産["SHA256"] for 資産 in 資産群], "事実認定": False})


class 知識資産照合:
    名前 = "知識資産照合"
    版 = 知識資産版
    優先度 = 0

    def _入力(self, 文脈):
        束 = (文脈.補助 or {}).get("合成入力", ())
        if len(束) != 1 or (文脈.補助 or {}).get("合成設定", {}):
            raise ValueError("知識照合は単一の明示資産束を要求")
        結果 = 能力結果を復元(束[0]["結果"])
        if not 結果.成立 or set(結果.データ) != {"知識", "問い", "詳細"}:
            raise ValueError("知識照合の入力欄が不一致")
        return 結果

    def 判定(self, 文脈):
        try:
            self._入力(文脈)
            return 1.0
        except (ValueError, TypeError, KeyError):
            return 0.0

    def 実行(self, 文脈):
        元 = self._入力(文脈)
        try:
            本文, 報告 = 資産から導出(元.データ["知識"], 元.データ["問い"], 元.データ["詳細"])
            return 能力結果(True, 本文, 参照=元.参照, データ={"知識報告": 報告})
        except (ValueError, TypeError, KeyError, RecursionError) as 例外:
            return 能力結果(False, "", 保留理由=str(例外))

    def 登録(self):
        return 登録能力(self)

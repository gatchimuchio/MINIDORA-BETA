"""成功した純粋工程を同じHDS循環で再実行し、条件付きの手順資産にする。

記録するのは命令・設定・入力スロットであり回答値ではない。汎用法則獲得ではない。
外部作用、未契約の追加能力、失敗枝、再表現は形成しない。
"""
from __future__ import annotations
from copy import deepcopy
import json
import math
from ..HDS実行主体 import HDS関数作用, HDS作用結果, HDS作用状態
from .値 import 指紋, 正準, 結果を保存, 結果を復元, 計画を復元
from .工程 import 現行計画, 工程供給, _工程鍵

手順形成版 = "HDS条件付き手順-v1"


def 目的鍵(解釈):
    値 = deepcopy(解釈["目的"] if 解釈["方式"] == "継続" else 解釈)
    値.pop("HDS", None)
    return 指紋(値)


def 素材構造(資料):
    def 形(値):
        if type(値) is dict:
            return {鍵: 形(項) for 鍵, 項 in sorted(値.items())}
        if type(値) is list:
            return [形(項) for 項 in 値]
        return type(値).__name__
    本文 = 資料["本文"]
    try:
        中身 = json.loads(本文)
        構造 = 形(中身)
    except (ValueError, TypeError):
        構造 = "本文"
    return {"種類": 資料["種類"], "本文構造": 構造, "データ構造": 形(資料["データ"])}


def 候補を構成(計画鍵, 包, 入力, 目録):
    if 包["方式"] != "能力計画" or 計画鍵 != "運用計画:0" or 包["外部許可"] or 包.get("手順由来"):
        return None
    if 包["回答種別"] == "再表現" or 包.get("解釈", {}).get("方式") in ("数量再表現", "一般再表現", "関係再表現"):
        return None
    計画 = 計画を復元(包["計画"])
    if not 計画.工程 or len(計画.工程) > 16:
        return None
    if any(len(工程.能力候補) != 1 or 工程.能力候補[0] not in 目録._純粋
           or 目録.取得(工程.能力候補[0]).外部読取 for 工程 in 計画.工程):
        return None
    固定, 束縛 = {}, {}
    for 鍵, 素材 in 包["資料"].items():
        対応 = [名前 for 名前, 元 in 入力["資料"].items() if 素材 == 元["結果"]]
        if len(対応) == 1:
            名前 = 対応[0]
            束縛[鍵] = {"資料": 名前, "構造": 素材構造(入力["資料"][名前])}
        elif 結果を復元(素材).参照:
            # 元資料から派生した、可変の合成素材を固定してしまわない。
            return None
        else:
            固定[鍵] = deepcopy(素材)
    雛型 = {鍵: deepcopy(値) for 鍵, 値 in 包.items() if 鍵 not in ("資料", "手順由来")}
    候補 = {"版": 手順形成版, "目的鍵": 目的鍵(包["解釈"]), "目録": 目録.ハッシュ,
            "雛型": 雛型, "固定素材": 固定, "資料束縛": 束縛,
            "検証": "未再現", "範囲": "同じ目的・素材構造・能力契約の純粋手順。値は再計算する"}
    return 正準(候補)


def 手順資産を検査(記録, 目録):
    必須 = {"版", "目的鍵", "目録", "雛型", "固定素材", "資料束縛", "検証", "範囲", "再現印", "SHA256"}
    if (type(記録) is not dict or set(記録) != 必須 or 記録["版"] != 手順形成版
            or 記録["検証"] != "再実行一致" or 記録["目録"] != 目録.ハッシュ
            or 記録["SHA256"] != 指紋({鍵: 値 for 鍵, 値 in 記録.items() if 鍵 != "SHA256"})):
        raise ValueError("手順資産の形式・契約不一致")
    for 鍵 in ("目的鍵", "再現印", "SHA256"):
        印 = 記録[鍵]
        if type(印) is not str or len(印) != 64 or any(字 not in "0123456789abcdef" for 字 in 印):
            raise ValueError("手順資産の署名形式不正")
    if any(type(記録[鍵]) is not dict for 鍵 in ("雛型", "固定素材", "資料束縛")):
        raise ValueError("手順資産の素材・雛型が辞書でない")
    包 = 記録["雛型"]
    必須雛型 = {"方式", "計画", "被覆", "回答種別", "外部許可", "目録", "役割", "禁止", "原文", "解釈"}
    if (set(包) != 必須雛型 or 包["方式"] != "能力計画" or 包["外部許可"] is not False
            or 包["目録"] != 目録.ハッシュ or 包["回答種別"] == "再表現"
            or 目的鍵(包["解釈"]) != 記録["目的鍵"]):
        raise ValueError("手順資産の意味・外部作用境界不一致")
    計画 = 計画を復元(包["計画"])
    if not 1 <= len(計画.工程) <= 16 or any(
            len(工程.能力候補) != 1 or 工程.能力候補[0] not in 目録._純粋
            or 目録.取得(工程.能力候補[0]).外部読取 for 工程 in 計画.工程):
        raise ValueError("手順資産に未契約又は外部作用がある")
    if set(記録["固定素材"]) & set(記録["資料束縛"]):
        raise ValueError("固定素材と可変資料の住所が重複")
    for 値 in 記録["固定素材"].values():
        if 結果を復元(値).参照:
            raise ValueError("参照付き派生成果を固定素材にしない")
    for 束縛 in 記録["資料束縛"].values():
        if type(束縛) is not dict or set(束縛) != {"資料", "構造"} or type(束縛["資料"]) is not str:
            raise ValueError("手順の資料束縛不正")
        if type(束縛["構造"]) is not dict or set(束縛["構造"]) != {"種類", "本文構造", "データ構造"}:
            raise ValueError("手順の資料構造不正")


def 手順を束縛(記録, 解釈, 入力, 目録):
    if (type(記録) is not dict or 記録.get("版") != 手順形成版
            or 記録.get("検証") != "再実行一致" or 記録.get("目録") != 目録.ハッシュ
            or 記録.get("目的鍵") != 目的鍵(解釈)):
        return None
    手順資産を検査(記録, 目録)
    本体 = {鍵: 値 for 鍵, 値 in 記録.items() if 鍵 != "SHA256"}
    if 記録.get("SHA256") != 指紋(本体):
        raise ValueError("手順資産の整合不一致")
    素材 = deepcopy(記録["固定素材"])
    for 鍵, 束縛 in 記録["資料束縛"].items():
        元 = 入力["資料"].get(束縛["資料"])
        if 元 is None or 素材構造(元) != 束縛["構造"]:
            return None
        素材[鍵] = deepcopy(元["結果"])
    包 = deepcopy(記録["雛型"])
    包.update(原文=入力["原文"], 解釈=deepcopy(解釈), 資料=素材)
    目録.構造検査器._準備(計画を復元(包["計画"]),
                           {鍵: 結果を復元(値) for 鍵, 値 in 素材.items()}, False)
    return 正準(包)


class 手順形成供給:
    def __init__(self, 目録, *, 有効=True, 最大作用回数=128):
        self.目録, self.有効, self.上限 = 目録, 有効, 最大作用回数
        self.橋 = 工程供給(目録)

    def __call__(self, 状態):
        if "運用:応答成立" not in 状態.成立状態 or "運用:形成処理済" in 状態.成立状態:
            return ()
        鍵, 包 = 現行計画(状態)
        値 = dict(状態.成果)
        候補 = 候補を構成(鍵, 包, 値["運用入力"], self.目録) if self.有効 else None
        計画 = 計画を復元(包["計画"]) if 候補 else None
        # 初回・再現・記憶・検証が入る保守的な予算を先に確保する。
        if 計画 is not None and 2 * len(計画.工程) + 12 > self.上限:
            候補, 計画 = None, None
        def 終了処理(s):
            現在 = dict(s.成果)
            記録 = None
            理由 = "形成済手順を現行要求へ照合して再利用" if 包.get("手順由来") else "形成対象外又は再現予算不足"
            if 候補 is not None:
                再現 = [現在.get("運用形成:" + 工程.識別子) for 工程 in 計画.工程]
                if 再現 and all(行 and 行["一致"] for 行 in 再現):
                    記録 = deepcopy(候補)
                    記録["検証"] = "再実行一致"
                    記録["再現印"] = 指紋(再現)
                    記録["SHA256"] = 指紋(記録)
                    理由 = "純粋工程の実再現で一致を確認"
                else:
                    理由 = "純粋手順の再現不一致。候補を再利用へ昇格しない"
            return HDS作用結果(HDS作用状態.成立, 追加状態=frozenset({"運用:形成処理済"}),
                    成果=(("運用形成結果", {"手順": 記録, "理由": 理由}),), 理由=(理由,))
        if 候補 is None:
            return (HDS関数作用("運用/形成対象判定", 終了処理, 入力状態=("運用:応答成立",),
                    出力状態=("運用:形成処理済",), 読取成果=(鍵,), 契約版=手順形成版),)
        工程群 = []
        for 工程 in 計画.工程:
            出力鍵 = "運用形成:" + 工程.識別子
            親 = tuple("運用形成:" + 参照.識別子 for 参照 in 工程.入力 if 参照.領域 == "工程")
            def 再現する(s, 工程=工程, 出力鍵=出力鍵, 親=親):
                現在 = dict(s.成果)
                記録 = {"一致": False, "結果": None, "能力": 工程.能力候補[0]}
                if all(現在[親鍵]["一致"] for 親鍵 in 親):
                    # 再現後続は元の結果ではなく、再現した上流の成果を受け取る。
                    for 参照 in 工程.入力:
                        if 参照.領域 == "工程":
                            現在[_工程鍵(鍵, 参照.識別子)] = 現在["運用形成:" + 参照.識別子]["結果"]
                    from dataclasses import replace
                    写像 = replace(s, 成果=tuple(sorted(現在.items())))
                    try:
                        文脈 = self.橋._文脈(写像, 鍵, 包, 工程)
                        部品 = self.目録.取得(工程.能力候補[0]).モジュール
                        適用 = 部品.判定(deepcopy(文脈))
                        if type(適用) not in (int, float) or not math.isfinite(適用) or not 0 < 適用 <= 1:
                            raise ValueError("再現の適用条件不成立")
                        結果 = 結果を保存(部品.実行(deepcopy(文脈)))
                        self.目録.照合()
                        一致 = 結果 == 現在[_工程鍵(鍵, 工程.識別子)]
                        記録.update(一致=一致, 結果=結果 if 一致 else None)
                    except Exception as 例外:
                        記録["理由"] = type(例外).__name__ + ":" + str(例外)
                return HDS作用結果(HDS作用状態.成立,
                        追加状態=frozenset({出力鍵}), 成果=((出力鍵, 記録),),
                        理由=("純粋工程をキャッシュなしで再実行", "一致" if 記録["一致"] else "形成を不採用"))
            工程群.append(HDS関数作用("運用/手順再現/" + 工程.識別子, 再現する,
                     入力状態=("運用:応答成立", *親), 出力状態=(出力鍵,),
                     読取成果=(鍵, _工程鍵(鍵, 工程.識別子), *親), 契約版=手順形成版))
        全出力 = tuple("運用形成:" + 工程.識別子 for 工程 in 計画.工程)
        工程群.append(HDS関数作用("運用/手順形成確定", 終了処理,
                       入力状態=全出力, 出力状態=("運用:形成処理済",),
                       読取成果=(鍵, *全出力), 契約版=手順形成版))
        return tuple(工程群)


def 形成結果を検査(状態, 目録):
    try:
        値 = dict(状態.成果)
        記録 = 値["運用形成結果"]["手順"]
        if 記録 is None:
            return True
        鍵, 包 = 現行計画(状態)
        候補 = 候補を構成(鍵, 包, 値["運用入力"], 目録)
        if 候補 is None:
            return False
        計画 = 計画を復元(包["計画"])
        再現 = [値["運用形成:" + 工程.識別子] for 工程 in 計画.工程]
        for 工程, 行 in zip(計画.工程, 再現):
            if (行["一致"] is not True or 行["能力"] != 工程.能力候補[0]
                    or 行["結果"] != 値[_工程鍵(鍵, 工程.識別子)]):
                return False
        候補["検証"] = "再実行一致"
        候補["再現印"] = 指紋(再現)
        候補["SHA256"] = 指紋(候補)
        return 候補 == 記録
    except (ValueError, TypeError, KeyError):
        return False

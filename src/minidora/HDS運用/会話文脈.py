"""実入力の会話証明を用途別に供与する。意味と採否はHDSが判定する。"""
from __future__ import annotations
from copy import deepcopy
from .値 import 正準

会話文脈欄 = ("前採用包", "文脈証明", "前命令包", "命令証明")


def 会話保存文脈を読む(値):
    """宣言を読み、実庫から再発行した値との照合は呼出側へ残す。"""
    if type(値) is not dict:
        raise ValueError("会話文脈の宣言型が不正です")
    新欄 = {"前命令包", "命令証明"} & set(値)
    if 新欄 and 新欄 != {"前命令包", "命令証明"}:
        raise ValueError("会話命令文脈の宣言が欠落しています")
    文脈 = {"前採用包": deepcopy(値.get("前採用包")),
          "文脈証明": 正準(値.get("文脈証明", [])),
          "前命令包": deepcopy(値.get("前命令包")),
          "命令証明": 正準(値.get("命令証明", []))}
    for 欄 in ("前採用包", "前命令包"):
        if 文脈[欄] is not None and type(文脈[欄]) is not dict:
            raise ValueError("会話源包の型が不正です:" + 欄)
    if 文脈["前採用包"] is not None and 文脈["前命令包"] is not None:
        raise ValueError("現行成果と歴史命令を同じ源として採用しません")
    for 欄 in ("文脈証明", "命令証明"):
        if type(文脈[欄]) is not list or any(type(項) is not dict for 項 in 文脈[欄]):
            raise ValueError("会話証明の列型が不正です:" + 欄)
    if len(文脈["文脈証明"]) + len(文脈["命令証明"]) > 4:
        raise ValueError("会話証明の全体上限4を超えています")
    return 文脈


def 会話入力文脈を読む(包, 入力):
    """当前HDS包の用途から、独立に取得済みの実入力証明を選ぶ。"""
    束縛 = 包.get("文脈束縛")
    if type(束縛) is not list or len(束縛) > 1:
        raise ValueError("会話の元源包が一意ではありません")
    if not 束縛:
        return 会話保存文脈を読む({})
    元 = 束縛[0]
    if type(元) is not dict:
        raise ValueError("会話の元源宣言が不正です")
    命令 = 元.get("用途") == "命令再実行"
    if "用途" in 元 and not 命令:
        raise ValueError("会話証明の用途が未対応です")
    候補 = 入力.get("前命令候補" if 命令 else "前採用会話")
    許容 = ({"契約包", "文脈証明", "命令証明"},) if 命令 else (
        {"契約包", "文脈証明"}, {"契約包", "文脈証明", "命令証明"})
    if type(候補) is not dict or set(候補) not in 許容:
        raise ValueError("会話用途に対応する実入力証明がありません")
    if not 命令 and not 入力.get("前回有効"):
        raise ValueError("普通の照応に使う前成果が失効しています")
    源 = 候補["契約包"]
    if (type(源) is not dict or 源.get("所有セッション") != 入力["セッションID"]
            or 元.get("前束印") != 源.get("意味束印")):
        raise ValueError("会話用途と実入力源包の所有・意味束印が不一致です")
    return 会話保存文脈を読む({
        "前採用包": None if 命令 else 源,
        "文脈証明": 候補["文脈証明"],
        "前命令包": 源 if 命令 else None,
        "命令証明": 候補.get("命令証明", [])})


def 会話形成証明を読む(意味IR, 入力):
    """今回IRの明示命令anchorに対応する証明poolだけを形成へ渡す。"""
    命令 = any(辺.種別 == "命令再実行" for 辺 in 意味IR.関係)
    候補 = 入力.get("前命令候補" if 命令 else "前採用会話")
    if 候補 is None:
        return {"文脈証明": [], "命令証明": []}
    許容 = ({"契約包", "文脈証明", "命令証明"},) if 命令 else (
        {"契約包", "文脈証明"}, {"契約包", "文脈証明", "命令証明"})
    if type(候補) is not dict or set(候補) not in 許容:
        raise ValueError("会話形成の実入力証明欄が不一致です")
    文脈 = 会話保存文脈を読む({"文脈証明": 候補["文脈証明"],
                      "命令証明": 候補.get("命令証明", []), "前命令包": None})
    return {"文脈証明": 文脈["文脈証明"], "命令証明": 文脈["命令証明"]}


def 会話設定を作る(包, 文脈):
    文脈 = 会話保存文脈を読む(文脈)
    if 文脈["前命令包"] is not None or 文脈["命令証明"]:
        return {"会話契約": deepcopy(包), **文脈}
    return {"会話契約": deepcopy(包), "前採用包": 文脈["前採用包"],
            "文脈証明": 文脈["文脈証明"]}


def 会話設定文脈を読む(設定):
    if type(設定) is not dict or set(設定) not in (
            {"会話契約", "前採用包", "文脈証明"},
            {"会話契約", *会話文脈欄}):
        raise ValueError("会話目的束の設定欄が不一致です")
    return 会話保存文脈を読む(設定)


def 命令提示材料を読む(包, 元目的ID, 入力):
    """実庫発行の提示原文を読む。歴史の生成回答を材料へ昇格しない。"""
    from hashlib import sha256
    from ..製品版.型 import 能力結果, 参照資料
    文脈 = 会話入力文脈を読む(包, 入力)
    源 = 文脈["前命令包"]
    if 源 is None or 元目的ID not in 源["目的参照"]:
        raise ValueError("提示命令の実源目的が不一致です")
    群 = 入力.get("命令提示原文", {})
    if type(群) is not dict or not set(群) <= set(源["目的参照"]):
        raise ValueError("提示命令の原文対応が不正です")
    原 = 群.get(元目的ID)
    if 原 is None:
        return None
    欄 = {"所有セッション", "前束印", "元目的ID", "原記録ID", "材料原記録ID",
         "材料原文印", "原要求", "原文範囲"}
    if type(原) is not dict or set(原) != 欄:
        raise ValueError("提示命令原文の宣言欄が不一致です")
    自己 = [項 for 項 in 文脈["命令証明"] if 項["前束印"] == 源["意味束印"]]
    範囲, 原文 = 原["原文範囲"], 原["原要求"]
    if (len(自己) != 1 or 原["所有セッション"] != 入力["セッションID"]
            or 原["前束印"] != 源["意味束印"] or 原["元目的ID"] != 元目的ID
            or 原["原記録ID"] != 自己[0]["原記録ID"]
            or type(原["材料原記録ID"]) is not str or not 原["材料原記録ID"]
            or type(原文) is not str or len(原文) > 1000000
            or sha256(原文.encode("utf-8")).hexdigest() != 原["材料原文印"]
            or type(範囲) is not list or len(範囲) != 2
            or any(type(項) is not int for 項 in 範囲)
            or not 0 <= 範囲[0] < 範囲[1] <= len(原文)):
        raise ValueError("提示命令原文と実庫発行対応が不一致です")
    本文 = 原文[範囲[0]:範囲[1]]
    if len(本文) > 24000:
        raise ValueError("提示命令の原材料が通常文字予算を超えています")
    参照 = 参照資料("命令提示本文:" + 原["材料原文印"] + ":" + str(範囲[0]) + ":" + str(範囲[1]),
                    "提示本文", "利用者提示本文", 本文=本文)
    return 能力結果(True, 本文, 参照=(参照,))

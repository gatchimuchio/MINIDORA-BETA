"""原資料の関係・実導出・未確認前提を保持する。資料間の同名を自動同一化しない。"""
from __future__ import annotations
from dataclasses import asdict
from ..命題句 import 構成句を分ける, 最上位位置
from ..命題構造 import 命題記載, 命題を復元, 原子群, 結合, 反対
from ..知識取得 import 知識取得要求
from ..命題推論 import 命題推論器, 推論上限
from ..導出説明 import 導出説明を構成
from ..製品版.型 import 能力結果
from .値 import 正準, 指紋, 結果を保存, 結果を復元
from .関係言語 import 関係文を読む, 例外節を読む, 材料局所役割を読む
from .一般接続 import 取得資料を復元

関係要求版 = "HDS関係説明要求-v2"
関係読解版 = "HDS関係読解-v2"
主張採用要求版 = "HDS主張採用検討要求-v1"
主張採用読解版 = "HDS主張採用読解-v2"
_形式 = ("箇条書き", "文章", "引用")


def 関係要求を検査(要求):
    if type(要求) is dict and 要求.get("版") == "HDS資料条件変更要求-v2":
        from .資料条件変更 import 資料条件変更要求を検査
        return 資料条件変更要求を検査(要求)
    if type(要求) is dict and 要求.get("版") == 主張採用要求版:
        return 主張採用要求を検査(要求)
    if type(要求) is dict and 要求.get("版") == "HDS資料横断要求-v1":
        from .資料横断 import 横断要求を検査
        return 横断要求を検査(要求)
    if (type(要求) is not dict or set(要求) != {"版", "対象", "範囲", "問い", "形式", "不足調査", "最大文字数"}
            or 要求["版"] != 関係要求版):
        raise ValueError("関係要求の型・欄・版不一致")
    群 = 要求["対象"]
    if (type(群) is not list or len(群) > 8 or any(type(名) is not str or not 名.strip() or len(名) > 128 for 名 in 群)
            or len(set(群)) != len(群)):
        raise ValueError("関係要求の資料名・件数不正")
    if 要求["範囲"] not in ("指定資料", "全資料") or (要求["範囲"] == "指定資料") != bool(群):
        raise ValueError("関係要求の資料範囲不一致")
    if 要求["形式"] not in _形式 or type(要求["不足調査"]) is not bool:
        raise ValueError("関係要求の形式・調査許可不正")
    if type(要求["最大文字数"]) is not int or not 100 <= 要求["最大文字数"] <= 24000:
        raise ValueError("関係回答の文字予算は100〜24000")
    式, _, _ = 関係文を読む(要求["問い"])
    if 要求["不足調査"] and len(要求["問い"]) > 512:
        raise ValueError("公開調査の問いは512文字以内")
    if 式.種別 not in ("原子", "否定", "含意", "連言", "選言", "全称", "存在", "帰属"):
        raise ValueError("問いの意味構造が未対応")
    return 正準(要求)


def 関係調査要求(要求):
    問い, _, _ = 関係文を読む(要求["問い"])
    原子列 = tuple(原子群(問い))
    語 = tuple(dict.fromkeys(項.名前 for 式 in 原子列 for 項 in 式.項 if 項.種別 == "定数"))
    if not 語:
        語 = tuple(dict.fromkeys(式.述語 for 式 in 原子列))
    結果 = 知識取得要求(検索語=要求["問い"], 必要語=語[:8], 最大検索回数=2, 最大候補数=4,
                        最大取得数=4, 最大資料数=3, 最大抜粋文字数=12000)
    結果.検証()
    return 結果


def _一資料記載を読む(名前, 資料, *, 宣言範囲=()):
    """既存の原記載・例外読解。問いの命題化や判定は行わない。"""
    if not isinstance(資料, 能力結果) or not 資料.成立 or not 0 < len(資料.本文) <= 16000:
        raise ValueError("関係読解の資料型・成立・文字上限")
    区間 = list(構成句を分ける(資料.本文))
    if len(区間) > 64:
        raise ValueError("関係資料の記載数上限")
    if (type(宣言範囲) not in (tuple, list)
            or any(type(項) not in (tuple, list) or len(項) != 2
                   or any(type(値) is not int for 値 in 項) or tuple(項) not in 区間 for 項 in 宣言範囲)
            or len({tuple(項) for 項 in 宣言範囲}) != len(宣言範囲)):
        raise ValueError("関係資料の宣言範囲は一意の原構成句区間が必要")
    宣言範囲 = tuple(tuple(項) for 項 in 宣言範囲)
    記載, 読解, 残差 = [], [], []
    先行 = None
    保留 = None
    限定隔離 = False

    def 残す(開始, 終了, 理由):
        残差.append({"範囲": [開始, 終了], "原文": 資料.本文[開始:終了], "理由": 理由})

    def 読むだけ(識別子, 式, 開始, 終了, 変換):
        原文 = 資料.本文[開始:終了]
        読解.append({"識別子": 識別子, "範囲": [開始, 終了], "原文": 原文,
                     "式": 式.辞書(), "変換": list(変換)})

    def 採る(識別子, 式, 開始, 終了, 変換):
        原文 = 資料.本文[開始:終了]
        記載.append(命題記載(識別子, 式, 名前, 原文, (開始, 終了)))
        読むだけ(識別子, 式, 開始, 終了, 変換)

    def 保留を採る():
        nonlocal 保留
        if 保留 is not None:
            採る(保留["識別子"], 保留["式"], 保留["開始"], 保留["終了"], 保留["変換"])
            保留 = None

    def 例外で確定(基本, 例外原文, 例外開始, 例外終了):
        if 基本["式"].種別 != "含意":
            raise ValueError("例外節の直前記載が条件規則ではない")
        条件, 例外帰結, 例外変換 = 例外節を読む(例外原文)
        本則 = 結合("含意", 結合("連言", 基本["式"].子[0], 反対(条件)), 基本["式"].子[1])
        全開始, 全終了 = 基本["開始"], 例外終了
        採る(基本["識別子"] + ":本則", 本則, 全開始, 全終了,
            [*基本["変換"], *例外変換, "例外適用:本則には例外条件の明示否定も必要"])
        if 例外帰結 is not None:
            採る(基本["識別子"] + ":例外", 結合("含意", 条件, 例外帰結), 例外開始, 例外終了,
                [*例外変換, "例外時帰結:例外条件→明示帰結"])

    for 番号, (開始, 終了) in enumerate(区間):
        原文 = 資料.本文[開始:終了]
        if (開始, 終了) in 宣言範囲:
            保留を採る()
            先行 = None
            continue
        stripped = 原文.strip()
        if stripped.startswith("#"):
            保留を採る()
            先行 = None
            残す(開始, 終了, "見出し（参照範囲を再開）")
            continue

        # 一つの構成句に「本則、ただし例外」が明示される場合。
        try:
            例外位置 = list(最上位位置(原文, ("ただし",)))
        except ValueError as 例外:
            保留を採る(); 先行 = None
            残す(開始, 終了, str(例外)); continue
        if 例外位置 and 例外位置[0][0] > 0:
            保留を採る()
            if len(例外位置) != 1:
                限定隔離 = True; 先行 = None
                残す(開始, 終了, "複数例外の作用域が未対応")
                continue
            位置, _ = 例外位置[0]
            基本文 = 原文[:位置].rstrip("、, ")
            例外文 = 原文[位置:]
            try:
                基本式, 基本主体, 基本変換 = 関係文を読む(基本文, 先行主体=先行)
                基本 = {"識別子": "記載" + str(番号), "式": 基本式, "開始": 開始,
                        "終了": 開始 + len(基本文), "変換": 基本変換, "主体": 基本主体}
                例外で確定(基本, 例外文, 開始 + 位置, 終了)
                先行 = None
            except ValueError as 例外:
                限定隔離 = True; 先行 = None
                残す(開始, 終了, "例外付き記載を採用しない:" + str(例外))
            continue

        # 別句の「ただし」は直前一件だけへ接続する。直前を先に事実化しない。
        if stripped.startswith("ただし"):
            if 保留 is None:
                限定隔離 = True; 先行 = None
                残す(開始, 終了, "例外節の直前規則がない")
                continue
            基本 = 保留; 保留 = None
            try:
                例外で確定(基本, 原文, 開始, 終了)
            except ValueError as 例外:
                限定隔離 = True
                読むだけ(基本["識別子"], 基本["式"], 基本["開始"], 基本["終了"],
                         [*基本["変換"], "後続例外が未解釈のため推論採用を保留"])
                残す(基本["開始"], 基本["終了"], "後続例外の作用域未確定:" + str(例外))
                残す(開始, 終了, "例外節を採用しない:" + str(例外))
            先行 = None
            continue

        # 「例外がある」のように作用域を特定できない後置記載は、直前だけを局所隔離する。
        if stripped.startswith("例外") and 保留 is not None:
            限定隔離 = True
            基本 = 保留; 保留 = None
            読むだけ(基本["識別子"], 基本["式"], 基本["開始"], 基本["終了"],
                     [*基本["変換"], "後続例外が未解釈のため推論採用を保留"])
            残す(基本["開始"], 基本["終了"], "後続例外の作用域が未確定")
            残す(開始, 終了, "例外の条件・作用域が未対応")
            先行 = None
            continue

        # 現在句が例外ではないと確定してから、直前の保留記載を採用する。
        保留を採る()
        try:
            式, 新先行, 変換 = 関係文を読む(原文, 先行主体=先行)
            保留 = {"識別子": "記載" + str(番号), "式": 式, "開始": 開始, "終了": 終了,
                    "変換": 変換, "主体": 新先行}
            先行 = 新先行
        except ValueError as 例外:
            先行 = None
            if any(語 in 原文 for 語 in ("ただし", "除く", "除き", "例外", "限り", "のみ")):
                限定隔離 = True
            残す(開始, 終了, str(例外))
    保留を採る()

    状態 = "限定作用域の一部確認待ち" if 限定隔離 else "部分解釈" if 残差 else "対応文法内の解釈成立"
    return {"資料": 結果を保存(資料), "読解": 読解, "残差": 残差, "限定隔離": 限定隔離,
            "解釈状態": 状態, "採用記載": [asdict(項) for 項 in 記載],
            "宣言範囲": [list(項) for 項 in 宣言範囲]}


def _採用を復元(群):
    return tuple(命題記載(項["識別子"], 命題を復元(項["式"], 内部許可=True), 項["資料"],
                       項["原文"], tuple(項["範囲"])) for 項 in 群)


def _一資料(名前, 資料, 問い, *, 宣言範囲=(), 採用保持=False):
    元 = _一資料記載を読む(名前, 資料, 宣言範囲=宣言範囲)
    判定, 説明, 不足 = 記載を判定(_採用を復元(元["採用記載"]), 問い)
    結果 = {名: 元[名] for 名 in ("資料", "読解", "残差", "限定隔離", "解釈状態")}
    結果.update(判定=判定, 説明=説明, 不足候補=不足)
    if 採用保持:
        結果["採用記載"] = 元["採用記載"]
    return 結果


def 主張採用要求を検査(要求):
    """原HDSへの拘束を保持するshape検査。原Pの命題成立を要求しない。"""
    欄 = {"版", "対象", "範囲", "問い", "形式", "不足調査", "最大文字数", "原主張"}
    if type(要求) is not dict or set(要求) != 欄 or 要求["版"] != 主張採用要求版:
        raise ValueError("主張採用要求の型・欄・版不一致")
    名 = 要求["対象"]
    if (type(名) is not list or not 1 <= len(名) <= 8 or len(set(名)) != len(名)
            or any(type(項) is not str or not 項.strip() or len(項) > 128 for 項 in 名)
            or 要求["範囲"] != "指定資料" or 要求["不足調査"] is not False
            or 要求["形式"] not in _形式 or type(要求["最大文字数"]) is not int
            or not 100 <= 要求["最大文字数"] <= 24000
            or type(要求["問い"]) is not str or not 0 < len(要求["問い"]) <= 16000):
        raise ValueError("主張採用要求の資料・形式・予算・原Pが不正")
    元 = 要求["原主張"]
    原欄 = {"所有セッション", "原文", "目的ID", "文節ID", "行為", "行為座標ID", "行為範囲", "行為原文",
          "主張", "資料", "契約印", "意味IR印", "HDS残差"}
    if (type(元) is not dict or set(元) != 原欄 or 元["行為"] not in ("支持確認", "断定要求", "保証要求")
            or any(type(元[項]) is not str or not 元[項] for 項 in
                   ("所有セッション", "原文", "目的ID", "文節ID", "行為座標ID", "行為原文"))
            or any(type(元[項]) is not str or len(元[項]) != 64
                   or any(字 not in "0123456789abcdef" for 字 in 元[項]) for 項 in ("契約印", "意味IR印"))
            or type(元["HDS残差"]) is not list):
        raise ValueError("主張採用要求の原HDS由来が不正")
    def 原区間(範囲, 本文):
        return (type(範囲) is list and len(範囲) == 2 and all(type(値) is int for 値 in 範囲)
                and 0 <= 範囲[0] < 範囲[1] <= len(元["原文"]) and 元["原文"][slice(*範囲)] == 本文)
    主張 = 元["主張"]
    if (type(主張) is not dict or set(主張) != {"座標ID", "原文範囲", "本文"}
            or type(主張["座標ID"]) is not str or not 主張["座標ID"] or 主張["本文"] != 要求["問い"]
            or not 原区間(主張["原文範囲"], 主張["本文"])
            or not 原区間(元["行為範囲"], 元["行為原文"])):
        raise ValueError("主張採用要求の原行為・原P区間不一致")
    資料 = 元["資料"]
    if type(資料) is not dict or set(資料) != {"方式", "対象", "区分ID", "原文範囲", "本文"}:
        raise ValueError("主張採用要求の資料由来欄不一致")
    if 資料["方式"] == "提示原文":
        if (名 != ["提示本文"] or 資料["対象"] != [] or type(資料["区分ID"]) is not str
                or not 資料["区分ID"] or not 原区間(資料["原文範囲"], 資料["本文"])):
            raise ValueError("主張採用要求の提示原資料区間不一致")
    elif 資料["方式"] == "登録資料":
        if 資料["対象"] != 名 or 資料["区分ID"] is not None or 資料["原文範囲"] is not None or 資料["本文"] != "":
            raise ValueError("主張採用要求の登録原資料対応不一致")
    else:
        raise ValueError("主張採用要求の資料方式不正")
    return 正準(要求)


def 主張採用を読む(資料群, 要求):
    要求 = 主張採用要求を検査(要求)
    if (type(資料群) is not dict or set(資料群) != set(要求["対象"])
            or any(not isinstance(値, 能力結果) or not 値.成立 or type(値.本文) is not str for 値 in 資料群.values())
            or sum(len(値.本文) for 値 in 資料群.values()) > 32000):
        raise ValueError("主張採用の資料対応・結果型・合計文字数不正")
    if (要求["原主張"]["資料"]["方式"] == "提示原文"
            and 資料群["提示本文"].本文 != 要求["原主張"]["資料"]["本文"]):
        raise ValueError("主張採用の実提示本文が原材料と不一致")
    P = 要求["原主張"]["主張"]
    try:
        問い, _, 変換 = 関係文を読む(要求["問い"])
        if 要求["原主張"]["HDS残差"]:
            raise ValueError("原HDS残差の原主張への影響が未確認")
        P読解 = {"状態": "全文型成立", "式": 問い.辞書(), "変換": list(変換), "原主張": P,
                "診断": [], "未型範囲": []}
    except ValueError as 例外:
        問い = None
        P読解 = {"状態": "未型・判定未実施", "式": None, "変換": [], "原主張": P, "診断": [str(例外)],
                "未型範囲": [{"種別": 種, "範囲": P["原文範囲"], "状態": "個別対応未確定"}
                            for 種 in ("対象", "条件", "期間", "量化")]}
    群 = {名: _一資料記載を読む(名, 値) for 名, 値 in sorted(資料群.items())}
    for 行 in 群.values():
        # 命題に採用できない原記載も、局所役割と未読を別欄で保持する。
        # 後続例外で隔離された型をこの経路から再採用しない。
        局所 = []
        for 番号, 残差 in enumerate(行["残差"]):
            if any(語 in 残差["理由"] for 語 in ("例外", "限定作用域", "留保")):
                continue
            if (番号 + 1 < len(行["残差"])
                    and 行["残差"][番号 + 1]["原文"].lstrip().startswith(("ただし", "例外"))):
                continue
            局所.append(材料局所役割を読む(残差["原文"], 起点=残差["範囲"][0]))
        行["材料局所読解"] = 局所
        if 問い is None:
            行.update(判定=None, 説明=None, 不足候補=[], 支持状態="P未型のため判定未実施")
        else:
            判定, 説明, 不足 = 記載を判定(_採用を復元(行["採用記載"]), 問い)
            行.update(判定=判定, 説明=説明, 不足候補=不足, 支持状態="読めた記載内の判定")
    行為 = 要求["原主張"]["行為"]
    return 正準({"版": 主張採用読解版, "要求": 要求, "P読解": P読解, "資料群": 群,
                 "公開資料群": {}, "取得": None, "取得状態": "未要求",
                 "行為検討": {"要求行為": 行為, "要求行為実行": False,
                     "説明範囲": "資料内の記載支持と原主張の読解状態", "世界断定": "未認定", "保証": "未認定"},
                 "境界": "原要求行為を保持して資料の記載を検討する。記載の支持は世界の断定・保証ではなく、原P未型は材料の反証や世界の偽を意味しない。"})


def 記載を判定(採用, 問い):
    """同じ命題推論・実導出説明を単一資料と明示横断の両方で使う。"""
    推論器 = 命題推論器(採用, 上限=推論上限(操作数=12000, 事実数=256, 項数=48, 根拠数=1024)) if 採用 else None
    判定 = 推論器.判定(問い) if 推論器 is not None else {
        "判定": "未確定", "問い": 問い.辞書(), "支持": None, "反証": None, "導出": {}, "操作数": 0,
        "解釈境界": "採用可能な記載がないため推論を行っていない。問いの否定ではない。"}
    説明 = 導出説明を構成(判定, [asdict(項) for 項 in 採用])
    不足 = []
    # 後件から前件を事実推論しない。既存ルールを使う場合の確認候補だけを得る。
    if 判定["判定"] != "支持" and 推論器 is not None:
        for 項 in 採用:
            if 項.式.種別 != "含意" or 項.式.子[1].鍵() != 問い.鍵():
                continue
            前件 = 項.式.子[0]
            前判定 = 推論器.判定(前件)
            if 前判定["判定"] != "支持":
                不足.append({"規則記載": 項.識別子, "前件": 前件.辞書(), "判定": 前判定["判定"],
                             "位置づけ": "この導出経路を使う場合の確認候補。事実・必要条件とは未認定"})
    return 判定, 説明, 不足


def 不足調査が必要(構造):
    if "横断" in 構造:
        横断 = 構造["横断"]
        return (横断["状態"] != "宣言条件内の横断判定" or 横断["判定"]["判定"] != "支持"
                or bool(横断["保留"]) or any(行["残差"] for 行 in 構造["資料群"].values()))
    return any(行["限定隔離"] or 行["残差"] or 行["判定"]["判定"] != "支持" for 行 in 構造["資料群"].values())


def 関係資料を読む(資料群, 要求, *, 取得=None):
    if type(要求) is dict and 要求.get("版") == "HDS資料条件変更要求-v2":
        if 取得 is not None:
            raise ValueError("資料条件変更の外部取得は未接続")
        from .資料条件変更 import 資料条件変更を読む
        return 資料条件変更を読む(資料群, 要求)
    if type(要求) is dict and 要求.get("版") == 主張採用要求版:
        if 取得 is not None:
            raise ValueError("主張採用の外部取得は未接続")
        return 主張採用を読む(資料群, 要求)
    要求 = 関係要求を検査(要求)
    if (type(資料群) is not dict or not 1 <= len(資料群) <= 8
            or any(type(名) is not str or not 名 or len(名) > 128 for 名 in 資料群)):
        raise ValueError("関係資料の名前・件数不正")
    if 要求["範囲"] == "指定資料" and set(資料群) != set(要求["対象"]):
        raise ValueError("関係要求と資料の対応不一致")
    if any(not isinstance(値, 能力結果) or not 値.成立 or type(値.本文) is not str for 値 in 資料群.values()):
        raise ValueError("関係資料の結果型・成立不正")
    if sum(len(値.本文) for 値 in 資料群.values()) > 32000:
        raise ValueError("関係資料の合計文字数上限")
    問い, _, 変換 = 関係文を読む(要求["問い"])
    横断指定 = 要求["版"] == "HDS資料横断要求-v1"
    宣言 = {}
    if 横断指定:
        from .資料横断 import 資料宣言を読む
        宣言 = {名: 資料宣言を読む(値.本文) for 名, 値 in 資料群.items()}
    結果 = {"版": 関係読解版, "要求": 要求, "問い式": 問い.辞書(), "問い変換": 変換,
          "資料群": {名: _一資料(名, 値, 問い,
                  宣言範囲=tuple(tuple(区間) for 区間 in 宣言[名]["範囲"]) if 横断指定 else (),
                  採用保持=横断指定) for 名, 値 in sorted(資料群.items())},
          "公開資料群": {}, "取得": None, "取得状態": "未要求"}
    if 横断指定:
        from .資料横断 import 資料を結び付ける
        結果["横断"] = 資料を結び付ける(結果["資料群"], 宣言, 要求, 問い)
    if 取得 is not None:
        if not 要求["不足調査"]:
            raise ValueError("公開調査を要求していない")
        if not isinstance(取得, 能力結果):
            raise ValueError("取得の結果型不正")
        # 外部読取自体の不成立も観測として保存。失敗を知識成立へ変えない。
        if 取得.成立:
            if 正準(取得.データ.get("要求")) != 正準(asdict(関係調査要求(要求))):
                raise ValueError("取得記録と質問の対応不一致")
            公開 = 取得資料を復元(取得)
            if len(公開) > 3 or sum(len(値.本文) for 値 in 公開.values()) > 32000:
                raise ValueError("公開関係資料の容量上限")
            結果["公開資料群"] = {名: _一資料(名, 値, 問い) for 名, 値 in sorted(公開.items())}
        結果["取得"] = 結果を保存(取得)
        結果["取得状態"] = "取得済（意味的十分性は別判定）" if 取得.成立 else "取得不成立"
    elif 要求["不足調査"]:
        結果["取得状態"] = "必要" if 不足調査が必要(結果) else "不要"
    結果["境界"] = "資料ごとの読めた記載における支持・反証。十分条件と必要条件の向きを区別し、例外付き本則は例外条件の明示否定がある場合だけ適用する。資料間の同名対象、時点・条件の同一性や世界の真偽を認定しない。条件適用は現実の因果検証ではない。"
    return 正準(結果)


def 関係資料を検査(構造):
    try:
        if type(構造) is not dict or 構造.get("版") not in (関係読解版, 主張採用読解版, "HDS資料条件変更読解-v1"):
            return False
        再 = 関係資料を読む({名: 結果を復元(行["資料"]) for 名, 行 in 構造["資料群"].items()},
                        構造["要求"], 取得=None if 構造["取得"] is None else 結果を復元(構造["取得"]))
        return 指紋(再) == 指紋(構造)
    except (ValueError, TypeError, KeyError, AttributeError, RecursionError, OverflowError):
        return False

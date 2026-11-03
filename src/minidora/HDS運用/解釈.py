"""HDSの知覚・計画作用。既存の意味解釈器を再利用し、実行は行わない。"""
from __future__ import annotations
from copy import deepcopy
from dataclasses import asdict, replace
from enum import Enum
import re
from .構文化接続 import 運用構文化器
from ..HDS適合器 import HDS文脈
from ..HDS目的射影 import HDSから目的要求, _HDS照合
from ..会話解釈 import 会話を解釈, HDS会話を照合
from ..会話意味 import 会話要求, 比較対象, 意味目的
from ..汎用要求IR import 汎用要求IR, 目的指定
from ..能力合成 import 合成計画, 合成工程, 素材参照, _結果辞書, _打切り
from ..会話回答 import 回答記録整合
from ..能力結果復元 import 能力結果を復元
from ..監査改善会話解釈 import 改善発話を解釈
from ..監査改善接続 import 改善回答を検査
from ..製品版.型 import 能力結果
from .値 import 正準, 指紋, 結果を保存, 結果を復元, 計画を保存, 運用版


def 構造を保存(value):
    if isinstance(value, Enum):
        return value.value
    if hasattr(value, "__dataclass_fields__"):
        return 構造を保存(asdict(value))
    if type(value) is dict:
        return {k: 構造を保存(v) for k, v in value.items()}
    if type(value) in (list, tuple):
        return [構造を保存(v) for v in value]
    if type(value) in (set, frozenset):
        return sorted((構造を保存(v) for v in value), key=指紋)
    return 正準(value)


def 要求を復元(raw):
    value = deepcopy(raw)
    value["対象"] = tuple(比較対象(t["資料"], tuple(tuple(p) for p in t["行条件"])) for t in value["対象"])
    value["対応"] = tuple(tuple(p) for p in value["対応"])
    return 会話要求(**value).固定複製()


def 型付き要求を保存(request):
    if type(request) is not 汎用要求IR:
        raise TypeError("型付き目的は汎用要求IRが必要")
    request = request.固定複製()
    raw = asdict(request)
    raw["素材"] = {k: 結果を保存(v) for k, v in request.素材.items()}
    return 正準(raw)


def 型付き要求を復元(raw):
    d = deepcopy(raw)
    if set(d) != set(汎用要求IR.__dataclass_fields__):
        raise ValueError("型付き目的の欄不一致")
    d["素材"] = {k: 結果を復元(v) for k, v in d["素材"].items()}
    d["目的"] = tuple(目的指定(**(row | {"原文範囲": tuple(row["原文範囲"])})) for row in d["目的"])
    d["出力目的"], d["残差"] = tuple(d["出力目的"]), tuple(d["残差"])
    return 汎用要求IR(**d).固定複製()


def _会話焦点を構成(目的, *, 前採用会話=None, 前命令候補=None):
    """前採用の意味役割を供与し、過去HDSの再帰複製は実庫へ任せる。

    元目的のHDS全文・手順は保存目的と原記録へ保全する。今回のCompiler
    には原依頼、契約包と呼出元が再発行した参照証明だけを渡す。
    """
    if type(目的) is dict and 目的.get("方式") == "一般会話目的束":
        焦点 = {"方式": 目的["方式"], "原文": 目的["原文"],
              "要求": deepcopy(目的["要求"])}
    else:
        焦点 = deepcopy(目的)
    if 前採用会話 is not None or 前命令候補 is not None:
        if type(焦点) is not dict:
            raise ValueError("会話焦点に対応する目的がありません")
        if 前採用会話 is not None:
            焦点["前採用会話"] = deepcopy(前採用会話)
        if 前命令候補 is not None:
            焦点["前命令候補"] = deepcopy(前命令候補)
    return 焦点


def 依頼を解釈(入力):
    raw = 入力["原文"]
    if 入力.get("文脈要求") is not None:
        return {"方式": "文脈目的", "要求": 入力["文脈要求"], "原文": raw}
    if 入力.get("型付き目的") is not None:
        request = 型付き要求を復元(入力["型付き目的"])
        if request.原文 != raw:
            raise ValueError("型付き目的と原依頼が不一致")
        return {"方式": "型付き目的", "目的": 入力["型付き目的"], "原文": raw}
    if 入力.get("意味目的") is not None:
        goal = 意味目的(**入力["意味目的"])
        goal.鍵()
        return {"方式": "明示役割", "目的": asdict(goal), "原文": raw}
    if 入力.get("明示合成") is not None:
        return {"方式": "明示合成", "合成": 入力["明示合成"], "原文": raw}
    構文化器 = 運用構文化器()
    文脈目的 = 入力.get("保留目的") or 入力.get("前回目的")
    前採用 = 入力.get("前採用会話")
    有効前採用 = (前採用 if 入力.get("前回有効") and 文脈目的 == 入力.get("前回目的") else None)
    文脈目的 = _会話焦点を構成(文脈目的, 前採用会話=有効前採用,
                            前命令候補=入力.get("前命令候補"))
    文脈 = HDS文脈(現在焦点=文脈目的,
                   記憶引用=tuple(入力.get("前回依存", {})),
                   直前採否="採用" if 入力.get("前回有効") else None)
    ir = 構文化器.コンパイル(raw, 文脈=文脈)
    saved = {"全文": 構造を保存(構文化器.原IR), "局所作用ビュー": 構造を保存(ir),
             "コア入力": 構造を保存(構文化器.コア入力),
             "責任対応": 構造を保存(構文化器.責任対応)}
    from .数量依頼 import 数量依頼を読む
    数量要求 = 数量依頼を読む(入力)
    if 数量要求 is not None:
        return {**数量要求, "HDS": saved}
    知識照会 = re.fullmatch(r'知識(?:全体)?から「([^「」]+)」(?:の根拠)?を(?:説明|確認)して[。？?]?', raw.strip())
    if 知識照会:
        return {"方式": "知識横断", "問い": 知識照会[1], "原文": raw, "HDS": saved}
    原文照会 = re.fullmatch(r'資料「([^「」]+)」の原文を再参照して[。？?]?', raw.strip())
    記憶照会 = re.fullmatch(r'記憶から「([^「」]+)」を探して[。？?]?', raw.strip())
    if 原文照会 or 記憶照会:
        return {"方式": "文脈目的", "原文": raw, "HDS": saved,
                "要求": {"必須資料": [原文照会[1]] if 原文照会 else [],
                         "検索語": [記憶照会[1]] if 記憶照会 else [], "最大バイト数": 32768}}
    if raw.strip() in ("再計算して", "もう一度", "続けて"):
        old = 入力.get("保留目的") or 入力.get("前回目的")
        if old is None:
            raise ValueError("再開する目的がない")
        if old.get("方式") != "一般会話目的束":
            return {"方式": "継続", "目的": old, "原文": raw, "HDS": saved}
    science = re.fullmatch(r'資料「([^「」]+)」の問題を解いて[。？?]?', raw.strip())
    if science:
        return {"方式": "科学目的", "資料": science[1], "原文": raw, "HDS": saved}
    # 知識も提供資料として版・出典付きで保存し、実行命令にはしない。
    knowledge = re.fullmatch(r'知識「([^「」\n]+)」を(登録|更新)[:：]([\s\S]+)', raw.strip())
    if knowledge:
        return {"方式": "管理", "行為": knowledge[2], "名前": knowledge[1],
                "本文": knowledge[3], "種類": "知識", "データ": {}, "原文": raw, "HDS": saved}
    # 既存の本文・命題・仮説・介入の文法を同じ入口へ接続する。
    improved = None
    if raw.startswith(("本文資料", "命題資料", "仮説資料", "介入資料")):
        improved = 改善発話を解釈(raw)
    else:
        try:
            候補 = 改善発話を解釈(raw)
            if 候補["行為"] == "検討":
                improved = 候補
            elif 候補["行為"] == "訂正" and (入力.get("保留目的") or 入力.get("前回目的") or {}).get("方式") == "資料検討":
                improved = 候補
        except ValueError:
            pass
    if improved is not None:
        if improved["行為"] in ("登録", "更新"):
            return {"方式": "管理", "行為": improved["行為"], "名前": improved["資料"],
                    "本文": improved["本文"], "種類": improved["種類"], "データ": improved["データ"],
                    "原文": raw, "HDS": saved}
        if improved["行為"] == "訂正":
            old = deepcopy(入力.get("保留目的") or 入力["前回目的"])
            old["依頼"]["変更"].update(improved["変更"])
            old["原文"] = raw
            old["HDS"] = saved
            return old
        return {"方式": "資料検討", "依頼": improved, "原文": raw, "HDS": saved}
    from .関係依頼 import 関係依頼を読む
    from ..HDS会話意味射影 import 会話未読解
    try:
        関係要求 = 関係依頼を読む(入力, 意味IR=ir)
    except 会話未読解 as 例外:
        return {"方式": "会話確認", "原文": raw, "診断": deepcopy(例外.診断), "HDS": saved}
    if 関係要求 is not None:
        return {**関係要求, "HDS": saved}
    from .一般依頼 import 一般依頼を読む
    from ..HDS会話意味射影 import 会話未読解
    try:
        一般要求 = 一般依頼を読む(入力, 意味IR=ir)
    except 会話未読解 as 例外:
        return {"方式": "会話確認", "原文": raw, "診断": deepcopy(例外.診断), "HDS": saved}
    if 一般要求 is not None:
        return {**一般要求, "HDS": saved}
    request = 会話を解釈(raw, tuple(入力["資料"]))
    if request.行為 in ("登録", "更新"):
        return {"方式": "管理", "行為": request.行為, "名前": request.対象[0].資料,
                "本文": request.補助["本文"], "種類": "資料", "データ": {}, "原文": raw, "HDS": saved}
    if request.行為 in ("初期化", "会話"):
        return {"方式": "管理", "行為": request.行為, "発話": request.補助.get("発話", ""), "原文": raw, "HDS": saved}
    if request.行為 in ("訂正", "確認返答"):
        target = 入力.get("保留目的") or 入力.get("前回目的")
        if target is None or target.get("方式") != "会話":
            raise ValueError("訂正・確認に対応する目的がない")
        old = 要求を復元(target["依頼"])
        if old.行為 not in ("比較", "集合", "取得"):
            raise ValueError("この目的のスロット訂正は未対応")
        key, value = request.補助["欄"], request.補助["値"]
        kw = {}
        if key in ("属性", "単位"):
            kw[key] = value
        elif key in ("年", "左の年", "右の年"):
            if not re.fullmatch(r"[0-9]{4}", value) or not old.対象:
                raise ValueError("年の訂正対象・値が未確定")
            targets = list(old.対象)
            indices = range(len(targets)) if key == "年" else (0 if key == "左の年" else 1,)
            for index in indices:
                targets[index] = replace(targets[index], 行条件=tuple({**dict(targets[index].行条件), "年": value}.items()))
            kw = {"対象": tuple(targets), "時点差": key != "年"}
        else:
            raise ValueError("未対応の訂正欄")
        HDS会話を照合(ir, request)
        request = replace(old, 原文=raw, 補助=old.補助 | {"改訂元": old.補助.get("改訂元", old.原文)},
                          対応=(("目的の明示改訂", 0, len(raw)),), **kw).固定複製()
    elif request.行為 not in ("既存目的", "再表現", "命題照合", "命題取得"):
        HDS会話を照合(ir, request, 文脈解消=bool(request.補助.get("資料参照解消")))
    return {"方式": "会話", "依頼": 正準(asdict(request)), "原文": raw, "HDS": saved}


def _形式(value):
    explicit = value.データ.get("形式")
    if explicit in ("JSON", "CSV"):
        return explicit
    text = value.本文.lstrip()
    if text.startswith(("{", "[")):
        return "JSON"
    if "," in text.split("\n", 1)[0]:
        return "CSV"
    raise ValueError("比較資料はJSON又は見出し付きCSVが必要")


def _数量目的(request, materials):
    targets = []
    for target in request.対象:
        if request.補助.get("供給") == "取得":
            targets.append({"主題": target.資料, "属性": request.属性, "単位": request.単位})
        else:
            if target.資料 not in materials:
                raise ValueError("資料不足:" + target.資料)
            targets.append({"資料": target.資料, "形式": _形式(materials[target.資料]),
                            "属性": request.属性, "単位": request.単位, "行条件": dict(target.行条件)})
    if request.行為 == "比較":
        if len(targets) != 2:
            raise ValueError("比較は二つの対象が必要")
        return 意味目的("比較回答", {"左": targets[0], "右": targets[1], "時点差": request.時点差, "詳細": request.詳細})
    aux = request.補助
    return 意味目的("集合回答", {"対象": targets, "供給": aux.get("供給", "資料"), "操作": list(aux["操作"]),
                "時点差": request.時点差, "詳細": request.詳細, "形式": aux["形式"], "手順": aux["手順"],
                "選別": aux["選別"], "除外資料": list(aux["除外資料"])})


def 計画を構成(解釈, 入力, 目録, *, 禁止=()):
    """依頼から計画を構成するだけ。計画中に能力本体を実行しない。"""
    mode = 解釈["方式"]
    if mode == "会話確認":
        from ..HDS会話意味射影 import 会話未読解, 会話残差を読む
        from ..HDS再生 import HDSIR復元
        from .構文化接続 import 局所接続を構成
        if set(解釈) != {"方式", "原文", "診断", "HDS"} or 解釈["原文"] != 入力["原文"]:
            raise ValueError("会話確認の実原文又は欄が不一致です")
        保存 = 解釈["HDS"]
        if (type(保存) is not dict or set(保存) != {"全文", "局所作用ビュー", "コア入力", "責任対応"}
                or type(保存["全文"]) is not dict or type(保存["局所作用ビュー"]) is not dict
                or 保存["全文"].get("手順") is not None):
            raise ValueError("会話確認の保存HDS欄又は再生対応が不正です")
        再 = HDSIR復元(保存["全文"])
        # 再生器の省略時既定や型変換を、未読解の保存契約へ持ち込まない。
        if 指紋(構造を保存(再)) != 指紋(保存["全文"]) or 再.原文 != 解釈["原文"]:
            raise ValueError("会話確認のHDS全文が保存時の型・欄・原文と完全同値ではありません")
        局所, 責任 = 局所接続を構成(再)
        原IR = 構造を保存(局所)
        if (指紋(原IR) != 指紋(保存["局所作用ビュー"])
                or 指紋(構造を保存(責任)) != 指紋(保存["責任対応"])):
            raise ValueError("会話確認の局所作用ビュー又は責任対応がHDS全文と不一致です")
        原診断 = 会話残差を読む(局所)
        if 原IR["原文"] != 解釈["原文"] or type(解釈["診断"]) is not list or not 解釈["診断"]:
            raise ValueError("会話確認の原IR又は診断が不正です")
        原残差 = {項["残差ID"]: 項 for 項 in 原IR["残差"] if 項["種別"] == "会話未消費"}
        if (len(原残差) != len(解釈["診断"])
                or set(原残差) != {項["残差ID"] for 項 in 解釈["診断"]}):
            raise ValueError("会話確認の残差被覆が不一致です")
        for 項 in 解釈["診断"]:
            if set(項) != {"残差ID", "種別", "原文", "原文範囲", "理由", "影響座標", "解消条件"}:
                raise ValueError("会話確認の診断欄が不正です")
            元 = 原残差[項["残差ID"]]
            if any(項[名] != 元[名] for 名 in ("種別", "原文", "理由", "影響座標", "解消条件")):
                raise ValueError("会話確認の診断と原残差が不一致です")
            if type(項["原文範囲"]) is not list or not 項["原文範囲"]:
                raise ValueError("会話確認の原文範囲が空又は不正です")
            for 始, 終 in 項["原文範囲"]:
                if (type(始) is not int or type(終) is not int or not 0 <= 始 < 終 <= len(解釈["原文"])
                        or 解釈["原文"][始:終] != 項["原文"]):
                    raise ValueError("会話確認の原文範囲が不一致です")
        if 指紋(解釈["診断"]) != 指紋(原診断):
            raise ValueError("会話確認の診断がHDS全文の制御範囲・残差構造と不一致です")
        raise 会話未読解(解釈["診断"])
    if mode == "継続":
        return 計画を構成(解釈["目的"], 入力, 目録, 禁止=禁止)
    sources = {k: 結果を復元(v["結果"]) for k, v in 入力["資料"].items()}
    goal = None
    detail = False
    kind = "会話回答"
    request = None
    if mode == "管理":
        return {"方式": "管理", "解釈": deepcopy(解釈), "原文": 入力["原文"]}
    if mode in ("数量言語", "数量再表現"):
        from .数量依頼 import 数量計画を作る
        plan, materials, kind = 数量計画を作る(解釈, 入力)
        coverage = []
    elif mode in ("関係資料", "関係再表現", "関係検討"):
        from .関係依頼 import 関係計画を作る
        plan, materials, kind = 関係計画を作る(解釈, 入力)
        coverage = []
    elif mode in ("一般資料", "一般再表現", "一般会話目的束"):
        from .一般依頼 import 一般計画を作る
        plan, materials, kind = 一般計画を作る(解釈, 入力)
        coverage = []
    elif mode == "知識横断":
        資産 = 入力["知識資産"]
        if not 資産:
            raise ValueError("登録された知識資産がない")
        from .知識資産 import 資産を検査
        from ..能力合成 import _参照結合
        for 名前, 値 in 資産.items():
            資産を検査(値)
            if 名前 not in 入力["資料"] or 値["資料版"] != 入力["資料"][名前]["版"]:
                raise ValueError("知識資産と現行資料版が不一致")
        参照 = _参照結合(参照 for 名前 in sorted(資産) for 参照 in sources[名前].参照)
        materials = {"知識束": 能力結果(True, "提供知識の形成資産", 参照=参照,
            データ={"知識": [資産[名前] for 名前 in sorted(資産)], "問い": 解釈["問い"], "詳細": True}),
            "知識指示": 能力結果(True, "提供知識の条件から問いを導出する")}
        plan = 合成計画((合成工程("知識導出", ("知識資産照合",), "知識指示", (素材参照("入力", "知識束"),)),), ("知識導出",))
        coverage = []
    elif mode == "文脈目的":
        from ..長文脈管理 import 文脈選択要求
        設定 = 解釈["要求"]
        if type(設定) is not dict or set(設定) != {"必須資料", "検索語", "最大バイト数"}:
            raise ValueError("文脈要求の欄不正")
        必須 = tuple(入力["原記録資料"][名前] for 名前 in 設定["必須資料"])
        選択要求 = 文脈選択要求(必須, tuple(設定["検索語"]), 0, 設定["最大バイト数"])
        選択要求.検証()
        materials = {"文脈要求": 能力結果(True, "", データ={"起点": 入力["原記録起点"], "選択要求": asdict(選択要求)}),
                     "文脈指示": 能力結果(True, "予算内の原記録を原文対応付きで再参照する")}
        plan = 合成計画((合成工程("原文選択", ("長文脈選択",), "文脈指示", (素材参照("入力", "文脈要求"),)),), ("原文選択",))
        coverage = []
    elif mode == "明示役割":
        materials = sources
        goal = 意味目的(**解釈["目的"])
        kind = "能力成果"
    elif mode == "科学目的":
        materials = sources
        if 解釈["資料"] not in materials:
            raise ValueError("問題資料が未登録")
        goal = 意味目的("科学回答", {"資料": 解釈["資料"], "詳細": False})
    elif mode == "明示合成":
        from .値 import 計画を復元
        packed = 解釈["合成"]
        plan = 計画を復元(packed["計画"])
        materials = {k: 結果を復元(v) for k, v in packed["資料"].items()}
        coverage = []
        kind = "能力成果"
    elif mode == "型付き目的":
        purpose = 型付き要求を復元(解釈["目的"])
        planned = 目録.単体目的.計画する(purpose)
        if not planned.成立:
            raise ValueError(planned.理由)
        plan, materials = planned.計画, planned.資料
        coverage = 構造を保存(planned.要求被覆)
    elif mode == "資料検討":
        task = 解釈["依頼"]
        name, category = task["資料"], task["種類"]
        if name not in sources:
            raise ValueError("確認待ち:資料不足:" + name)
        元資料 = sources[name]
        params = {"資料": [{"名前": name, "本文": 元資料.本文}]} if category in ("読解", "命題") else deepcopy(入力["資料"][name]["データ"])
        params.update(task.get("変更", {}))
        original = 能力結果(True, 解釈["原文"], 参照=元資料.参照, データ=params)
        materials = {"原要求": original}
        detail = task.get("詳細", True)
        goal = 意味目的("監査改善の検証済回答", {"種類": category, "要求資料": "原要求", "詳細": detail, "再説明": False})
        kind = "検討回答"
    else:
        request = 要求を復元(解釈["依頼"])
        detail = request.詳細
        if request.行為 == "再表現":
            previous = 入力.get("前回結果")
            if previous is None or not 入力.get("前回有効"):
                raise ValueError("再表現する有効な成果がない。資料更新後は再計算してください")
            old = 結果を復元(previous)
            if old.データ.get("元結果") is not None:
                if not 回答記録整合(old):
                    raise ValueError("前回回答の整合不一致")
                materials = {"前回": old, "指示": 能力結果(True, "実成果から再表現する"),
                             "設定": 能力結果(True, "", データ={"詳細": detail, **request.補助})}
                plan = 合成計画((合成工程("再表現", ("会話再表現",), "指示", (素材参照("入力", "前回"),), "設定"),), ("再表現",))
                coverage = []
                kind = "再表現"
            else:
                if not 改善回答を検査(old.データ):
                    raise ValueError("この成果の再表現契約は未接続")
                if request.補助:
                    raise ValueError("検討回答の表示条件は詳細・簡潔に限定")
                from ..監査改善接続 import 改善回答版
                from ..監査改善計画 import 報告版
                category = next(k for k, v in 報告版.items() if v == old.データ["報告"]["版"])
                materials = {"原要求": 能力結果(True, "", 参照=old.参照, データ=old.データ["報告"]["要求"]),
                             "保存報告": 能力結果(True, "", 参照=old.参照, データ=old.データ["報告"])}
                goal = 意味目的("監査改善の検証済回答", {"種類": category, "要求資料": "原要求", "詳細": detail, "再説明": True})
                kind = "検討回答"
        elif request.行為 in ("比較", "集合"):
            materials = sources
            goal = _数量目的(request, materials)
        elif request.行為 == "取得":
            materials = sources
            goal = 意味目的("取得回答", {"主題": request.補助["主題"], "属性": request.属性, "単位": request.単位, "詳細": detail})
        elif request.行為 == "既存目的":
            canonical = request.補助["射影文"]
            prior = ()
            if re.search("それ|その結果", canonical) and 入力.get("前回有効") and 入力.get("前回結果") is not None:
                old = 結果を復元(入力["前回結果"])
                if 回答記録整合(old):
                    prior = tuple((f"前回:{i}", 能力結果を復元(v)) for i, v in enumerate(old.データ["元結果"]))
            構文化器 = 運用構文化器()
            ir = 構文化器.コンパイル(canonical)
            projected = HDSから目的要求(ir, sources, 前回成果=prior)
            if not projected.成立:
                raise ValueError(projected.理由)
            if canonical != request.原文:
                literals = tuple(m.span() for m in re.finditer('「[^「」]*」', request.原文))
                refs = tuple(m.span() for m in re.finditer("それ|その結果", request.原文))
                equations = tuple((a, b) for a, b in literals if "=" in request.原文[a:b])
                _HDS照合(構文化器.コンパイル(request.原文), literals, refs, equations)
            planned = 目録.単体目的.計画する(projected.要求)
            if not planned.成立:
                raise ValueError(planned.理由)
            plan, materials = planned.計画, planned.資料
            coverage = 構造を保存(planned.要求被覆)
        else:
            raise ValueError("この会話行為はHDS通常運用に未接続:" + request.行為)
    役割データ = {}
    if goal is not None:
        allow = 入力["外部許可"] and not (request and request.外部禁止)
        planned = 目録.複数目的.計画する(goal, materials, 禁止=tuple(禁止), 外部許可=allow)
        plan, materials = planned.計画, planned.資料
        coverage = 構造を保存(planned.要求被覆)
        役割データ = {"目的": asdict(goal), "工程作用": 正準(planned.工程作用),
                     "入力役割": 構造を保存(planned.入力役割), "作用契約印": planned.作用契約印}
    elif kind == "会話回答":
        materials = deepcopy(materials)
        materials["運用:回答指示"] = 能力結果(True, "得られた成果を回答へ構成する")
        materials["運用:回答設定"] = 能力結果(True, "", データ={"詳細": detail})
        plan = 合成計画((*plan.工程, 合成工程("運用:回答", ("会話回答構成",), "運用:回答指示",
                            tuple(素材参照("工程", k) for k in plan.出力工程), "運用:回答設定")), ("運用:回答",))
    allow = 入力["外部許可"] and not (request and request.外部禁止)
    try:
        目録.構造検査器._準備(plan, materials, allow)
    except _打切り as exc:
        raise ValueError(str(exc)) from exc
    return 正準({"方式": "能力計画", "計画": 計画を保存(plan),
                 "資料": {k: 結果を保存(v) for k, v in materials.items()}, "被覆": coverage,
                 "回答種別": kind, "外部許可": allow, "目録": 目録.ハッシュ, "役割": 役割データ,
                 "禁止": list(禁止), "原文": 入力["原文"], "解釈": 解釈})

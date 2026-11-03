"""資料の数量定義を依存グラフへ結合し、有理数・単位を保持して評価する。"""
from __future__ import annotations
from copy import deepcopy
from fractions import Fraction
from .数量言語 import 定義を読む, 式を読む, 名前を読む, 参照名, 有理を読む
from .値 import 正準, 指紋

数量構造版 = "HDS数量構造-v1"


def 要求を検査(要求):
    欄 = {"対象", "規則", "数量", "操作", "条件変更", "表示"}
    if type(要求) is not dict or set(要求) != 欄:
        raise ValueError("数量要求の欄不一致")
    for 欄名, 上限 in (("対象", 8), ("規則", 8)):
        群 = 要求[欄名]
        if type(群) is not list or len(群) > 上限 or len(set(群)) != len(群):
            raise ValueError("数量対象・規則の数又は重複")
        for 名前 in 群: 名前を読む(名前)
    if not 要求["対象"] or set(要求["対象"]) & set(要求["規則"]):
        raise ValueError("数量対象と共通規則を区別してください")
    名前を読む(要求["数量"])
    操作 = 要求["操作"]
    if (type(操作) is not list or not 操作 or len(操作) > 7 or len(set(操作)) != len(操作)
            or set(操作) - {"計算", "比較", "合計", "平均", "最大", "最小", "検算"}):
        raise ValueError("数量操作の型・範囲")
    if "比較" in 操作 and len(要求["対象"]) < 2:
        raise ValueError("比較対象が二つ以上必要")
    表示 = 要求["表示"]
    if (type(表示) is not dict or set(表示) != {"詳細", "コード", "形式", "読者"}
            or type(表示["詳細"]) is not bool or type(表示["コード"]) is not bool
            or 表示["形式"] not in ("文章", "表") or 表示["読者"] not in ("一般", "技術者")):
        raise ValueError("数量表示設定の範囲")
    変更 = 要求["条件変更"]
    if type(変更) is not list or len(変更) > 32:
        raise ValueError("条件変更の上限")
    for 項 in 変更:
        if (type(項) is not dict or set(項) != {"資料", "数量", "値", "原文", "範囲"}
                or 項["資料"] not in 要求["対象"]):
            raise ValueError("条件変更の対象・欄不正")
        名前を読む(項["数量"])
        if type(項["原文"]) is not str or len(項["原文"]) > 8192:
            raise ValueError("条件変更の原文不正")
        範囲 = 項["範囲"]
        if (type(範囲) is not list or len(範囲) != 2 or any(type(x) is not int for x in 範囲)
                or not 0 <= 範囲[0] < 範囲[1] <= len(項["原文"])
                or 項["原文"][範囲[0]:範囲[1]] != 項["値"]):
            raise ValueError("条件変更と原発話の対応不一致")
        式 = 式を読む(項["値"], 開始=範囲[0])
        if 参照名(式):
            raise ValueError("条件変更は単位付き数値又は数値式で指定する")
    return deepcopy(要求)


def 数量を構成(資料, 要求):
    要求 = 要求を検査(要求)
    if type(資料) is not dict or set(資料) != set(要求["対象"] + 要求["規則"]):
        raise ValueError("数量の原資料対応が不一致")
    if sum(len(v) for v in 資料.values() if type(v) is str) > 64000:
        raise ValueError("数量資料合計の上限")
    定義 = {名前: 定義を読む(本文) for 名前, 本文 in 資料.items()}
    共通 = {}
    for 元 in 要求["規則"]:
        for 名, 記載 in 定義[元].items():
            if 名 in 共通:
                raise ValueError("共通定義が競合:" + 名)
            共通[名] = (元, 記載)
    節点, 束縛, 未使用, 出力 = [], [], [], []
    使用 = set()
    変更辞書 = {}
    for 変更 in 要求["条件変更"]:
        鍵 = (変更["資料"], 変更["数量"])
        if 鍵 in 変更辞書:
            raise ValueError("同じ数量の条件変更が重複")
        変更辞書[鍵] = 変更

    for 対象 in 要求["対象"]:
        環境 = dict(共通)
        for 名, 記載 in 定義[対象].items():
            if 名 in 環境:
                raise ValueError("対象資料と共通規則の定義競合:" + 名)
            環境[名] = (対象, 記載)
        利用名 = set(環境) | set().union(*(参照名(記載["式"]) for _, 記載 in 環境.values()))
        for (資料名, 名), 項 in 変更辞書.items():
            if 資料名 == 対象 and 名 not in 利用名:
                raise ValueError("原定義と無関係な条件変更:" + 名)
        # 問いと無関係な未解釈文を、架空の量への参照として通過させない。
        # この入口は数量資料全体の有限構文・名前束縛を要求する。
        for 定義名, (_, 記載) in 環境.items():
            for 参照 in sorted(参照名(記載["式"])):
                if 参照 not in 環境 and (対象, 参照) not in 変更辞書:
                    raise ValueError("確認待ち:数量不足:" + 対象 + "の" + 参照)
        解決済, 訪問中 = {}, set()
        def 追加(値):
            if len(節点) >= 64:
                raise ValueError("数量計算グラフの上限64節点")
            鍵 = "項" + str(len(節点))
            節点.append({"識別子": 鍵, **値})
            return 鍵
        def 下ろす(式, 元, 深さ):
            if 深さ > 24:
                raise ValueError("数量依存の深さ上限")
            if 式["種別"] == "参照":
                return 束縛する(式["名前"], 深さ+1)
            場所 = {"資料": 元, "範囲": 式["範囲"]}
            if 式["種別"] == "数値":
                return 追加({"種別":"数値", "値":式["値"], "単位":式["単位"],
                            "単位継承":式.get("単位継承", False), "出典":場所})
            左 = 下ろす(式["左"], 元, 深さ+1)
            右 = 下ろす(式["右"], 元, 深さ+1)
            return 追加({"種別":"演算", "演算":式["演算"], "左":左, "右":右, "出典":場所})
        def 束縛する(名, 深さ=0):
            if 名 in 解決済: return 解決済[名]
            if 名 in 訪問中:
                raise ValueError("数量定義の循環:" + 対象 + "/" + 名)
            訪問中.add(名)
            変更 = 変更辞書.get((対象, 名))
            if 変更:
                元 = "条件変更:" + 対象 + "/" + 名
                記載 = {"式": 式を読む(変更["値"], 開始=変更["範囲"][0]),
                        "範囲":変更["範囲"], "原文":変更["値"]}
            elif 名 in 環境:
                元, 記載 = 環境[名]
                使用.add((元, 名))
            else:
                raise ValueError("確認待ち:数量不足:" + 対象 + "の" + 名)
            鍵 = 下ろす(記載["式"], 元, 深さ)
            解決済[名] = 鍵; 訪問中.remove(名)
            束縛.append({"対象":対象, "数量":名, "節点":鍵, "資料":元,
                         "範囲":記載["範囲"], "原文":記載["原文"]})
            return 鍵
        根 = 束縛する(要求["数量"])
        出力.append({"資料":対象, "数量":要求["数量"], "節点":根})
        for (資料名, 名) in 変更辞書:
            if 資料名 == 対象 and 名 not in 解決済:
                raise ValueError("求める数量に寄与しない条件変更:" + 対象 + "の" + 名)
    for 元, 群 in 定義.items():
        for 名, 記載 in 群.items():
            if (元, 名) not in 使用:
                未使用.append({"資料":元, "数量":名, "範囲":記載["範囲"], "原文":記載["原文"]})
    return 正準({"版":数量構造版, "要求":要求, "資料":資料, "節点":節点,
                 "出力":出力, "束縛":束縛, "未使用記載":未使用})


def 構造を検査(構造):
    if type(構造) is not dict or 構造.get("版") != 数量構造版:
        raise ValueError("数量構造の版不一致")
    if 数量を構成(構造["資料"], 構造["要求"]) != 構造:
        raise ValueError("数量構造と原資料・原要求が不一致")


def _数の上限(数):
    if max(abs(数.numerator).bit_length(), 数.denominator.bit_length()) > 256:
        raise ValueError("数量演算の数値上限")
    return 数


def 数量を評価(構造):
    構造を検査(構造)
    値群, 行群 = {}, []
    節群 = {節["識別子"]:節 for 節 in 構造["節点"]}
    for 節 in 構造["節点"]:
        鍵 = 節["識別子"]
        if 節["種別"] == "数値":
            数, 単位 = 有理を読む(節["値"]), 節["単位"]
        else:
            左, 左単位 = 値群[節["左"]]
            右, 右単位 = 値群[節["右"]]
            演算 = 節["演算"]
            if 演算 in ("加算", "減算"):
                if 節群[節["左"]].get("単位継承"):
                    左単位 = 右単位
                if 左単位 != 右単位:
                    raise ValueError("数量の単位不一致:" + 鍵)
                数, 単位 = (左+右 if 演算 == "加算" else 左-右), 左単位
            else:
                if 演算 == "除算" and 右 == 0:
                    raise ValueError("数量のゼロ除算:" + 鍵)
                数 = 左*右 if 演算 == "乗算" else 左/右
                単位 = dict(左単位)
                for 名, 冪 in 右単位.items():
                    単位[名] = 単位.get(名, 0) + 冪*(1 if 演算 == "乗算" else -1)
                単位 = {名:冪 for 名, 冪 in sorted(単位.items()) if 冪}
                if any(abs(冪)>16 for 冪 in 単位.values()):
                    raise ValueError("数量結果の単位指数上限")
        値群[鍵] = (_数の上限(数), 単位)
        行群.append({"節点":鍵, "値":str(数), "単位":単位})
    結果 = [{**出力, "値":str(値群[出力["節点"]][0]), "単位":値群[出力["節点"]][1]}
            for 出力 in 構造["出力"]]
    操作 = 構造["要求"]["操作"]
    集計 = {}
    if set(操作) & {"比較", "合計", "平均", "最大", "最小"}:
        if any(行["単位"] != 結果[0]["単位"] for 行 in 結果):
            raise ValueError("比較・集計対象の単位が不一致")
        数群 = [Fraction(行["値"]) for 行 in 結果]
        if "比較" in 操作:
            集計["比較"] = [{"左":結果[0]["資料"], "右":行["資料"],
                            "差":str(_数の上限(Fraction(行["値"])-数群[0]))} for 行 in 結果[1:]]
        for 名, 数 in (("合計",sum(数群)), ("平均",sum(数群)/len(数群)),
                      ("最大",max(数群)), ("最小",min(数群))):
            if 名 in 操作: 集計[名] = str(_数の上限(数))
    return 正準({"版":数量構造版, "構造印":指紋(構造), "演算":行群, "結果":結果, "集計":集計})

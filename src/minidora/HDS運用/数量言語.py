"""数量の日本語表現を、出典範囲を持つ再利用可能な式へ構文化する。

有限な文法の全文消費を要求する。未解釈文を捨てて立式成功にはしない。
量の名前は資料から得る。費用等の問題固有の法則・回答は内蔵しない。
"""
from __future__ import annotations
from fractions import Fraction
import re

数量言語版 = "HDS数量言語-v1"
_数 = r"(?:[0-9]+(?:\.[0-9]+)?|\.[0-9]+)"
_単位字 = r"[A-Za-zµμ一-龠々ぁ-ゖァ-ヺ]+"
_単位式 = rf"{_単位字}(?:\^[+-]?[0-9]+)?(?:/{_単位字}(?:\^[+-]?[0-9]+)?)*"
_字 = re.compile(r"[A-Za-z_一-龠々ぁ-ゖァ-ヺ][A-Za-z_0-9一-龠々ぁ-ゖァ-ヺ]*")
_置換 = str.maketrans("０１２３４５６７８９＋－×÷（）％", "0123456789+-*/()%")


def 名前を読む(文):
    if type(文) is not str:
        raise ValueError("数量名は文字列")
    名前 = 文.strip()
    if 名前.startswith("「") and 名前.endswith("」"):
        名前 = 名前[1:-1]
    if not _字.fullmatch(名前) or len(名前) > 40:
        raise ValueError("数量名の範囲不正:" + 名前[:60])
    return 名前


def 単位を読む(文):
    if not 文:
        return {}
    if not re.fullmatch(_単位式, 文):
        raise ValueError("単位表記が未対応:" + 文)
    結果 = {}
    # 単位リテラルは a/b/c。積は数量の四則演算で扱い、自動換算はしない。
    for 印, 名, 冪 in re.findall(rf"([*/]?)({_単位字})(?:\^([+-]?[0-9]+))?", 文):
        指数 = int(冪 or "1") * (-1 if 印 == "/" else 1)
        if abs(指数) > 8:
            raise ValueError("単位指数の上限")
        結果[名] = 結果.get(名, 0) + 指数
    return {名: 値 for 名, 値 in sorted(結果.items()) if 値}


def 有理を読む(値):
    if type(値) is not str or len(値) > 80 or not re.fullmatch(r"[+-]?(?:" + _数 + r"|[0-9]+/[1-9][0-9]*)", 値):
        raise ValueError("有理数表記の範囲不正")
    結果 = Fraction(値)
    if max(abs(結果.numerator).bit_length(), 結果.denominator.bit_length()) > 128:
        raise ValueError("数値の規模上限")
    return 結果


def _位置群(文, 対象):
    深さ = 0
    引用 = False
    結果 = []
    for i, 字 in enumerate(文):
        if 字 == "「":
            if 引用:
                raise ValueError("数量式の引用入れ子は未対応")
            引用 = True
        elif 字 == "」":
            if not 引用:
                raise ValueError("閉じ引用だけの数量式")
            引用 = False
        elif not 引用:
            if 字 == "(":
                深さ += 1
            elif 字 == ")":
                深さ -= 1
                if 深さ < 0:
                    raise ValueError("数量式の括弧不正")
            elif 深さ == 0 and 文.startswith(対象, i):
                結果.append(i)
    if 深さ or 引用:
        raise ValueError("数量式の括弧・引用が閉じていない")
    return 結果


def 式を読む(原文, *, 開始=0):
    if type(原文) is not str or not 原文.strip() or len(原文) > 2048:
        raise ValueError("数量式の長さ・型不正")
    文 = 原文.translate(_置換)
    引用外 = re.sub(r"「[^「」]*」", "", 文)
    if any(語 in 引用外 for 語 in ("ではない", "ではありません", "でない", "なら", "場合", "ただし", "約", "少なくとも", "以下", "未満", "以上", "程度", "およそ", "予定", "かもしれ", "はず", "？", "?")):
        raise ValueError("数量の否定・条件・範囲・近似は未対応。原記載を修正せず保留")
    計数 = 0

    def 構成(種別, 始, 終, **項):
        nonlocal 計数
        計数 += 1
        if 計数 > 128:
            raise ValueError("数量式の節点数上限")
        return {"種別": 種別, "範囲": [開始 + 始, 開始 + 終], **項}

    def 読む(部分, 基点, 深さ=0):
        if 深さ > 24:
            raise ValueError("数量式の深さ上限")
        左空 = len(部分) - len(部分.lstrip())
        部分, 基点 = 部分.strip(), 基点 + 左空
        if not 部分:
            raise ValueError("数量式の項がない")
        _位置群(部分, "\x00")
        # 日本語の演算構文を再帰合成する。演算を含む名前は「名前」で明示する。
        for 末尾, 区切り, 演算 in (("を足した値", "に", "加算"), ("を加えた値", "に", "加算"),
                ("を足す", "に", "加算"), ("を加える", "に", "加算"),
                ("を引いた値", "から", "減算"), ("を引く", "から", "減算"),
                ("で割った値", "を", "除算"), ("で割る", "を", "除算")):
            if 部分.endswith(末尾):
                頭 = 部分[:-len(末尾)]
                位置 = _位置群(頭, 区切り)
                if len(位置) != 1:
                    raise ValueError("日本語数量式の作用域を確定できない。括弧で明示してください")
                i = 位置[0]
                return 構成("演算", 基点, 基点 + len(部分), 演算=演算,
                    左=読む(頭[:i], 基点, 深さ+1), 右=読む(頭[i+len(区切り):], 基点+i+len(区切り), 深さ+1))
        for 末尾, 演算 in (("の和", "加算"), ("の差", "減算"), ("の積", "乗算"), ("の商", "除算")):
            if 部分.endswith(末尾):
                頭 = 部分[:-len(末尾)]
                位置 = _位置群(頭, "と")
                if len(位置) != 1:
                    raise ValueError("二項数量式の項を確定できない")
                i = 位置[0]
                return 構成("演算", 基点, 基点 + len(部分), 演算=演算,
                    左=読む(頭[:i], 基点, 深さ+1), 右=読む(頭[i+1:], 基点+i+1, 深さ+1))
        倍 = re.fullmatch(r"(.+)の(" + _数 + r")(倍|%)", 部分)
        if 倍:
            i = len(倍[1]) + 1
            係数 = 有理を読む(倍[2]) / (100 if 倍[3] == "%" else 1)
            return 構成("演算", 基点, 基点 + len(部分), 演算="乗算", 左=読む(倍[1], 基点, 深さ+1),
                右=構成("数値", 基点+i, 基点+len(部分), 値=str(係数), 単位={}))
        # 括弧の内部では日本語の演算式を再帰的に読める。
        字句 = []
        i = 0
        while i < len(部分):
            if 部分[i].isspace():
                i += 1
                continue
            始 = i
            字 = 部分[i]
            if 字 == "(":
                水準, 引用 = 1, False
                i += 1
                while i < len(部分) and 水準:
                    if 部分[i] == "「": 引用 = True
                    elif 部分[i] == "」": 引用 = False
                    elif not 引用 and 部分[i] == "(": 水準 += 1
                    elif not 引用 and 部分[i] == ")": 水準 -= 1
                    i += 1
                if 水準:
                    raise ValueError("数量式の括弧不足")
                字句.append(("項", 読む(部分[始+1:i-1], 基点+始+1, 深さ+1)))
                continue
            if 字 in "+-*/":
                字句.append((字, None)); i += 1; continue
            if 字 == "「":
                終 = 部分.find("」", i+1)
                if 終 < 0:
                    raise ValueError("数量名の閉じ引用不足")
                名前 = 名前を読む(部分[i:終+1]); i = 終+1
                字句.append(("項", 構成("参照", 基点+始, 基点+i, 名前=名前))); continue
            数 = re.match(_数, 部分[i:])
            if 数:
                i += len(数[0])
                単位, 値 = {}, 有理を読む(数[0])
                単位一致 = re.match(_単位式, 部分[i:])
                if 単位一致:
                    単位 = 単位を読む(単位一致[0]); i += len(単位一致[0])
                elif i < len(部分) and 部分[i] == "%":
                    値 /= 100; i += 1
                字句.append(("項", 構成("数値", 基点+始, 基点+i, 値=str(値), 単位=単位))); continue
            名 = _字.match(部分, i)
            if 名:
                i = 名.end()
                字句.append(("項", 構成("参照", 基点+始, 基点+i, 名前=名前を読む(名[0])))); continue
            raise ValueError("未対応の数量式:" + 部分[i:i+32])
        if len(字句) > 256:
            raise ValueError("数量式の字句数上限")
        位置 = 0
        def 項を読む():
            nonlocal 位置
            if 位置 >= len(字句):
                raise ValueError("数量式の項不足")
            種, 値 = 字句[位置]; 位置 += 1
            if 種 in ("+", "-"):
                右 = 項を読む()
                if 種 == "+": return 右
                return 構成("演算", 基点, 基点+len(部分), 演算="減算",
                    左=構成("数値", 基点, 基点, 値="0", 単位={}, 単位継承=True), 右=右)
            if 種 != "項":
                raise ValueError("数量式の演算位置不正")
            return 値
        def 二項(最低):
            nonlocal 位置
            左 = 項を読む()
            優先 = {"+": 1, "-": 1, "*": 2, "/": 2}
            while 位置 < len(字句) and 字句[位置][0] in 優先 and 優先[字句[位置][0]] >= 最低:
                演算 = 字句[位置][0]; 位置 += 1
                右 = 二項(優先[演算]+1)
                左 = 構成("演算", 左["範囲"][0]-開始, 右["範囲"][1]-開始,
                    演算={"+":"加算", "-":"減算", "*":"乗算", "/":"除算"}[演算], 左=左, 右=右)
            return 左
        結果 = 二項(1)
        if 位置 != len(字句):
            raise ValueError("数量式に未消費の記載がある")
        return 結果
    return 読む(文, 0)


def 参照名(式):
    if 式["種別"] == "参照":
        return {式["名前"]}
    if 式["種別"] == "数値":
        return set()
    return 参照名(式["左"]) | 参照名(式["右"])


def 定義を読む(本文):
    if type(本文) is not str or not 本文.strip() or len(本文) > 16000 or "\x00" in 本文:
        raise ValueError("数量資料の型・長さ不正")
    # 小数点は区切らない。括弧/引用内部の記号も区切らない。
    変換 = 本文.translate(_置換)
    分割 = sorted(set(i for 区切り in ("。", "\n", ";", "；", "、") for i in _位置群(変換, 区切り)))
    境界 = [-1, *分割, len(本文)]
    結果 = {}
    for 左, 右 in zip(境界, 境界[1:]):
        片 = 変換[左+1:右]
        始 = 左+1 + len(片)-len(片.lstrip())
        文 = 片.strip()
        if not 文: continue
        if len(結果) >= 48:
            raise ValueError("数量資料の定義数上限")
        一致 = re.fullmatch(r'(?P<名>「[^「」]+」|[^=]+?)(?:とは|は|=)\s*(?P<式>.+?)(?:です|である|とする)?', 文)
        if not 一致:
            raise ValueError(f"未解釈の数量記載:{始}:{文[:60]}")
        名 = 名前を読む(一致["名"])
        if 名 in 結果:
            raise ValueError("数量定義の重複:" + 名)
        式始 = 始 + 一致.start("式")
        結果[名] = {"名前": 名, "式": 式を読む(一致["式"], 開始=式始),
                    "範囲": [始, 始+len(文)], "原文": 本文[始:始+len(文)]}
    if not 結果:
        raise ValueError("数量定義がない")
    return 結果

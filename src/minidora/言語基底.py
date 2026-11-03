from __future__ import annotations

from dataclasses import dataclass
import re
import unicodedata

from .言語基底_英語 import (
    英語基本形 as _英語基本形,
    英語関係概念 as _英語関係概念,
    英語関係族 as _英語関係族,
    英語明示関係構文,
)


言語基底版 = "v0.4"
_日本語規定名 = "日本語"
_日本語外部互換コード = "ja"


@dataclass(frozen=True, slots=True)
class 文字知識:
    文字: str
    体系: str
    役割: str
    読み: str = ""
    ローマ字: str = ""


@dataclass(frozen=True, slots=True)
class 語彙知識:
    表記: str
    言語: str
    区分: str
    基本義: tuple[str, ...]
    読み: tuple[str, ...] = ()
    対応語: tuple[str, ...] = ()


_ひらがなローマ字 = {
    "あ": "a", "い": "i", "う": "u", "え": "e", "お": "o",
    "か": "ka", "き": "ki", "く": "ku", "け": "ke", "こ": "ko",
    "さ": "sa", "し": "shi", "す": "su", "せ": "se", "そ": "so",
    "た": "ta", "ち": "chi", "つ": "tsu", "て": "te", "と": "to",
    "な": "na", "に": "ni", "ぬ": "nu", "ね": "ne", "の": "no",
    "は": "ha", "ひ": "hi", "ふ": "fu", "へ": "he", "ほ": "ho",
    "ま": "ma", "み": "mi", "む": "mu", "め": "me", "も": "mo",
    "や": "ya", "ゆ": "yu", "よ": "yo",
    "ら": "ra", "り": "ri", "る": "ru", "れ": "re", "ろ": "ro",
    "わ": "wa", "ゐ": "wi", "ゑ": "we", "を": "wo", "ん": "n",
    "が": "ga", "ぎ": "gi", "ぐ": "gu", "げ": "ge", "ご": "go",
    "ざ": "za", "じ": "ji", "ず": "zu", "ぜ": "ze", "ぞ": "zo",
    "だ": "da", "ぢ": "ji", "づ": "zu", "で": "de", "ど": "do",
    "ば": "ba", "び": "bi", "ぶ": "bu", "べ": "be", "ぼ": "bo",
    "ぱ": "pa", "ぴ": "pi", "ぷ": "pu", "ぺ": "pe", "ぽ": "po",
    "ゔ": "vu",
    "ぁ": "a", "ぃ": "i", "ぅ": "u", "ぇ": "e", "ぉ": "o",
    "ゃ": "ya", "ゅ": "yu", "ょ": "yo", "っ": "tsu", "ゎ": "wa",
    "ゕ": "ka", "ゖ": "ke",
}


def _ひらがなをカタカナ(char: str) -> str:
    code = ord(char)
    if 0x3041 <= code <= 0x3096:
        return chr(code + 0x60)
    return char


_カタカナローマ字 = {_ひらがなをカタカナ(k): v for k, v in _ひらがなローマ字.items()}
_カタカナローマ字["ー"] = ""


_日本語基底語彙 = {
    "状態": 語彙知識("状態", "ja", "基底概念", ("対象がある時点で持つあり方",), ("じょうたい",), ('状態',)),
    "現在": 語彙知識("現在", "ja", "時間", ("今この時点",), ("げんざい",), ("current", "present")),
    "過去": 語彙知識("過去", "ja", "時間", ("現在より前の時点または期間",), ("かこ",), ("past",)),
    "保持": 語彙知識("保持", "ja", "作用", ("状態や情報を失わず持つ",), ("ほじ",), ("retain", "preserve")),
    "更新": 語彙知識("更新", "ja", "作用", ("現在状態を新しい状態へ改める",), ("こうしん",), ("update",)),
    "参照": 語彙知識("参照", "ja", "作用", ("対象を根拠または入力として見る",), ("さんしょう",), ('参照', "refer")),
    "取得": 語彙知識("取得", "ja", "作用", ("対象を取り込む",), ("しゅとく",), ("retrieve", "acquire")),
    "抽出": 語彙知識("抽出", "ja", "作用", ("集合や表現から対象部分を取り出す",), ("ちゅうしゅつ",), ("extract",)),
    "関係": 語彙知識("関係", "ja", "基底概念", ("複数対象の間に成立する結びつき",), ("かんけい",), ('関係',)),
    "包含": 語彙知識("包含", "ja", "関係", ("ある対象が別の対象を内に含む関係",), ("ほうがん",), ("contain", "include")),
    "同値": 語彙知識("同値", "ja", "関係", ("定めた評価軸で同じ値または意味として扱える関係",), ("どうち",), ("equivalent",)),
    "順序": 語彙知識("順序", "ja", "関係", ("前後または順位の関係",), ("じゅんじょ",), ("order",)),
    "因果": 語彙知識("因果", "ja", "関係", ("原因と結果として扱う関係",), ("いんが",), ("cause", "causal")),
    "条件": 語彙知識("条件", "ja", "関係", ("成立や作用に必要または制約となる事項",), ("じょうけん",), ("condition",)),
    "境界": 語彙知識("境界", "ja", "基底概念", ("対象・作用・適用範囲を分ける限界",), ("きょうかい",), ('境界',)),
    "依存": 語彙知識("依存", "ja", "関係", ("一方の成立が他方に条件づけられる関係",), ("いぞん",), ("depend",)),
    "反対": 語彙知識("反対", "ja", "関係", ("定めた軸で向きまたは内容が対立する関係",), ("はんたい",), ("opposite",)),
    "変換": 語彙知識("変換", "ja", "作用", ("表現または状態を別の形へ移す",), ("へんかん",), ("transform", "convert")),
    "結合": 語彙知識("結合", "ja", "作用", ("複数対象を関係づけて一つの構成へする",), ("けつごう",), ("combine", "bind")),
    "分離": 語彙知識("分離", "ja", "作用", ("混在した対象または責務を分ける",), ("ぶんり",), ("separate",)),
    "比較": 語彙知識("比較", "ja", "作用", ("複数対象を共通の評価軸で照合する",), ("ひかく",), ('比較',)),
    "検証": 語彙知識("検証", "ja", "作用", ("主張・状態・結果を根拠に照らして確認する",), ("けんしょう",), ("verify", "validate")),
    "停止": 語彙知識("停止", "ja", "作用", ("現在の処理または循環を止める",), ("ていし",), ("stop",)),
    "合格": 語彙知識("合格", "ja", "採否", ("現在の評価条件を満たす状態",), ("ごうかく",), ("pass", "approve")),
    "保留": 語彙知識("保留", "ja", "採否", ("現在は確定せず状態を保持する",), ("ほりゅう",), ("suspend", "hold")),
    "失敗": 語彙知識("失敗", "ja", "採否", ("要求された成立条件を満たせなかった状態",), ("しっぱい",), ("fail", "failure")),
}


_外部英語文法機能 = {
    "a": "不定冠詞", "an": "不定冠詞", "the": "定冠詞", "and": "並列", "or": "選択",
    "not": "否定", "no": "否定", "if": "条件", "unless": "否定条件", "when": "時条件",
    "before": "時間順序", "after": "時間順序", "because": "因果接続", "why": "理由疑問",
    "what": "対象疑問", "which": "選択疑問", "who": "人物疑問", "where": "場所疑問", "how": "方法・状態疑問",
    "is": "be動詞", "are": "be動詞", "was": "be動詞・過去", "were": "be動詞・過去",
    "do": "助動的作用", "does": "助動的作用", "did": "助動的作用・過去",
    "can": "可能", "could": "可能・過去または仮定", "may": "可能性", "must": "義務・強制",
}


_日本語文法機能 = {
    "は": "主題", "が": "主格・焦点", "を": "対象格", "に": "到達・時点・対象", "へ": "方向",
    "で": "場所・手段", "と": "並列・引用・共同", "の": "所属・修飾", "から": "起点・原因", "まで": "終点・範囲",
    "より": "比較・起点", "ない": "否定", "です": "丁寧断定", "ます": "丁寧叙述",
}


_中国語特徴字 = frozenset("的了是在有和与吗呢这那它给总为于将把被从个们")
_ラテン字 = re.compile(r"[A-Za-z]")
_かな = re.compile(r"[ぁ-んァ-ヶー]")
_漢字 = re.compile(r"[一-龥々]")


class 言語基底P:
    'HDS 構文化器とMINIDORA実行系が共有する最小言語基底知識。\n\n    日本語を唯一の規定・基底・内部意味正本とする。\n    外国語資産は外部互換のために必要な範囲だけ保持し、内部正本へ昇格させない。\n    百科事典的な世界知識はここへ入れない。\n    '

    版 = 言語基底版
    規定言語 = _日本語規定名
    基底言語 = _日本語規定名
    基底言語コード = _日本語外部互換コード

    def 文字知識(self, char: str) -> 文字知識:
        if len(char) != 1:
            raise ValueError("文字知識は1文字単位で取得する")
        if char in _ひらがなローマ字:
            return 文字知識(char, "ひらがな", "表音文字", char, _ひらがなローマ字[char])
        if char in _カタカナローマ字:
            reading = chr(ord(char) - 0x60) if char != "ー" and 0x30A1 <= ord(char) <= 0x30F6 else ""
            return 文字知識(char, "カタカナ", "表音文字", reading, _カタカナローマ字[char])
        if "A" <= char <= "Z" or "a" <= char <= "z":
            return 文字知識(char, "ラテン文字", "表音・外部互換記号", char.casefold(), char.casefold())
        if _漢字.fullmatch(char):
            return 文字知識(char, "漢字", "表意文字")
        if char.isdigit():
            return 文字知識(char, "数字", "数量記号", char, char)
        category = unicodedata.category(char)
        return 文字知識(char, "記号" if category.startswith(("P", "S")) else "その他", "補助記号")

    def 文字列知識(self, text: str) -> tuple[文字知識, ...]:
        normalized = unicodedata.normalize("NFKC", str(text))
        return tuple(self.文字知識(char) for char in normalized if not char.isspace())

    def 語彙知識(self, word: str) -> 語彙知識 | None:
        value = unicodedata.normalize("NFKC", str(word)).strip()
        if value in _日本語基底語彙:
            return _日本語基底語彙[value]
        lowered = value.casefold()
        if lowered in _外部英語文法機能:
            return 語彙知識(value, "en", "文法機能", (_外部英語文法機能[lowered],))
        if value in _日本語文法機能:
            return 語彙知識(value, "ja", "文法機能", (_日本語文法機能[value],))
        関係 = _英語関係概念(value)
        if 関係 is not None:
            return 語彙知識(value, "en", "関係語", (関係,), 対応語=(関係,))
        return None

    def 文法機能(self, word: str) -> str | None:
        value = unicodedata.normalize("NFKC", str(word)).strip()
        if value in _日本語文法機能:
            return _日本語文法機能[value]
        return _外部英語文法機能.get(value.casefold())

    def 英語基本形(self, word: str) -> str:
        return _英語基本形(word)

    def 英語関係概念(self, word: str) -> str | None:
        return _英語関係概念(word)

    def 英語関係族(self) -> dict[str, frozenset[str]]:
        return _英語関係族()

    def 英語関係構文(self):
        return 英語明示関係構文

    def 入力言語判定(self, text: str) -> str:
        """外部互換用の言語識別コードを返す。内部意味正本を決める関数ではない。"""
        value = unicodedata.normalize("NFKC", str(text))
        kana = len(_かな.findall(value))
        latin = len(_ラテン字.findall(value))
        han_chars = _漢字.findall(value)
        if kana:
            return "ja"
        if han_chars and not latin:
            if any(char in _中国語特徴字 for char in han_chars):
                return "zh"
            return "ja"
        if latin:
            return "en"
        return "ja"

    def 入力言語名(self, text: str) -> str:
        return {"ja": "日本語", "en": "英語", "zh": "中国語"}.get(self.入力言語判定(text), "外部言語")

    def 統計(self) -> dict[str, int | str]:
        families = self.英語関係族()
        return {
            "版": self.版,
            "規定言語": self.規定言語,
            "基底言語": self.基底言語,
            "基底言語コード": self.基底言語コード,
            "ひらがな": len(_ひらがなローマ字),
            "カタカナ": len(_カタカナローマ字),
            "日本語基底語彙": len(_日本語基底語彙),
            "日本語文法機能": len(_日本語文法機能),
            "外部英語文法機能": len(_外部英語文法機能),
            "外部英語関係族": len(families),
            "外部英語関係基本形": sum(len(words) for words in families.values()),
        }


標準言語基底P = 言語基底P()


__all__ = ["言語基底版", "文字知識", "語彙知識", "言語基底P", "標準言語基底P"]

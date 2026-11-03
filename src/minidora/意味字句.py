from __future__ import annotations

import re
import unicodedata

from .言語基底 import 標準言語基底P
from .言語基底_英語 import 英語基本形 as _英語基底基本形, 英語関係概念


_WORD = re.compile(r"[A-Za-z0-9_+\-\.]+|[Α-Ωα-ωϐ-Ͽ]+|[ぁ-んァ-ヶー]+|[一-龥々]+")
_MATH_NUMBER = r"[-+]?(?:\d+(?:\.\d+)?|\.\d+)(?:[eE][-+]?\d+)?"
_MATH_ANCHOR = re.compile(
    rf"(?<![A-Za-z0-9_]){_MATH_NUMBER}(?:\s*(?:/|\^|\*|×)\s*{_MATH_NUMBER})?(?![A-Za-z0-9_])"
)
_LATEX_FRAC = re.compile(
    rf"\\frac\s*\{{\s*({_MATH_NUMBER})\s*\}}\s*\{{\s*({_MATH_NUMBER})\s*\}}"
)
_SQRT_ANCHOR = re.compile(
    rf"(?:\\sqrt|sqrt)\s*[\{{\(]\s*({_MATH_NUMBER})\s*[\}}\)]",
    flags=re.I,
)
_ENUM_ATOM = re.compile(r"(?<![A-Za-z0-9_])([A-Za-zΑ-Ωα-ωϐ-Ͽ])\s*[\)\].:]")
_MATH_VAR_LEFT = re.compile(r"(?<![A-Za-z0-9_])([A-Za-zΑ-Ωα-ωϐ-Ͽ])(?=\s*(?:=|[+\-*/^<>≤≥]))")
_MATH_VAR_RIGHT = re.compile(r"(?:=|[+\-*/^<>≤≥])\s*([A-Za-zΑ-Ωα-ωϐ-Ͽ])(?![A-Za-z0-9_])")

_STOP = {
    "the", "a", "an", "of", "to", "in", "on", "at", "for", "from", "with", "and", "or",
    "is", "are", "was", "were", "be", "been", "being", "which", "what", "who", "when", "where",
    "why", "how", "this", "that", "these", "those", "it", "its", "as", "by", "than", "then",
    "do", "does", "did", "have", "has", "had", "will", "shall", "would", "could", "should",
    "may", "might", "can", "about", "into", "through", "during", "after", "before", "between", "among",
    "following", "statement", "statements", "answer", "answers", "option", "options", "choice", "choices", '選択肢',
    "correct", "incorrect", "true", "false", "most", "least", "likely", "unlikely", "best", "except",
    "select", "choose", "chosen", "consider", "considered", "describe", "describes", "described",
    "regarding", "according", "given", "respect", "respectively", "not",
}


def _english_stem(value: str) -> str:
    """意味照合用の保守的な英語表層正規化。"""
    if not re.fullmatch(r"[a-z]+", value):
        return value
    lexical = _英語基底基本形(value)
    if lexical != value:
        return lexical
    if value in {"species", "series"}:
        return value
    if len(value) > 4 and value.endswith("ies"):
        return value[:-3] + "y"
    if len(value) > 5 and value.endswith("sses"):
        return value[:-2]
    # characterize/organize等の -ize/-ise 系は -es ではなく3単現の -s だけを落とす。
    if len(value) > 5 and value.endswith(("izes", "ises")):
        return value[:-1]
    if len(value) > 5 and value.endswith(("xes", "zes", "ches", "shes")):
        return value[:-2]
    if len(value) > 4 and value.endswith("ing"):
        stem = value[:-3]
        if len(stem) >= 2 and stem[-1] == stem[-2] and stem[-1] not in "aeiou":
            stem = stem[:-1]
        if stem.endswith("us"):
            return stem + "e"
        return stem
    if len(value) > 4 and value.endswith("ied"):
        return value[:-3] + "y"
    if len(value) > 4 and value.endswith("ed"):
        stem = value[:-2]
        if len(stem) >= 2 and stem[-1] == stem[-2] and stem[-1] not in "aeiou":
            stem = stem[:-1]
        return stem
    if len(value) > 3 and value.endswith("s") and not value.endswith(("ss", "is", "us")):
        return value[:-1]
    return value


def _数値語(value: str) -> bool:
    return re.fullmatch(_MATH_NUMBER, value) is not None


def _記号語(value: str) -> str:
    return "sym:" + value.casefold()


def _数式anchor(raw: str) -> set[str]:
    out: set[str] = set()
    for numerator, denominator in _LATEX_FRAC.findall(raw):
        out.add(f"math:{numerator}/{denominator}".casefold())
    for value in _SQRT_ANCHOR.findall(raw):
        out.add(f"math:sqrt({value})".casefold())
    for anchor in _MATH_ANCHOR.findall(raw):
        compact = re.sub(r"\s+", "", anchor).replace("×", "*")
        if compact:
            out.add("math:" + compact.casefold())
    return out


def 意味語(text: object) -> frozenset[str]:
    """HDS意味照合で共有する正規化語集合を返す。"""
    raw = unicodedata.normalize("NFKC", str(text))
    out: set[str] = _数式anchor(raw)

    stripped = raw.strip()
    if re.fullmatch(r"[A-Za-zΑ-Ωα-ωϐ-Ͽ]", stripped):
        out.add("atom:" + stripped.casefold())

    for atom in _ENUM_ATOM.findall(raw):
        out.add("atom:" + atom.casefold())

    for variable in (*_MATH_VAR_LEFT.findall(raw), *_MATH_VAR_RIGHT.findall(raw)):
        out.add(_記号語(variable))

    for 字句 in _WORD.findall(raw):
        original = 字句
        value = 字句.casefold().strip("._")
        if _数値語(value):
            out.add(value)
            continue

        if len(original) == 1 and (
            original.isupper()
            or re.fullmatch(r"[Α-Ωα-ωϐ-Ͽ]", original) is not None
        ):
            out.add(_記号語(original))
            continue

        if len(original) == 1 and 標準言語基底P.文字知識(original).体系 == "漢字":
            out.add(original)
            continue

        if 標準言語基底P.文法機能(original) is not None:
            continue

        value = value.strip("-")
        if len(value) <= 1 or value in _STOP:
            continue
        normalized = _english_stem(value)
        if len(normalized) <= 1 or normalized in _STOP:
            continue
        out.add(normalized)
        関係 = 英語関係概念(normalized)
        if 関係 is not None:
            out.add("rel:" + 関係)
    return frozenset(out)


__all__ = ["意味語"]

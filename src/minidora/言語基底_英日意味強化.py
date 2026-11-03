from __future__ import annotations

from dataclasses import dataclass, replace
import re
import unicodedata

from .言語基底_英日意味 import (
    英語質問境界,
    英語質問境界解析,
    英日意味制御,
    英日関係質問,
    英日意味フレーム,
    英日意味フレーム抽出 as _旧抽出,
)
from .言語基底_英語 import 英語基本形, 英語関係概念


@dataclass(frozen=True, slots=True)
class 英語明示述語関係:
    種別: str
    始点: str
    終点: str
    検索述語: str
    極性: str = "肯定"
    修飾: tuple[tuple[str, str], ...] = ()


_末尾疑問符 = re.compile(r"[?？]+$")
_文分割 = re.compile(r"(?<=[?!.。？！])\s+|\n+")
_語 = re.compile(r"[A-Za-z][A-Za-z0-9_-]*")
_補助語 = frozenset({
    "is", "are", "was", "were", "be", "been", "being", "do", "does", "did",
    "can", "could", "may", "might", "must", "should", "would", "will", "has", "have", "had",
})
_機能語 = frozenset({
    "which", "what", "who", "where", "when", "why", "how", "of", "the", "following",
    "most", "least", "likely", "unlikely", "best", "correctly", "accurately", "directly",
    "and", "or", "nor",
})
_前置詞 = frozenset({"to", "in", "on", "with", "against", "from", "of", "for", "by", "as", "into", "onto", "through"})
_判定反転 = re.compile(r"\b(?:least\s+likely|unlikely|except|incorrect|false|inaccurate|invalid|inconsistent|unsupported|not\s+true|not\s+correct)\b", re.I)
_否定 = re.compile(r"\b(?:never|not|no|without|cannot|can't|does\s+not|do\s+not|did\s+not|is\s+not|are\s+not|was\s+not|were\s+not)\b", re.I)
_先頭条件 = re.compile(r"^(?P<c>(?:(?:if|when|under|given|assuming|unless)\b|in\s+the\s+(?:presence|absence)\s+of\b)[^?]{1,220}?)[,;]\s*(?P<q>.+)$", re.I)
_末尾条件 = re.compile(r"\b(?P<c>(?:when|if|unless|under|given|assuming)\b.+)$", re.I)
_命題選択 = re.compile(
    r"^(?:which|what)(?:\s+of\s+the\s+following)?(?:\s+(?:statement|statements|claim|claims|description|descriptions|explanation|explanations))?\s+"
    r"(?:(?:is|are)\s+)?(?:(?:most|least)\s+likely\s+)?"
    r"(?P<judge>correct|incorrect|true|false|accurate|inaccurate|valid|invalid|consistent|inconsistent|supported|unsupported)"
    r"(?:\s+(?:regarding|about|for|with\s+respect\s+to)\s+(?P<topic>.+))?$", re.I,
)
_説明選択 = re.compile(
    r"^(?:which|what)(?:\s+of\s+the\s+following)?(?:\s+(?:statement|statements|option|options|choice|choices|explanation|explanations))?\s+"
    r"(?:(?:best|most\s+directly|most\s+accurately)\s+)?(?P<v>describes?|explains?|accounts?\s+for|characterizes?|represents?)\s+(?P<o>.+)$", re.I,
)
_数量 = re.compile(r"^how\s+(?:many|much)\s+(?P<o>.+)$", re.I)
_一般同定 = re.compile(r"^(?:what|which)\s+(?:(?:would|could|may|might|should|can|will|must)\s+)?(?:be\s+)?(?P<o>.+)$", re.I)
_様相コピュラ同定 = re.compile(
    r"^(?:what|which)\s+(?:would|could|may|might|should|can|will|must)\s+be\b", re.I,
)
_協調主語 = re.compile(r"\b(?:and|or|nor)\b", re.I)
_協調接続終端 = re.compile(r"\b(?:and|or|nor)\s*$", re.I)
_末尾補助語 = re.compile(
    r"\s+(?:do|does|did|is|are|was|were|be|been|being|can|could|may|might|must|should|would|will|has|have|had)\s*$",
    re.I,
)
_受動 = re.compile(
    r"^(?P<s>.+?)\s+(?:is|are|was|were|be|been|being|has\s+been|have\s+been|had\s+been)\s+"
    r"(?P<v>[A-Za-z][A-Za-z-]*(?:\s+(?:to|in|on|with|against|from|of|for))?)\s+by\s+(?P<o>.+)$", re.I,
)


def _正規化(text: object) -> str:
    return " ".join(unicodedata.normalize("NFKC", str(text)).split()).strip()


def _端点(text: object) -> str:
    return _末尾疑問符.sub("", _正規化(text)).strip(" ,;:()[]")


def _開放主語(text: object) -> str:
    """open述語探索が主語側へ吸収した補助語だけを除く。世界知識は足さない。"""
    return _末尾補助語.sub("", _端点(text)).strip()


def _動詞基本形(surface: str) -> str:
    phrase = _正規化(surface).casefold()
    parts = phrase.split()
    if not parts:
        return ""
    word = parts[0]
    known = 英語基本形(word)
    if known != word:
        head = known
    elif len(word) > 4 and word.endswith("ies"):
        head = word[:-3] + "y"
    elif len(word) > 4 and word.endswith(("izes", "ises")):
        head = word[:-1]
    elif len(word) > 4 and word.endswith(("ches", "shes", "xes", "sses", "oes")):
        head = word[:-2]
    elif len(word) > 3 and word.endswith("s") and not word.endswith(("ss", "is", "us")):
        head = word[:-1]
    elif len(word) > 5 and word.endswith(("ized", "ised")):
        head = word[:-1]
    elif len(word) > 4 and word.endswith("ied"):
        head = word[:-3] + "y"
    elif len(word) > 4 and word.endswith("ed"):
        head = word[:-2]
        if head in {"stabiliz", "activat", "regulat", "generat", "characteriz", "represent", "associat"}:
            head += "e"
    elif len(word) > 5 and word.endswith("ing"):
        head = word[:-3]
        if head.endswith(("iz", "at")):
            head += "e"
    else:
        head = word
    return " ".join((head, *parts[1:]))


def _関係種別(predicate: str) -> tuple[str, str]:
    normalized = _動詞基本形(predicate)
    head = normalized.split()[0] if normalized else ""
    kind = 英語関係概念(head)
    return (kind or "開放述語", normalized)


def _修飾(text: str, conditions: tuple[str, ...] = ()) -> tuple[tuple[str, str], ...]:
    out: list[tuple[str, str]] = []
    if _否定.search(text):
        out.append(("極性", "否定"))
    low = text.casefold()
    if re.search(r"\b(?:can|could|may|might|would|should)\b", low):
        out.append(("様相", "可能"))
    if re.search(r"\b(?:must|necessary)\b", low):
        out.append(("様相", "必要"))
    if re.search(r"\b(?:all|each|every)\b", low):
        out.append(("量化", "全称"))
    for condition in conditions:
        if condition:
            out.append(('条件範囲', condition))
    return tuple(dict.fromkeys(out))


def _条件分離(text: str) -> tuple[str, tuple[str, ...]]:
    raw = _端点(text)
    first = _先頭条件.fullmatch(raw)
    if first:
        return _正規化(first.group("q")), (_正規化(first.group("c")),)
    tail = _末尾条件.search(raw)
    if tail and tail.start() > 0:
        return raw[:tail.start()].strip(" ,;:"), (_正規化(tail.group("c")),)
    return raw, ()


def _反転(text: str) -> bool:
    return bool(_判定反転.search(text))


def _predicate_from_字句(字句列: list[str], start: int) -> tuple[str, list[str]] | None:
    for index in range(start, len(字句列)):
        字句 = 字句列[index].casefold().strip("?,.;:")
        if 字句 in _補助語 or 字句 in _機能語 or 字句 == "never":
            continue
        if 英語関係概念(字句) is not None or re.fullmatch(r"[a-z][a-z-]{2,}", 字句):
            predicate = [字句列[index]]
            if index + 1 < len(字句列) and 字句列[index + 1].casefold().strip("?,.;:") in _前置詞:
                predicate.append(字句列[index + 1])
                return " ".join(predicate), 字句列[index + 2:]
            return " ".join(predicate), 字句列[index + 1:]
    return None


def _一般関係質問(raw: str, conditions: tuple[str, ...]) -> 英日関係質問 | None:
    body = _端点(raw)
    字句 = body.split()
    if not 字句 or 字句[0].casefold() not in {"which", "what", "who"}:
        return None
    if len(字句) == 1:
        return None

    aux_index = next((i for i, t in enumerate(字句) if t.casefold() in {"does", "do", "did"}), None)
    if aux_index is not None and aux_index >= 1:
        requested = " ".join(字句[1:aux_index]).replace("of the following", "").strip() or "選択肢"
        predicate_info = _predicate_from_字句(字句, aux_index + 1)
        if predicate_info:
            predicate_surface, tail = predicate_info
            関係_index = body.casefold().rfind(predicate_surface.casefold())
            主体 = body[len(" ".join(字句[:aux_index + 1])):関係_index].strip() if 関係_index >= 0 else " ".join(字句[aux_index + 1:-1])
            kind, predicate = _関係種別(predicate_surface)
            return 英日関係質問(kind, "終点", requested, _端点(主体), predicate, _反転(body), False, _修飾(body, conditions))

    prefix = 1
    if len(字句) >= 4 and [t.casefold() for t in 字句[1:4]] == ["of", "the", "following"]:
        requested = "選択肢"
        prefix = 4
    else:
        requested_字句: list[str] = []
        while prefix < len(字句):
            low = 字句[prefix].casefold().strip("?,.;:")
            if low in _補助語 or low in {"never", "not", "most", "least", "likely", "unlikely"}:
                break
            if prefix + 1 < len(字句):
                next_low = 字句[prefix + 1].casefold().strip("?,.;:")
                if 英語関係概念(next_low) is not None or next_low.endswith(("s", "ed", "ing")):
                    requested_字句.append(字句[prefix])
                    prefix += 1
                    break
            requested_字句.append(字句[prefix])
            prefix += 1
        requested = " ".join(requested_字句).strip() or "選択肢"

    predicate_info = _predicate_from_字句(字句, prefix)
    if not predicate_info:
        return 英日関係質問("問い適合", "始点", requested, body, "match", _反転(body), False, _修飾(body, conditions))
    predicate_surface, tail = predicate_info
    kind, predicate = _関係種別(predicate_surface)
    known = _端点(" ".join(tail)) or body
    return 英日関係質問(kind, "始点", requested, known, predicate, _反転(body), False, _修飾(body, conditions))


def _代替質問(focus: str, 境界: 英語質問境界 | None = None) -> 英日関係質問 | None:
    境界 = 境界 if 境界 is not None else 英語質問境界解析(focus)
    if not 境界.質問表示:
        return None
    body, conditions = 境界.本体, 境界.条件範囲
    if 境界.境界状態 == "括弧境界未確定":
        return 英日関係質問("問い適合", "始点", "選択肢", body, "match", _反転(body), False, _修飾(body))
    proposition = _命題選択.fullmatch(body)
    if proposition:
        topic = _端点(proposition.group("topic") or "候補命題")
        return 英日関係質問("命題適合", "始点", "選択肢", topic, "proposition_match", _反転(body), False, _修飾(body, conditions))
    explanation = _説明選択.fullmatch(body)
    if explanation:
        predicate = _動詞基本形(explanation.group("v"))
        return 英日関係質問("説明適合", "始点", "選択肢", _端点(explanation.group("o")), predicate, _反転(body), False, _修飾(body, conditions))
    quantity = _数量.fullmatch(body)
    if quantity:
        return 英日関係質問("数量同定", "終点", "数量", _端点(quantity.group("o")), "count", _反転(body), False, _修飾(body, conditions))
    # "What would be ..." 等は内容語をopen述語と誤認する前に同定要求として閉じる。
    if _様相コピュラ同定.match(body):
        identity = _一般同定.fullmatch(body)
        if identity:
            target = _端点(identity.group("o"))
            return 英日関係質問("同定", "終点", "未特定", target, "identify", _反転(body), False, _修飾(body, conditions))
    generic = _一般関係質問(body, conditions)
    if generic:
        return generic
    identity = _一般同定.fullmatch(body)
    if identity:
        target = _端点(identity.group("o"))
        return 英日関係質問("同定", "終点", "未特定", target, "identify", _反転(body), False, _修飾(body, conditions))

    # 既知構文へ閉じられなくても、質問として明示された意味内容はtopic bagへ捨てない。
    # 世界事実を補わず、質問表層そのものを既知端点にした「問い適合」として保持する。
    content_字句 = [
        字句 for 字句 in _語.findall(body)
        if 字句.casefold() not in _機能語 and 字句.casefold() not in _補助語
    ]
    if content_字句:
        return 英日関係質問(
            "問い適合", "始点", "選択肢", body, "match", _反転(body), False, _修飾(body, conditions),
        )
    return None


def 英日意味フレーム抽出(text: str) -> 英日意味フレーム:
    境界 = 英語質問境界解析(text)
    focus = 境界.焦点
    proposition = _命題選択.fullmatch(境界.本体)
    代替経路 = _代替質問(focus, 境界)
    if proposition is not None and 代替経路 is not None:
        base = _旧抽出(text)
        canonical = tuple((*base.正本意味, f"関係:{代替経路.種別}", f"述語:{代替経路.検索述語}", f"不足位置:{代替経路.未知位置}"))
        return 英日意味フレーム(tuple(dict.fromkeys(canonical)), base.外部検索語, base.制御, 代替経路)

    base = _旧抽出(text)
    if base.関係質問 is not None:
        conditions = 境界.条件範囲
        modifiers = tuple(dict.fromkeys((*base.関係質問.修飾, *_修飾(focus, conditions))))
        return replace(base, 関係質問=replace(base.関係質問, 修飾=modifiers))
    if 代替経路 is None:
        return base
    canonical = tuple((*base.正本意味, f"関係:{代替経路.種別}", f"述語:{代替経路.検索述語}", f"不足位置:{代替経路.未知位置}"))
    return 英日意味フレーム(tuple(dict.fromkeys(canonical)), base.外部検索語, base.制御, 代替経路)


def _declaration_conditions(text: str) -> tuple[str, tuple[tuple[str, str], ...]]:
    head, conditions = _条件分離(text)
    return head, _修飾(text, conditions)


def _declaration_one(sentence: str) -> 英語明示述語関係 | None:
    raw = _端点(sentence)
    if not raw or re.match(r"^(?:which|what|who|where|when|why|how)\b", raw, re.I):
        return None
    極性 = "否定" if _否定.search(raw) else "肯定"
    normalized = re.sub(r"\b(?:do|does|did|is|are|was|were|can|could|may|might|must|will|would|should|has|have|had)\s+not\b", lambda m: m.group(0).rsplit(None, 1)[0], raw, flags=re.I)
    passive = _受動.fullmatch(normalized)
    if passive:
        kind, predicate = _関係種別(passive.group("v"))
        obj, mods = _declaration_conditions(passive.group("s"))
        return 英語明示述語関係(kind, _端点(passive.group("o")), obj, predicate, 極性, mods)
    字句 = normalized.split()
    if len(字句) < 3:
        return None
    predicate_info = _predicate_from_字句(字句, 1)
    if predicate_info:
        predicate_surface, tail = predicate_info
        predicate_index = next((i for i, t in enumerate(字句[1:], 1) if predicate_surface.casefold().startswith(t.casefold().strip("?,.;:"))), None)
        if predicate_index is not None and tail:
            主体_surface = _端点(" ".join(字句[:predicate_index]))
            主体 = _開放主語(主体_surface)
            object_text, mods = _declaration_conditions(" ".join(tail))
            if 主体 and object_text:
                kind, predicate = _関係種別(predicate_surface)
                if kind == "開放述語" and (
                    _協調接続終端.search(主体_surface)
                    or (_協調主語.search(主体) and _末尾補助語.search(主体_surface))
                ):
                    return None
                return 英語明示述語関係(kind, 主体, object_text, predicate, 極性, mods)
    copula = re.match(r"^(?P<s>.+?)\s+(?:is|are|was|were)\s+(?P<o>.+)$", raw, re.I)
    if copula:
        obj, mods = _declaration_conditions(copula.group("o"))
        return 英語明示述語関係("開放述語", _端点(copula.group("s")), obj, "be", 極性, mods)
    return None


def 英語明示述語関係抽出(text: str) -> tuple[英語明示述語関係, ...]:
    raw = _正規化(text)
    out: list[英語明示述語関係] = []
    seen: set[tuple[object, ...]] = set()
    for sentence in _文分割.split(raw):
        関係 = _declaration_one(sentence)
        if 関係 is None:
            continue
        key = (関係.種別, 関係.始点.casefold(), 関係.終点.casefold(), 関係.検索述語, 関係.極性, 関係.修飾)
        if key not in seen:
            seen.add(key)
            out.append(関係)
    return tuple(out)


__all__ = [
    "英日意味制御", "英日関係質問", "英日意味フレーム", "英語明示述語関係",
    "英日意味フレーム抽出", "英語明示述語関係抽出",
]

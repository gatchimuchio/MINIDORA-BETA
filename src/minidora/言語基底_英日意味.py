from __future__ import annotations

from dataclasses import dataclass, replace
import re
import unicodedata

from .言語基底_英語 import 英語基本形, 英語関係概念


@dataclass(frozen=True, slots=True)
class 英日意味制御:
    種別: str
    表層: str
    正本: str


@dataclass(frozen=True, slots=True)
class 英日関係質問:
    種別: str
    未知位置: str
    要求型: str
    既知端点: str
    検索述語: str
    反転: bool = False
    受動: bool = False
    修飾: tuple[tuple[str, str], ...] = ()


@dataclass(frozen=True, slots=True)
class 英日意味フレーム:
    正本意味: tuple[str, ...]
    外部検索語: tuple[str, ...]
    制御: tuple[英日意味制御, ...]
    関係質問: 英日関係質問 | None = None


@dataclass(frozen=True, slots=True)
class 英語質問境界:
    焦点: str
    本体: str
    条件範囲: tuple[str, ...]
    質問表示: bool
    表示根拠: str
    境界状態: str


_語 = re.compile(r"[A-Za-z][A-Za-z0-9_-]*")
_末尾疑問符 = re.compile(r"[?？]+$")
_文分割 = re.compile(r"(?<=[?!.。？！])\s+|\n+")
_先頭疑問語 = re.compile(r"^(?:which|what|who|where|when|why|how)\b", re.I)
_引用区間 = re.compile(
    r'''"[^"]*"|“[^”]*”|「[^」]*」|『[^』]*』|(?<!\w)'(?:[^']|(?<=\w)'(?=\w))*'(?!\w)|‘[^’]*’'''
)
_関係句 = r"(?P<v>[A-Za-z]+(?:\s+(?:to|in|on|with|against|from|of))?)"
_型 = r"(?P<kind>[A-Za-z][A-Za-z0-9 _-]{0,72}?)"
_助動 = r"(?:(?:would|could|may|might|can|must)\s+)?"
_受動助動 = r"(?:is|are|was|were|has\s+been|have\s+been|had\s+been|(?:would|could|may|might|can|must)\s+be)"

_条件導入 = re.compile(
    r"^(?:(?:if|when|under|given|assuming|unless)\b|in\s+the\s+(?:presence|absence)\s+of\b)",
    re.I,
)
_有限助動詞 = r"(?:do|does|did|is|are|was|were|has|have|had|can|could|may|might|must|should|would|will)"
_先頭助動詞 = re.compile(rf"^{_有限助動詞}\b", re.I)
_when倒置 = re.compile(rf"^when\s+(?P<adjunct>[^,;?!]*?)\b{_有限助動詞}\b", re.I)
_疑問前置詞 = r"(?:in|on|at|to|for|from|with|without|by|of|about|under|over|between|among|through|within|during|before|after)"
_前置疑問句 = re.compile(rf"^{_疑問前置詞}\s+(?:which|what|who|whom|whose)\b", re.I)
_疑問時点修飾 = frozenset({
    "exactly", "precisely", "approximately", "roughly", "actually", "typically",
    "usually", "normally", "else", "ever", "again", "next",
})
_when前置句 = re.compile(
    r"^when\s+(?:in|on|at|during|before|after|between|within|around|since|until|throughout)\b",
    re.I,
)
_後置質問条件 = re.compile(r"\b(?:when|if|unless|under|given|assuming)\b", re.I)
_括弧対応 = {"(": ")", "[": "]"}

_受動未知対象 = re.compile(
    rf"^(?:which|what)\s+(?:of\s+the\s+following\s+)?{_型}\s+"
    rf"{_受動助動}\s+{_関係句}\s+by\s+(?P<s>.+)$",
    re.I,
)
_選択肢能動 = re.compile(
    rf"^(?:which|what)\s+of\s+the\s+following\s+"
    rf"(?:(?:is|are)\s+)?(?:(?P<degree>most|least)\s+likely\s+to\s+|(?P<likelihood>likely|unlikely)\s+to\s+)?"
    rf"{_助動}{_関係句}\s+(?P<o>.+)$",
    re.I,
)
_能動未知対象 = re.compile(
    rf"^(?:which|what)\s+(?:of\s+the\s+following\s+)?{_型}\s+"
    rf"(?:(?:is|are)\s+)?(?:(?P<degree>most|least)\s+likely\s+to\s+|(?P<likelihood>likely|unlikely)\s+to\s+)?"
    rf"{_助動}{_関係句}\s+(?P<o>.+)$",
    re.I,
)
_能動未知終点 = re.compile(
    rf"^(?:which|what)\s+(?:of\s+the\s+following\s+)?{_型}\s+"
    rf"(?:does|do|did)\s+(?P<s>.+?)\s+{_関係句}$",
    re.I,
)
_無型未知終点 = re.compile(
    rf"^(?:what|which)\s+(?:does|do|did)\s+(?P<s>.+?)\s+{_関係句}$",
    re.I,
)
_選択肢受動 = re.compile(
    rf"^(?:which|what)\s+of\s+the\s+following\s+"
    rf"{_受動助動}\s+{_関係句}\s+by\s+(?P<s>.+)$",
    re.I,
)

_制御規則 = (
    ("選択", re.compile(r"\b(?:least\s+likely|unlikely|except|most\s+unlikely)\b", re.I), "反転"),
    ("否定", re.compile(r"\b(?:not|no|never|without|cannot|can't|does\s+not|is\s+not)\b", re.I), "否定"),
    ("量化", re.compile(r"\b(?:all|each|every)\b", re.I), "全称"),
    ("量化", re.compile(r"\b(?:some|any)\b", re.I), "不定"),
    ("量化", re.compile(r"\b(?:none|neither)\b", re.I), "全否定"),
    ("比較", re.compile(r"\b(?:greater|higher|more)\s+than\b", re.I), "大"),
    ("比較", re.compile(r"\b(?:less|lower)\s+than\b", re.I), "小"),
    ("比較", re.compile(r"\bat\s+least\b", re.I), "以上"),
    ("比較", re.compile(r"\bat\s+most\b", re.I), "以下"),
    ("条件", re.compile(r"\b(?:if|when|under|given|assuming)\b", re.I), "条件"),
    ("条件", re.compile(r"\bunless\b|\bin\s+the\s+absence\s+of\b", re.I), "否定条件"),
    ("条件", re.compile(r"\bexcept\b", re.I), "例外"),
    ("様相", re.compile(r"\b(?:can|could|may|might|would)\b", re.I), "可能"),
    ("様相", re.compile(r"\b(?:must|required|necessary)\b", re.I), "必要"),
    ("蓋然性", re.compile(r"\bmost\s+likely\b", re.I), "最大"),
    ("蓋然性", re.compile(r"\bleast\s+likely\b|\bunlikely\b", re.I), "最小"),
)

_検索除外 = frozenset(
    {
        "a", "an", "the", "of", "to", "in", "on", "at", "for", "from", "with", "and", "or",
        "is", "are", "was", "were", "be", "been", "being", "which", "what", "who", "when", "where",
        "why", "how", "this", "that", "these", "those", "it", "its", "do", "does", "did", "has", "have",
        "had", "would", "could", "may", "might", "can", "must", "most", "least", "likely", "unlikely",
        "following", "statement", "statements", "answer", "answers", "option", "options", '選択肢', "choices",
        "correct", "incorrect", "true", "false", "best", "select", "choose",
    }
)


def _正規化(text: str) -> str:
    return " ".join(unicodedata.normalize("NFKC", str(text)).split()).strip()


def _引用外(text: str) -> str:
    """引用内容を位置を変えず覆う。単語内部のapostropheは引用開始にしない。"""
    return _引用区間.sub(lambda match: " " * len(match.group(0)), text)


def _括弧外位置(visible: str) -> frozenset[int] | None:
    stack: list[str] = []
    outside: set[int] = set()
    for index, char in enumerate(visible):
        if char in _括弧対応:
            stack.append(_括弧対応[char])
        elif char in _括弧対応.values():
            if not stack or stack.pop() != char:
                return None
        elif not stack:
            outside.add(index)
    return frozenset(outside) if not stack else None


def _全文括弧除去(text: str) -> str:
    """本文全体を包む整合した括弧だけを外し、疑問・感嘆の根拠を残す。"""
    raw = text.strip()
    while raw:
        body = raw.rstrip(".。?!？！").rstrip()
        suffix = raw[len(body):]
        visible = _引用外(body)
        if not visible or visible[0] not in _括弧対応:
            break
        stack: list[str] = []
        complete = False
        for index, char in enumerate(visible):
            if char in _括弧対応:
                stack.append(_括弧対応[char])
            elif char in _括弧対応.values():
                if not stack or stack.pop() != char:
                    break
                if not stack:
                    complete = index == len(visible) - 1
                    break
        if not complete:
            break
        raw = body[1:-1].strip() + suffix.strip()
    return raw


def _直接when疑問(visible: str) -> bool:
    """when句に続く助動詞倒置を認定し、主語を含む条件節と分ける。"""
    match = _when倒置.match(visible)
    if match is None:
        return False
    adjunct = match.group("adjunct").strip().casefold()
    字句 = _語.findall(adjunct)
    if " ".join(字句) != adjunct:
        return False
    while 字句 and 字句[0] in _疑問時点修飾:
        字句.pop(0)
    return not 字句


def _when前置句未確定(visible: str) -> bool:
    """前置詞句の終わりを推測せず、別主節が見つからない問い候補を全文保持する。"""
    return _when前置句.match(visible) is not None and _when倒置.match(visible) is not None


def _主節疑問表示(visible: str) -> bool:
    if re.match(r"^when\b", visible, re.I):
        if _直接when疑問(visible):
            return True
        outside = _括弧外位置(visible)
        return (_when前置句未確定(visible) and outside is not None
                and not any(visible[index] in ",;" for index in outside))
    return bool(_先頭疑問語.match(visible) or _前置疑問句.match(visible))


def _文の質問境界(text: str) -> 英語質問境界:
    raw = _全文括弧除去(text)
    visible = _引用外(raw)
    explicit = "?" in visible or "？" in visible
    exclamation = visible.rstrip(".。").endswith(("!", "！"))
    direct_when = _直接when疑問(visible)
    uncertain_when = _when前置句未確定(visible)
    main_question = _主節疑問表示(visible)
    conditional = _条件導入.match(visible) is not None and not main_question
    outside = _括弧外位置(visible)
    balanced = outside is not None
    outside = outside if outside is not None else frozenset()
    boundaries = [index for index in sorted(outside)
                  if visible[index] in ",;"
                  and (_主節疑問表示(visible[index + 1:].lstrip())
                       or (explicit and _先頭助動詞.match(visible[index + 1:].lstrip())))] if conditional else []
    body, conditions, 状態 = raw, (), "主節"
    if not balanced:
        状態 = "括弧境界未確定"
    elif len(boundaries) == 1 and boundaries[0] <= 220:
        index = boundaries[0]
        body, conditions = raw[index + 1:].strip(), (raw[:index].strip(),)
        状態 = "先頭条件"
    elif conditional and boundaries:
        状態 = "条件境界未確定"
    elif uncertain_when:
        状態 = "when前置句境界未確定"
    elif conditional:
        状態 = "条件主節未確認"
    elif direct_when:
        状態 = "直接when疑問"
    main_wh = _主節疑問表示(_引用外(body)) and not conditional
    if len(boundaries) == 1:
        main_wh = True
    displayed = explicit or (not exclamation and (main_wh or bool(boundaries)))
    # 後置条件は質問の主節が確認できた場合だけ分離する。直接when疑問は全文を保つ。
    if displayed and 状態 == "主節":
        for match in _後置質問条件.finditer(visible):
            if match.start() > 0 and match.start() in outside:
                body = raw[:match.start()].strip(" ,;:")
                conditions = (raw[match.start():].rstrip(" .。?!？！"),)
                状態 = "後置条件"
                break
    reason = "疑問符" if explicit else "主節疑問語" if displayed else "なし"
    return 英語質問境界(raw.rstrip(".。"), body.rstrip(" .。?!？！").strip(),
                    conditions, displayed, reason, 状態)


def 英語質問境界解析(text: str) -> 英語質問境界:
    """焦点・質問表示・本体・条件を同じ有限な境界認定から返す。"""
    raw = _全文括弧除去(_正規化(text))
    if not raw:
        return _文の質問境界("")
    # 引用・括弧内の文末記号で外側の文を分断しない。
    parts: list[str] = []
    start = 0
    visible = _引用外(raw)
    outside = _括弧外位置(visible)
    for 境界 in _文分割.finditer(visible):
        if outside is None or 境界.start() not in outside:
            continue
        part = _正規化(raw[start:境界.start()])
        if part:
            parts.append(part)
        start = 境界.end()
    tail = _正規化(raw[start:])
    if tail:
        parts.append(tail)
    parsed = [_文の質問境界(part) for part in parts]
    for part in reversed(parsed):
        if part.表示根拠 == "疑問符":
            return part
    for part in reversed(parsed):
        if part.質問表示:
            return part
    for part in reversed(parsed):
        if part.境界状態 == "when前置句境界未確定":
            return part
    return parsed[-1] if parsed else _文の質問境界(raw)


def _質問焦点(text: str) -> str:
    return 英語質問境界解析(text).焦点


def 英語質問表示(text: str) -> bool:
    """基礎・強化抽出と残差判定で同じ質問焦点を使う。内容を解析できたかとは別。"""
    return 英語質問境界解析(text).質問表示


def _端点(text: str) -> str:
    value = _末尾疑問符.sub("", _正規化(text)).strip(" ,;:()[]")
    return value


def _要求型(text: str) -> str:
    value = _正規化(text)
    value = re.sub(r"^(?:of\s+the\s+following\s+)", "", value, flags=re.I)
    value = re.sub(r"\b(?:most|least)\s+likely\s*$", "", value, flags=re.I)
    return value.strip()


def _関係句意味(surface: str) -> tuple[str, str] | None:
    phrase = _正規化(surface).casefold()
    head = phrase.split()[0] if phrase else ""
    kind = 英語関係概念(head)
    if kind is None:
        return None
    lemma = 英語基本形(head)
    suffix = ""
    if " " in phrase:
        suffix = " " + phrase.split(" ", 1)[1]
    return kind, lemma + suffix


def _反転(match: re.Match[str] | None, raw: str) -> bool:
    if match is not None:
        degree = (match.groupdict().get("degree") or "").casefold()
        likelihood = (match.groupdict().get("likelihood") or "").casefold()
        if degree == "least" or likelihood == "unlikely":
            return True
    lowered = raw.casefold()
    return "least likely" in lowered or "most unlikely" in lowered or " except" in (" " + lowered)


def _質問本体と条件範囲(text: str) -> tuple[str, tuple[str, ...]]:
    """共有境界で確認した質問本体と局所条件を返す。"""
    境界 = 英語質問境界解析(text)
    return 境界.本体, 境界.条件範囲


def _質問関係(text: str) -> 英日関係質問 | None:
    raw = _端点(text)

    match = _選択肢受動.fullmatch(raw)
    if match:
        関係 = _関係句意味(match.group("v"))
        if 関係 is not None:
            kind, predicate = 関係
            return 英日関係質問(kind, "終点", "選択肢", _端点(match.group("s")), predicate, _反転(match, raw), True)

    match = _選択肢能動.fullmatch(raw)
    if match:
        関係 = _関係句意味(match.group("v"))
        if 関係 is not None:
            kind, predicate = 関係
            return 英日関係質問(kind, "始点", "選択肢", _端点(match.group("o")), predicate, _反転(match, raw), False)

    match = _受動未知対象.fullmatch(raw)
    if match:
        関係 = _関係句意味(match.group("v"))
        if 関係 is not None:
            kind, predicate = 関係
            return 英日関係質問(kind, "終点", _要求型(match.group("kind")), _端点(match.group("s")), predicate, _反転(match, raw), True)

    match = _能動未知終点.fullmatch(raw)
    if match:
        関係 = _関係句意味(match.group("v"))
        if 関係 is not None:
            kind, predicate = 関係
            return 英日関係質問(kind, "終点", _要求型(match.group("kind")), _端点(match.group("s")), predicate, _反転(match, raw), False)

    match = _無型未知終点.fullmatch(raw)
    if match:
        関係 = _関係句意味(match.group("v"))
        if 関係 is not None:
            kind, predicate = 関係
            return 英日関係質問(kind, "終点", "", _端点(match.group("s")), predicate, _反転(match, raw), False)

    match = _能動未知対象.fullmatch(raw)
    if match:
        関係 = _関係句意味(match.group("v"))
        if 関係 is not None:
            kind, predicate = 関係
            return 英日関係質問(kind, "始点", _要求型(match.group("kind")), _端点(match.group("o")), predicate, _反転(match, raw), False)
    return None


def _制御(text: str) -> tuple[英日意味制御, ...]:
    out: list[英日意味制御] = []
    seen: set[tuple[str, str]] = set()
    for kind, pattern, canonical in _制御規則:
        for match in pattern.finditer(text):
            key = (kind, canonical)
            if key in seen:
                continue
            seen.add(key)
            out.append(英日意味制御(kind, _正規化(match.group(0)), canonical))
    return tuple(out)


def _関係質問修飾(
    controls: tuple[英日意味制御, ...],
    condition_scopes: tuple[str, ...],
) -> tuple[tuple[str, str], ...]:
    '質問に明示された関係修飾だけをHDS 関係 identityへ射影する。'
    out: list[tuple[str, str]] = []
    seen: set[tuple[str, str]] = set()
    for item in controls:
        if item.種別 not in {"様相", "量化"}:
            continue
        pair = (item.種別, item.正本)
        if pair not in seen:
            seen.add(pair)
            out.append(pair)
    for condition in condition_scopes:
        pair = ('条件範囲', condition)
        if condition and pair not in seen:
            seen.add(pair)
            out.append(pair)
    return tuple(out)


def _検索語(text: str) -> tuple[str, ...]:
    out: list[str] = []
    seen: set[str] = set()
    for 字句 in _語.findall(text):
        value = 字句.casefold()
        if value in _検索除外:
            continue
        lemma = 英語基本形(value)
        if lemma in _検索除外 or not lemma:
            continue
        if lemma not in seen:
            seen.add(lemma)
            out.append(lemma)
        関係 = 英語関係概念(lemma)
        if 関係 is not None:
            marker = f"rel:{関係}"
            if marker not in seen:
                seen.add(marker)
                out.append(marker)
    return tuple(out)


def 英日意味フレーム抽出(text: str) -> 英日意味フレーム:
    境界 = 英語質問境界解析(text)
    focus = 境界.焦点
    controls = _制御(focus)
    question_body, condition_scopes = 境界.本体, 境界.条件範囲
    question = _質問関係(question_body) if 境界.質問表示 and 境界.境界状態 != "括弧境界未確定" else None
    if question is not None:
        question = replace(question, 修飾=_関係質問修飾(controls, condition_scopes))

    canonical: list[str] = [f"{item.種別}:{item.正本}" for item in controls]
    if condition_scopes:
        canonical.extend(f"条件scope:{範囲}" for 範囲 in condition_scopes)
    if question is not None:
        canonical.extend((f"関係:{question.種別}", f"不足位置:{question.未知位置}", f"要求型:{question.要求型}" if question.要求型 else "要求型:未特定"))
        canonical.extend(f"関係修飾:{key}={value}" for key, value in question.修飾)
    return 英日意味フレーム(tuple(canonical), _検索語(focus), controls, question)


__all__ = ["英日意味制御", "英日関係質問", "英日意味フレーム", "英日意味フレーム抽出", "英語質問表示", "英語質問境界", "英語質問境界解析"]

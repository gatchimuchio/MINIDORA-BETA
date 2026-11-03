from __future__ import annotations

from dataclasses import dataclass, replace
import re
import unicodedata

from .意味字句 import 意味語
from .言語基底_英語 import 英語明示関係構文 as _原本英語構文, 英語関係一致, 英語関係構文


_語 = re.compile(r"[A-Za-z0-9_+./^%µμΩ°\-]+|[Α-Ωα-ωϐ-Ͽ]+|[ぁ-んァ-ヶー]+|[一-龥々]+|[^\s]")
_英語否定 = re.compile(
    r"\b(?:do|does|did|is|are|was|were|be|been|being|can|could|may|might|must|will|would|should|has|have|had)\s+not\b|\bnever\b",
    re.I,
)
_日本語否定 = re.compile(r"(?:ではない|じゃない|しない|ない|ず|ぬ)")
_条件 = (
    re.compile(r"\b(?:if|when|given|assuming|unless)\s+([^,;.!?]{1,160})", re.I),
    re.compile(r"\bunder\s+([^,;.!?]{1,160})", re.I),
    re.compile(r"(?:もし|場合|とき|条件下|前提(?:として)?)([^。！？、]{1,120})"),
)
_記号関係 = re.compile(
    r"(?P<s>[A-Za-zΑ-Ωα-ωϐ-Ͽ0-9_µμΩ.+\-]+)\s*"
    r"(?P<op>->|=>|→|⇒|>=|<=|≥|≤|!=|≠|>|<|=)\s*"
    r"(?P<o>[A-Za-zΑ-Ωα-ωϐ-Ͽ0-9_µμΩ.+\-]+)"
)

_日本語関係語 = (
    ("因果", ("引き起こす", "生じさせる", "もたらす", "原因となる")),
    ("増加", ("増加させる", "高める", "促進する")),
    ("減少", ("減少させる", "低下させる", "抑える")),
    ("阻害", ("阻害する", "抑制する", "遮断する")),
    ("活性化", ("活性化する", "刺激する")),
    ("生成", ("生成する", "産生する", "作る")),
    ("要求", ("必要とする", "依存する")),
    ("包含", ("含む", "包含する")),
    ("使用", ("使う", "使用する", "利用する")),
    ("防止", ("防ぐ", "予防する")),
)


def _関係正規表現(verbs: tuple[str, ...]) -> re.Pattern[str]:
    choices = "|".join(re.escape(v) for v in sorted(verbs, key=len, reverse=True))
    return re.compile(
        rf"(?P<s>[^。！？、]{{1,100}}?)(?:が|は)(?P<o>[^。！？、]{{1,100}}?)(?:を)?(?P<v>{choices})"
    )


def _日本語否定形(verb: str) -> tuple[str, ...]:
    """既存述語の辞書形から一般的な否定活用だけを派生する。"""
    if verb.endswith("する"):
        stem = verb[:-2]
        return (stem + "しない", stem + "しません", stem + "せず", stem + "せぬ")
    if verb.endswith("させる") or verb.endswith("める") or verb.endswith("える"):
        stem = verb[:-1]
        return (stem + "ない", stem + "ません", stem + "ず", stem + "ぬ")
    last = verb[-1:]
    a_row = {"う":"わ","く":"か","ぐ":"が","す":"さ","つ":"た","ぬ":"な","ぶ":"ば","む":"ま","る":"ら"}
    i_row = {"う":"い","く":"き","ぐ":"ぎ","す":"し","つ":"ち","ぬ":"に","ぶ":"び","む":"み","る":"り"}
    if last in a_row:
        base = verb[:-1]
        return (
            base + a_row[last] + "ない",
            base + i_row[last] + "ません",
            base + a_row[last] + "ず",
            base + a_row[last] + "ぬ",
        )
    return ()


_日本語関係構文 = tuple((kind, _関係正規表現(verbs)) for kind, verbs in _日本語関係語)
_日本語否定関係構文 = tuple(
    (kind, _関係正規表現(tuple(form for verb in verbs for form in _日本語否定形(verb))))
    for kind, verbs in _日本語関係語
)

_記号種別 = {
    "->": "方向", "=>": "方向", "→": "方向", "⇒": "方向",
    ">": "比較.大", "<": "比較.小", ">=": "比較.以上", "≥": "比較.以上",
    "<=": "比較.以下", "≤": "比較.以下", "=": "等価", "!=": "不同", "≠": "不同",
}
_明示対比境界 = re.compile(r"\s*,\s*(?:but|whereas)\s+|\s*;\s*(?:but|whereas)\s+|、?しかし(?:、)?", re.I)


@dataclass(frozen=True, slots=True)
class 言語関係構造:
    種別: str
    始点: frozenset[str]
    終点: frozenset[str]
    肯定: bool = True
    条件: tuple[frozenset[str], ...] = ()
    述語: frozenset[str] = frozenset()

    @property
    def 署名(self) -> tuple[object, ...]:
        return (
            self.種別,
            tuple(sorted(self.始点)), tuple(sorted(self.終点)), self.肯定,
            tuple(tuple(sorted(item)) for item in self.条件),
            tuple(sorted(self.述語)) if self.種別 in (
                "開放述語", "命題適合", "説明適合", "問い適合", "同定", "数量同定",
            ) else (),
        )


def _意味集合(text: str) -> frozenset[str]:
    value = " ".join(str(text).split()).strip(" ,;:。！？?")
    字句 = 意味語(value)
    # 物理量・記号の大文字/小文字を汎用語のcasefoldへ埋没させない。
    # 追加知識ではなく、入力に存在する記号identityの保全。
    if re.fullmatch(r"[A-Za-zΑ-Ωα-ω](?:[0-9_][A-Za-z0-9_]*)?", value):
        字句 = 字句 | frozenset({"識別記号:" + value})
    return 字句


def 意味列(text: str) -> tuple[str, ...]:
    out: list[str] = []
    for surface in _語.findall(unicodedata.normalize("NFKC", str(text))):
        values = sorted(意味語(surface))
        if values:
            out.extend(values)
    return tuple(out)


def _条件群(text: str) -> tuple[frozenset[str], ...]:
    out: list[frozenset[str]] = []
    seen: set[tuple[str, ...]] = set()
    for pattern in _条件:
        for match in pattern.finditer(text):
            value = _意味集合(match.group(1))
            if re.match(r"unless\b", match.group(0), re.I) or re.search(r"\bnot\b", match.group(1), re.I):
                value = value | frozenset({"条件否定"})
            if not value:
                continue
            key = tuple(sorted(value))
            if key in seen:
                continue
            seen.add(key); out.append(value)
    return tuple(out)


def _否定除去(text: str) -> tuple[str, bool]:
    normalized = unicodedata.normalize("NFKC", str(text))
    negative = bool(_英語否定.search(normalized) or _日本語否定.search(normalized))
    cleaned = re.sub(r"\b(?:do|does|did)\s+not\s+", "", normalized, flags=re.I)
    cleaned = re.sub(
        r"\b(is|are|was|were|be|been|being|can|could|may|might|must|will|would|should|has|have|had)\s+not\s+",
        r"\1 ", cleaned, flags=re.I,
    )
    return cleaned, negative


def _文単位(text: str) -> tuple[str, ...]:
    '否定・条件の範囲を文と明示対比境界へ局所化する。小数点は分割しない。'
    value = unicodedata.normalize("NFKC", str(text))
    parts: list[str] = []
    start = 0
    for index, char in enumerate(value):
        sentence_end = char in "。！？!?\n"
        if char == ".":
            left_digit = index > 0 and value[index - 1].isdigit()
            right_digit = index + 1 < len(value) and value[index + 1].isdigit()
            sentence_end = not (left_digit and right_digit)
        if sentence_end:
            piece = (value[start:index] + (char if char in "?？" else "")).strip()
            if piece:
                parts.extend(x.strip() for x in _明示対比境界.split(piece) if x.strip())
            start = index + 1
    tail = value[start:].strip()
    if tail:
        parts.extend(x.strip() for x in _明示対比境界.split(tail) if x.strip())
    return tuple(parts) if parts else (value,)


def _単位関係抽出(raw: str, 言語体系: str) -> tuple[言語関係構造, ...]:
    conditions = _条件群(raw)
    body = _条件本文(raw)
    outer_negation = False
    wrapper = re.match(r"^(?:it\s+is\s+false\s+that|it\s+is\s+not\s+true\s+that)\s+(.+)$", body, re.I)
    if wrapper and not re.search(r"\b(?:and|or|but|whereas)\b",wrapper.group(1),re.I):
        body = wrapper.group(1)
        outer_negation = True
    cleaned, negative = _否定除去(body)
    negative = negative != outer_negation
    if "?" in raw or "？" in raw or re.match(r"^(?:what|which|who|why|how|does|did|do|is|are)\b", raw, re.I):
        conditions = (*conditions, frozenset({"未確定疑問"}))
    # 様相・選言を確定した単一関係へ昇格させない。
    if re.search(r"\b(?:may|might|could|possibly|perhaps|either|likely|probably|potentially)\b|かもしれない", raw, re.I):
        conditions = (*conditions, frozenset({"未確定様相"}))
    if re.search(r"\bor\b|または|或いは", re.sub(r"\s+or\s+equal\s+to\b", "", raw, flags=re.I), re.I):
        conditions = (*conditions, frozenset({"未確定選言"}))
    out: list[言語関係構造] = []
    seen: set[tuple[object, ...]] = set()

    def add(kind: str, 主体: str, object_: str, predicate: str, *, reverse: bool = False, positive: bool | None = None) -> None:
        s = _意味集合(主体); o = _意味集合(object_); p = _意味集合(predicate)
        if kind == "開放述語" and predicate.lower().strip() in {"is","are","was","were"}:
            p = frozenset({"文法述語:be"})
        elif kind == "開放述語" and predicate.lower().strip() in {"has","have","had"}:
            p = frozenset({"文法述語:have"})
        if reverse: s, o = o, s
        if not s or not o: return
        item = 言語関係構造(kind, s, o, not negative if positive is None else positive, conditions, p)
        if item.署名 in seen: return
        seen.add(item.署名); out.append(item)

    for match in _記号関係.finditer(cleaned):
        add(_記号種別[match.group("op")], match.group("s"), match.group("o"), match.group("op"))

    if str(言語体系).casefold().startswith("自然言語:en") or re.search(r"[A-Za-z]", cleaned):
        matches = _英文一致(cleaned)
        passive_spans = [match.span() for syntax, match in matches if syntax.反転]
        for syntax, match in matches:
            if not syntax.反転 and any(a <= match.start() and match.end() <= b for a, b in passive_spans):
                continue
            # 条件前置部を関係の主語へ混ぜない。
            主体 = re.sub(r"^(?:if|when|given|assuming|unless|under)\b[^,]*,\s*", "", match.group("s"), flags=re.I)
            add(syntax.種別, 主体, match.group("o"), match.group("v"), reverse=syntax.反転)
        for match in _自然文比較構文.finditer(cleaned):
            kind = _比較語形種別[" ".join(match.group("op").lower().split())]
            add(kind, match.group("s"), match.group("o"), match.group("v"))

    if str(言語体系).casefold().startswith("自然言語:ja") or re.search(r"[ぁ-んァ-ヶ一-龥]", cleaned):
        for kind, pattern in _日本語関係構文:
            for match in pattern.finditer(cleaned):
                add(kind, match.group("s"), match.group("o"), match.group("v"), positive=True)
        for kind, pattern in _日本語否定関係構文:
            for match in pattern.finditer(raw):
                add(kind, match.group("s"), match.group("o"), match.group("v"), positive=False)

    return tuple(out)


def 言語関係抽出(text: str, 言語体系: str = "自然言語:ja") -> tuple[言語関係構造, ...]:
    out: list[言語関係構造] = []
    seen: set[tuple[object, ...]] = set()
    for unit in _自然文節単位(text, 言語体系):
        for item in _単位関係抽出(unit, 言語体系):
            if item.署名 in seen: continue
            seen.add(item.署名); out.append(item)
    return tuple(out)


__all__ = ["言語関係構造", "意味列", "言語関係抽出"]


# 言語形の比較対応。最長の演算子を一度だけ解析し、以上を大・等価へ誤分解しない。
_比較語形種別 = {
    "greater than or equal to":"比較.以上", "at least as large as":"比較.以上",
    "less than or equal to":"比較.以下", "at most as large as":"比較.以下",
    "greater than":"比較.大", "larger than":"比較.大", "higher than":"比較.大",
    "less than":"比較.小", "smaller than":"比較.小", "lower than":"比較.小",
    "equal to":"等価",
}
_比較選択肢 = "|".join(re.escape(x) for x in sorted(_比較語形種別,key=len,reverse=True))
_自然文比較構文 = re.compile(
    rf"(?P<s>[^,;!?]{{1,120}}?)\s+(?P<v>(?:is|are|was|were)\s+(?P<op>{_比較選択肢}))\s+(?P<o>[^,;!?]{{1,120}})", re.I)


def _条件本文(raw: str) -> str:
    """記述済み条件は条件欄に保持し、主語・目的語の一部として二重解釈しない。"""
    body = re.sub(r"^(?:if|when|given|assuming|unless|under)\b[^,]*,\s*", "", raw.strip(), flags=re.I)
    return re.sub(r"\s+(?:if|when|given|assuming|unless|under)\s+[^,;!?]+[?？]?$", "", body, flags=re.I)

_関係節境界 = re.compile(r"\s*;\s*|\s*,?\s+\b(?:but|whereas|and)\b\s+|、?しかし、?", re.I)
_疑問開始 = re.compile(r"^(?:what|which|who|why|how|when|where)\b", re.I)
_助動詞末尾 = re.compile(r"\s+(?:do|does|did|is|are|was|were|can|will|would|could|should|may|might)$", re.I)


def _自然文節単位(text: str, 言語体系: str = "自然言語:ja") -> tuple[str, ...]:
    '明示された並列述語の省略主語だけを継承し、否定・目的語の範囲を分ける。'
    out = []
    for sentence in _文単位(text):
        pieces = [x.strip() for x in _関係節境界.split(sentence) if x.strip()]
        if len(pieces) <= 1:
            out.append(sentence)
            continue
        expanded = []
        主体 = None
        valid = True
        範囲_match = re.match(r"^((?:if|when|given|assuming|unless|under)\b[^,]*,\s*)",sentence,re.I)
        範囲 = 範囲_match.group(1) if 範囲_match else ""
        for piece_index, piece in enumerate(pieces):
            if piece_index and 範囲 and not re.match(r"^(?:if|when|given|assuming|unless|under)\b",piece,re.I):
                piece = 範囲 + piece
            cleaned, _ = _否定除去(_条件本文(piece))
            matches = _英文一致(cleaned)
            comparisons = list(_自然文比較構文.finditer(cleaned))
            if comparisons:
                主体 = comparisons[0].group("s").strip()
                expanded.append(piece)
                continue
            if not matches and 主体:
                local_body = _条件本文(piece)
                own_範囲 = piece[:-len(local_body)] if local_body and piece.endswith(local_body) else ""
                trial = own_範囲 + 主体 + " " + local_body
                trial_cleaned, _ = _否定除去(_条件本文(trial))
                matches = _英文一致(trial_cleaned)
                if matches:
                    piece = trial
            if not matches:
                valid = False
                break
            preferred = next(((sy, ma) for sy, ma in matches if sy.反転), matches[0])
            主体 = _助動詞末尾.sub("", preferred[1].group("s")).strip()
            expanded.append(piece)
        out.extend(expanded if valid else [sentence])
    return tuple(out)


def 問い候補関係形成(question: str, 候補: str, 言語体系: str = "自然言語:ja") -> tuple[言語関係構造, ...]:
    """問いの未知端点へ候補を束縛する。候補の正しさは判定せず、照合対象だけを作る。"""
    if not question.strip() or not 候補.strip():
        return ()
    units = _文単位(question)
    # 最後の注意書きを問いそのものへ誤認しない。明示疑問節を後方から探索する。
    unit = next((x for x in reversed(units) if "?" in x or "？" in x or _疑問開始.match(_条件本文(x))), units[-1] if units else question).strip()
    unit = unit.rstrip("?？")
    # 条件を保持したまま、末尾の実際の疑問節だけを対象化する。
    tail = re.sub(r"^(?:if|when|given|assuming|unless|under)\b[^,]*,\s*", "", unit, flags=re.I)
    conditions = _条件群(unit)
    candidates = []
    # does X not inhibit -> does X inhibit を極性付きで保持する。
    question_negative = re.search(r"\bnot\b|\bnever\b", tail, re.I) is not None
    cleaned, negative = _否定除去(tail)
    cleaned = re.sub(r"\s+not\s+", " ", cleaned, flags=re.I)
    negative = negative or question_negative
    copula = re.fullmatch(r"(?i:what|which(?:\s+\w+)?)\s+((?i:is|are))\s+((?i:the)\s+.+|[A-Z][A-Z0-9_]*)", cleaned)
    if copula:
        bound = 言語関係抽出(copula.group(2) + " " + copula.group(1) + " " + 候補, 言語体系)
        return tuple(replace(item, 肯定=not item.肯定 if negative else item.肯定,
                             条件=tuple(dict.fromkeys((*item.条件,*conditions)))) for item in bound)
    if _疑問開始.match(tail):
        # 目的語空所: What does X inhibit? -> X inhibits <candidate>.
        match = re.fullmatch(r"(?:what|which(?:\s+of\s+the\s+following)?(?:\s+\w+)?)\s+(?:do|does|did)\s+(.+)", cleaned, re.I)
        if match:
            bound = 言語関係抽出(match.group(1) + " " + 候補, 言語体系)
            candidates.extend(replace(item, 肯定=not item.肯定) if negative else item for item in bound)
        else:
            # 主語空所・受動態: What inhibits Y? / What is inhibited by X?
            matches = [(syntax, m) for syntax,m in _英文一致(cleaned)
                       if re.match(r"^(?:what|which|who)\b", m.group("s"), re.I)]
            spans = [m.span() for syntax, m in matches if syntax.反転]
            for syntax, m in matches:
                if not syntax.反転 and any(a <= m.start() and m.end() <= b for a,b in spans):
                    continue
                left, right = _意味集合(候補), _意味集合(m.group("o"))
                if syntax.反転:
                    left, right = right, left
                if left and right:
                    predicate = m.group("v").lower().strip()
                    p = frozenset({"文法述語:be"}) if predicate in {"is","are","was","were"} else frozenset({"文法述語:have"}) if predicate in {"has","have","had"} else _意味集合(predicate)
                    candidates.append(言語関係構造(syntax.種別,left,right,not negative,conditions,p))
    elif re.search(r"何(?:が|は|を|に)|どれ(?:が|は|を)", tail):
        # 日本語の明示疑問端点。記述済み関係への単なる空所代入。
        filled = re.sub(r"(?:何|どれ)(?=が|は|を|に)", 候補, tail, count=1)
        candidates.extend(言語関係抽出(filled, 言語体系))
    out = []
    seen = set()
    for 関係 in candidates:
        関係 = replace(関係, 条件=tuple(dict.fromkeys((*関係.条件,*conditions))))
        if 関係.署名 not in seen:
            seen.add(関係.署名);out.append(関係)
    return tuple(out)


def 問題前提関係抽出(question: str, 言語体系: str = "自然言語:ja") -> tuple[言語関係構造,...]:
    """末尾の質問を事実扱いせず、質問より前に明示された前提だけを再利用する。"""
    units = _文単位(question)
    if len(units) < 2:
        return ()
    tail = units[-1]
    if not (_疑問開始.match(tail) or re.search(r"何|どれ|どの|か[?？]?$",tail)):
        return ()
    out = []
    for unit in units[:-1]:
        if _疑問開始.match(unit) or "?" in unit or "？" in unit:
            continue
        out.extend(言語関係抽出(unit, 言語体系))
    return tuple(out)


_補助英語構文 = (
    英語関係構文("開放述語",re.compile(r"(?P<s>[^,;.!?]{1,120}?)\s+(?P<v>is|are|was|were)\s+(?P<o>[^,;!?]{1,120})",re.I)),
    英語関係構文("開放述語",re.compile(r"(?P<s>[^,;.!?]{1,120}?)\s+(?P<v>has|have|had)\s+(?P<o>[^,;!?]{1,120})",re.I)),
)
英語明示関係構文 = (*_原本英語構文,*_補助英語構文)


def _英文一致(text):
    typed = [(sy,ma) for sy in _原本英語構文 for ma in 英語関係一致(sy,text)]
    # be + 分詞を属性・同一性へ重複射影しない。比較述語も同様に優先する。
    spans = [ma.span() for _,ma in typed] + [ma.span() for ma in _自然文比較構文.finditer(text)]
    extra = [(sy,ma) for sy in _補助英語構文 for ma in sy.正規表現.finditer(text)
             if not any(a<ma.end() and ma.start()<b for a,b in spans)]
    return typed+extra

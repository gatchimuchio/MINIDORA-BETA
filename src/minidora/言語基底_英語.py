from __future__ import annotations

from collections.abc import Iterator
from dataclasses import dataclass, field
import re
import unicodedata


@dataclass(frozen=True, slots=True)
class 英語関係構文:
    種別: str
    正規表現: re.Pattern[str]
    反転: bool = False
    述語必要条件: re.Pattern[str] | None = field(default=None, kw_only=True, compare=False, repr=False)


def 英語関係一致(構文: 英語関係構文, 本文: str) -> Iterator[re.Match[str]]:
    """同じ述語式が不在の標準構文だけを省略し、既存の全文探索を保持する。

    必要条件を持たない独自構文は従来どおり探索する。条件が一致しても、
    その位置から探索を始めず、完全式の全matchを元の順序で返す。
    """
    必要条件 = getattr(構文, "述語必要条件", None)
    if 必要条件 is not None and 必要条件.search(本文) is None:
        return iter(())
    return 構文.正規表現.finditer(本文)


# 世界知識ではなく、英語という言語体系の基底知識だけを保持する。
# 科学・技術文を含む一般文で頻出する関係語の屈折・派生差を同一概念へ戻す。
_英語基本形表 = {
    # 因果
    "cause": "cause", "causes": "cause", "caused": "cause", "causing": "cause",
    "causal": "cause", "causally": "cause", "causation": "cause",
    "lead": "lead", "leads": "lead", "led": "lead", "leading": "lead",
    '結果': '結果', "results": '結果', "resulted": '結果', "resulting": '結果',
    # 増加
    "increase": "increase", "increases": "increase", "increased": "increase", "increasing": "increase",
    "raise": "raise", "raises": "raise", "raised": "raise", "raising": "raise",
    "enhance": "enhance", "enhances": "enhance", "enhanced": "enhance", "enhancing": "enhance",
    "enhancement": "enhance", "enhancements": "enhance",
    # 減少
    "decrease": "decrease", "decreases": "decrease", "decreased": "decrease", "decreasing": "decrease",
    "reduce": "reduce", "reduces": "reduce", "reduced": "reduce", "reducing": "reduce",
    "reduction": "reduce", "reductions": "reduce", "reductive": "reduce",
    "lower": "lower", "lowers": "lower", "lowered": "lower", "lowering": "lower",
    # 阻害
    "inhibit": "inhibit", "inhibits": "inhibit", "inhibited": "inhibit", "inhibiting": "inhibit",
    "inhibition": "inhibit", "inhibitions": "inhibit", "inhibitory": "inhibit",
    "suppress": "suppress", "suppresses": "suppress", "suppressed": "suppress", "suppressing": "suppress",
    "suppression": "suppress", "suppressions": "suppress", "suppressive": "suppress",
    "block": "block", "blocks": "block", "blocked": "block", "blocking": "block", "blockage": "block",
    # 活性化
    "activate": "activate", "activates": "activate", "activated": "activate", "activating": "activate",
    "activation": "activate", "activations": "activate",
    "stimulate": "stimulate", "stimulates": "stimulate", "stimulated": "stimulate", "stimulating": "stimulate",
    "stimulation": "stimulate", "stimulations": "stimulate", "stimulatory": "stimulate",
    # 生成
    "produce": "produce", "produces": "produce", "produced": "produce", "producing": "produce",
    "production": "produce", "productions": "produce", "productive": "produce",
    "generate": "generate", "generates": "generate", "generated": "generate", "generating": "generate",
    "generation": "generate", "generations": "generate", "generative": "generate",
    # 要求・依存
    "require": "require", "requires": "require", "required": "require", "requiring": "require",
    "requirement": "require", "requirements": "require",
    "need": "need", "needs": "need", "needed": "need", "needing": "need",
    "depend": "depend", "depends": "depend", "depended": "depend", "depending": "depend",
    "dependent": "depend", "dependence": "depend", '依存': "depend", "dependencies": "depend",
    # 包含
    "contain": "contain", "contains": "contain", "contained": "contain", "containing": "contain",
    "containment": "contain",
    "include": "include", "includes": "include", "included": "include", "including": "include",
    "inclusion": "include", "inclusions": "include", "inclusive": "include",
    "comprise": "comprise", "comprises": "comprise", "comprised": "comprise", "comprising": "comprise",
    # 使用
    "use": "use", "uses": "use", "used": "use", "using": "use", "usage": "use",
    "utilize": "utilize", "utilizes": "utilize", "utilized": "utilize", "utilizing": "utilize",
    "utilization": "utilize", "utilisation": "utilize",
    "employ": "employ", "employs": "employ", "employed": "employ", "employing": "employ",
    "employment": "employ",
    # 防止
    "prevent": "prevent", "prevents": "prevent", "prevented": "prevent", "preventing": "prevent",
    "prevention": "prevent", "preventive": "prevent", "preventative": "prevent",
    "protect": "protect", "protects": "protect", "protected": "protect", "protecting": "protect",
    "protection": "protect", "protective": "protect",
    # 相関
    "associate": "associate", "associates": "associate", "associated": "associate", "associating": "associate",
    "association": "associate", "associations": "associate",
    "correlate": "correlate", "correlates": "correlate", "correlated": "correlate", "correlating": "correlate",
    "correlation": "correlate", "correlations": "correlate", "correlative": "correlate",
    "relate": "relate", "relates": "relate", "related": "relate", "relating": "relate",
    '関係': "relate", "relations": "relate", "relational": "relate",
    # 分野横断の一般関係
    "bind": "bind", "binds": "bind", "bound": "bind", "binding": "bind", "bindings": "bind",
    "interact": "interact", "interacts": "interact", "interacted": "interact", "interacting": "interact",
    "interaction": "interact", "interactions": "interact",
    "consist": "consist", "consists": "consist", "consisted": "consist", "consisting": "consist",
    "compose": "compose", "composes": "compose", "composed": "compose", "composing": "compose",
    "composition": "compose", "compositions": "compose",
    "belong": "belong", "belongs": "belong", "belonged": "belong", "belonging": "belong",
    "locate": "locate", "locates": "locate", "located": "locate", "locating": "locate",
    "location": "locate", "locations": "locate",
    "derive": "derive", "derives": "derive", "derived": "derive", "deriving": "derive",
    "derivation": "derive", "derivations": "derive",
}


_英語関係族 = {
    "因果": frozenset({"cause", "lead", '結果'}),
    "増加": frozenset({"increase", "raise", "enhance"}),
    "減少": frozenset({"decrease", "reduce", "lower"}),
    "阻害": frozenset({"inhibit", "suppress", "block"}),
    "活性化": frozenset({"activate", "stimulate"}),
    "生成": frozenset({"produce", "generate"}),
    "要求": frozenset({"require", "need", "depend"}),
    "包含": frozenset({"contain", "include", "comprise"}),
    "使用": frozenset({"use", "utilize", "employ"}),
    "防止": frozenset({"prevent", "protect"}),
    "相関": frozenset({"associate", "correlate", "relate"}),
    "結合": frozenset({"bind"}),
    "相互作用": frozenset({"interact"}),
    "構成": frozenset({"consist", "compose"}),
    "所属": frozenset({"belong"}),
    "位置": frozenset({"locate"}),
    "由来": frozenset({"derive"}),
}

_基本形から関係 = {
    lemma: kind
    for kind, lemmas in _英語関係族.items()
    for lemma in lemmas
}


def 英語基本形(word: str) -> str:
    value = unicodedata.normalize("NFKC", str(word)).casefold().strip("._-")
    return _英語基本形表.get(value, value)


def 英語関係概念(word: str) -> str | None:
    return _基本形から関係.get(英語基本形(word))


def 英語関係族() -> dict[str, frozenset[str]]:
    return dict(_英語関係族)


def 英語語形数() -> int:
    return len(_英語基本形表)


_主体 = r"(?P<s>[^?!.;,\n]{1,120}?)"
_OBJECT = r"(?P<o>[^?!.;,\n]{1,120})"
_AUX = r"(?:is|are|was|were|be|been|being|has\s+been|have\s+been|had\s+been)"


def _構文生成(種別: str, 述語式: str, *, 反転: bool = False) -> 英語関係構文:
    # 標準述語式は外側groupを参照しない。同じ式・flagsの不一致は完全式の不一致を含意する。
    完全式 = re.compile(rf"{_主体}\s+(?P<v>{述語式})\s+{_OBJECT}", re.I)
    return 英語関係構文(
        種別, 完全式, 反転,
        述語必要条件=re.compile(述語式, 完全式.flags),
    )


def _受動構文(種別: str, 語形式: str) -> 英語関係構文:
    return _構文生成(種別, rf"{_AUX}\s+(?:{語形式})\s+by", 反転=True)


# 高確度の明示構文だけを扱う。名詞共起や近接だけから関係を推定しない。
英語明示関係構文 = (
    _構文生成("因果", r"cause|causes|caused|causing|lead\s+to|leads\s+to|led\s+to|leading\s+to|result\s+in|results\s+in|resulted\s+in|resulting\s+in"),
    _受動構文("因果", r"caused"),
    _構文生成("増加", r"increase|increases|increased|increasing|raise|raises|raised|raising|enhance|enhances|enhanced|enhancing"),
    _受動構文("増加", r"increased|raised|enhanced"),
    _構文生成("減少", r"decrease|decreases|decreased|decreasing|reduce|reduces|reduced|reducing|lower|lowers|lowered|lowering"),
    _受動構文("減少", r"decreased|reduced|lowered"),
    _構文生成("阻害", r"inhibit|inhibits|inhibited|inhibiting|suppress|suppresses|suppressed|suppressing|block|blocks|blocked|blocking"),
    _受動構文("阻害", r"inhibited|suppressed|blocked"),
    _構文生成("活性化", r"activate|activates|activated|activating|stimulate|stimulates|stimulated|stimulating"),
    _受動構文("活性化", r"activated|stimulated"),
    _構文生成("生成", r"produce|produces|produced|producing|generate|generates|generated|generating"),
    _受動構文("生成", r"produced|generated"),
    _構文生成("要求", r"require|requires|required|requiring|need|needs|needed|needing|depend\s+on|depends\s+on|depended\s+on|depending\s+on"),
    _受動構文("要求", r"required|needed"),
    _構文生成("包含", r"contain|contains|contained|containing|include|includes|included|including|comprise|comprises|comprised|comprising"),
    _構文生成("使用", r"use|uses|used|using|utilize|utilizes|utilized|utilizing|employ|employs|employed|employing"),
    _受動構文("使用", r"used|utilized|employed"),
    _構文生成("防止", r"prevent|prevents|prevented|preventing|protect\s+against|protects\s+against|protected\s+against|protecting\s+against|protect\s+from|protects\s+from|protected\s+from|protecting\s+from"),
    _受動構文("防止", r"prevented|protected"),
    _構文生成("相関", r"associate\s+with|associates\s+with|associated\s+with|associating\s+with|correlate\s+with|correlates\s+with|correlated\s+with|correlating\s+with|relate\s+to|relates\s+to|related\s+to|relating\s+to"),
    _構文生成("結合", r"bind\s+to|binds\s+to|binding\s+to"),
    _構文生成("結合", r"is\s+bound\s+to|are\s+bound\s+to|was\s+bound\s+to|were\s+bound\s+to"),
    _構文生成("相互作用", r"interact\s+with|interacts\s+with|interacted\s+with|interacting\s+with"),
    _構文生成("構成", r"consist\s+of|consists\s+of|consisted\s+of|consisting\s+of"),
    _構文生成("構成", r"is\s+composed\s+of|are\s+composed\s+of|was\s+composed\s+of|were\s+composed\s+of"),
    _構文生成("所属", r"belong\s+to|belongs\s+to|belonged\s+to|belonging\s+to"),
    _構文生成("位置", r"is\s+located\s+in|are\s+located\s+in|was\s+located\s+in|were\s+located\s+in"),
    _構文生成("由来", r"derive\s+from|derives\s+from|derived\s+from|deriving\s+from"),
    _構文生成("由来", r"is\s+derived\s+from|are\s+derived\s+from|was\s+derived\s+from|were\s+derived\s+from"),
)


__all__ = [
    "英語関係構文",
    "英語関係一致",
    "英語基本形",
    "英語関係概念",
    "英語関係族",
    "英語語形数",
    "英語明示関係構文",
]

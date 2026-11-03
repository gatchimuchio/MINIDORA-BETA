from __future__ import annotations

from dataclasses import dataclass, replace
import re
import unicodedata
from typing import Sequence

from .HDS適合器 import HDS文脈
from .HDS中間表現 import HDSIR, HDS実行核, HDS座標, HDS関係, HDS残差, HDS意味作用, 値状態
from .意味字句 import 意味語
from .言語 import 自然言語器


_英字 = re.compile(r"[A-Za-z]")
_かな = re.compile(r"[ぁ-んァ-ヶー]")
_漢字 = re.compile(r"[一-龥々]")
_疑問 = re.compile(r"[?？]|(?:どれ|どの|何|なに|なぜ|どう|誰|いつ|どこ)|\b(?:which|what|why|how|who|when|where)\b", re.I)
_節分割 = re.compile(r"(?<=[。！？!?;；])\s*|\n+")
_語 = re.compile(r"[A-Za-z0-9_+./^%µμΩ°\-]+|[Α-Ωα-ωϐ-Ͽ]+|[ぁ-んァ-ヶー]+|[一-龥々]+")
_数量 = re.compile(
    r"(?<![A-Za-z0-9_.])"
    r"(?P<value>"
    r"(?:(?:[-+]?(?:\d+(?:\.\d+)?|\.\d+)\s*(?:[x×*]\s*)?)?10\s*\^\s*\{?\s*[-+]?\d+\s*\}?"
    r"|[-+]?(?:\d+(?:\.\d+)?|\.\d+)(?:[eE][-+]?\d+)?(?:/\d+(?:\.\d+)?)?)"
    r")"
    r"\s*(?P<unit>%|[A-Za-zµμΩ°][A-Za-z0-9µμΩ°/%^+\-]*)?"
)
_十冪数量 = re.compile(
    r"(?P<係数>[-+]?(?:\d+(?:\.\d+)?|\.\d+))?"
    r"\s*(?:[x×*]\s*)?10\s*\^\s*\{?\s*(?P<指数>[-+]?\d+)\s*\}?\Z"
)


def _数量値を正規化(raw: str) -> str:
    value = str(raw).strip()
    match = _十冪数量.fullmatch(value)
    if match:
        coefficient = match.group("係数") or "1"
        exponent = str(int(match.group("指数")))
        return coefficient + "e" + exponent
    return re.sub(r"\s+", "", value).lower()
_記号関係 = re.compile(
    r"(?P<s>[A-Za-zΑ-Ωα-ωϐ-Ͽ0-9_µμΩ.+\-]+)\s*"
    r"(?P<op>->|=>|→|⇒|>=|<=|≥|≤|!=|≠|>|<|=)\s*"
    r"(?P<o>[A-Za-zΑ-Ωα-ωϐ-Ͽ0-9_µμΩ.+\-]+)"
)

_英語関係規則 = (
    ("因果", re.compile(r"(?P<s>[^?!.;,]{1,160}?)\s+(?P<v>is\s+caused\s+by)\s+(?P<o>[^?!.;,]{1,160})", re.I), True),
    ("因果", re.compile(r"(?P<s>[^?!.;,]{1,160}?)\s+(?P<v>causes?|leads?\s+to|results?\s+in)\s+(?P<o>[^?!.;,]{1,160})", re.I), False),
    ("増加", re.compile(r"(?P<s>[^?!.;,]{1,160}?)\s+(?P<v>increases?|raises?|enhances?)\s+(?P<o>[^?!.;,]{1,160})", re.I), False),
    ("減少", re.compile(r"(?P<s>[^?!.;,]{1,160}?)\s+(?P<v>decreases?|reduces?|lowers?)\s+(?P<o>[^?!.;,]{1,160})", re.I), False),
    ("阻害", re.compile(r"(?P<s>[^?!.;,]{1,160}?)\s+(?P<v>inhibits?|suppresses?|blocks?)\s+(?P<o>[^?!.;,]{1,160})", re.I), False),
    ("活性化", re.compile(r"(?P<s>[^?!.;,]{1,160}?)\s+(?P<v>activates?|stimulates?)\s+(?P<o>[^?!.;,]{1,160})", re.I), False),
    ("生成", re.compile(r"(?P<s>[^?!.;,]{1,160}?)\s+(?P<v>produces?|generates?)\s+(?P<o>[^?!.;,]{1,160})", re.I), False),
    ("要求", re.compile(r"(?P<s>[^?!.;,]{1,160}?)\s+(?P<v>requires?|depends?\s+on)\s+(?P<o>[^?!.;,]{1,160})", re.I), False),
    ("包含", re.compile(r"(?P<s>[^?!.;,]{1,160}?)\s+(?P<v>contains?|includes?|comprises?)\s+(?P<o>[^?!.;,]{1,160})", re.I), False),
    ("使用", re.compile(r"(?P<s>[^?!.;,]{1,160}?)\s+(?P<v>uses?|utilizes?)\s+(?P<o>[^?!.;,]{1,160})", re.I), False),
    ("防止", re.compile(r"(?P<s>[^?!.;,]{1,160}?)\s+(?P<v>prevents?|protects?\s+against)\s+(?P<o>[^?!.;,]{1,160})", re.I), False),
    ("相関", re.compile(r"(?P<s>[^?!.;,]{1,160}?)\s+(?P<v>is\s+associated\s+with|correlates?\s+with)\s+(?P<o>[^?!.;,]{1,160})", re.I), False),
)

_日本語関係規則 = (
    ("因果", re.compile(r"(?P<s>[^。！？、]{1,120}?)(?:が|は)(?P<o>[^。！？、]{1,120}?)(?:を)?(?P<v>引き起こす|生じさせる|もたらす|原因となる)"), False),
    ("増加", re.compile(r"(?P<s>[^。！？、]{1,120}?)(?:が|は)(?P<o>[^。！？、]{1,120}?)(?:を)?(?P<v>増加させる|高める|促進する)"), False),
    ("減少", re.compile(r"(?P<s>[^。！？、]{1,120}?)(?:が|は)(?P<o>[^。！？、]{1,120}?)(?:を)?(?P<v>減少させる|低下させる|抑える)"), False),
    ("阻害", re.compile(r"(?P<s>[^。！？、]{1,120}?)(?:が|は)(?P<o>[^。！？、]{1,120}?)(?:を)?(?P<v>阻害する|抑制する|遮断する)"), False),
    ("活性化", re.compile(r"(?P<s>[^。！？、]{1,120}?)(?:が|は)(?P<o>[^。！？、]{1,120}?)(?:を)?(?P<v>活性化する|刺激する)"), False),
    ("生成", re.compile(r"(?P<s>[^。！？、]{1,120}?)(?:が|は)(?P<o>[^。！？、]{1,120}?)(?:を)?(?P<v>生成する|産生する|作る)"), False),
    ("要求", re.compile(r"(?P<s>[^。！？、]{1,120}?)(?:が|は)(?P<o>[^。！？、]{1,120}?)(?:を)?(?P<v>必要とする|依存する)"), False),
    ("包含", re.compile(r"(?P<s>[^。！？、]{1,120}?)(?:が|は)(?P<o>[^。！？、]{1,120}?)(?:を)?(?P<v>含む|包含する)"), False),
    ("使用", re.compile(r"(?P<s>[^。！？、]{1,120}?)(?:が|は)(?P<o>[^。！？、]{1,120}?)(?:を)?(?P<v>使う|使用する|利用する)"), False),
    ("防止", re.compile(r"(?P<s>[^。！？、]{1,120}?)(?:が|は)(?P<o>[^。！？、]{1,120}?)(?:を)?(?P<v>防ぐ|予防する)"), False),
)

_条件規則 = (
    re.compile(r"\b(?:if|when|given|assuming|unless)\b[^,.;!?]{1,180}", re.I),
    re.compile(r"\bunder\s+[^,.;!?]{1,160}", re.I),
    re.compile(r"\bin\s+the\s+(?:presence|absence)\s+of\s+[^,.;!?]{1,160}", re.I),
    re.compile(r"(?:もし|場合|とき|条件下|前提(?:として)?|ならば|なら)[^。！？、]{0,160}"),
)
_反転規則 = (
    re.compile(r"\b(?:except|incorrect|false|not\s+true|least\s+(?:likely|probable|expected|consistent|compatible|supported|plausible)|most\s+unlikely)\b", re.I),
    re.compile(r"(?:除く|誤っている|誤り|正しくない|不適切|最も可能性が低い|最も考えにくい)"),
)
_否定規則 = (
    re.compile(r"\b(?:not|no|never|without|cannot|can't|doesn't|does\s+not|isn't|is\s+not)\b", re.I),
    re.compile(r"(?:ない|なし|ではない|しない|不能|欠如|非)"),
)
_共参照 = re.compile(r"\b(?:it|this|that|these|those|they|them|former|latter)\b|(?:それ|これ|あれ|その|この|あの|前者|後者)")

_英語検索述語 = {
    "因果": "causes",
    "増加": "increases",
    "減少": "decreases",
    "阻害": "inhibits",
    "活性化": "activates",
    "生成": "produces",
    "要求": "requires",
    "包含": "contains",
    "使用": "uses",
    "防止": "prevents",
    "相関": "associated with",
}
_日本語検索述語 = {
    "因果": "引き起こす",
    "増加": "増加させる",
    "減少": "減少させる",
    "阻害": "阻害する",
    "活性化": "活性化する",
    "生成": "生成する",
    "要求": "必要とする",
    "包含": "含む",
    "使用": "使う",
    "防止": "防ぐ",
    "相関": "関連する",
}


def _未知端点(text: str) -> tuple[bool, str]:
    value = " ".join(str(text).split()).strip(" ,;:。！？?")
    if not value:
        return False, ""
    lowered = value.casefold()
    if lowered in {"who", "whom"}:
        return True, "person"
    if lowered in {"what", "which"}:
        return True, ""
    match = re.fullmatch(r"(?:which|what)\s+(?P<kind>.+)", value, flags=re.I)
    if match:
        kind = re.sub(r"^of\s+the\s+following\s+", "", match.group("kind"), flags=re.I).strip()
        return True, kind
    if value == "誰":
        return True, "人物"
    if value in {"何", "なに"}:
        return True, ""
    if value.startswith("どの") and len(value) > 2:
        return True, value[2:].strip()
    match = re.fullmatch(r"(?:何|なに)の(?P<kind>.+)", value)
    if match:
        return True, match.group("kind").strip()
    return False, ""


def _条件表層除去(text: str, conditions: tuple[str, ...]) -> str:
    value = " ".join(str(text).split()).strip()
    for condition in sorted((" ".join(c.split()).strip() for c in conditions if c), key=len, reverse=True):
        if value.casefold().endswith(condition.casefold()):
            value = value[: len(value) - len(condition)].strip(" ,;:。！？?")
    return value


def _関係検索述語(kind: str, surface: str, 言語: str, *, reverse: bool) -> str:
    if not reverse:
        return " ".join(str(surface).split()).strip()
    table = _日本語検索述語 if str(言語).casefold().startswith("ja") else _英語検索述語
    return table.get(kind, " ".join(str(surface).split()).strip())



@dataclass(frozen=True, slots=True)
class 公開HDSコンパイラ方針:
    基底言語: str = "ja"
    最大主題語数: int = 48
    最大関係数: int = 24
    多言語実務互換: bool = True


class 公開HDSコンパイラ:
    'MINIDORA公開標準HDS 構文化器。\n\n    HDS本体の導出規則ではなく、自然言語を公開HDS-IR契約へ有限射影する実装。\n    内部の役割名・状態名・関係名は日本語を正本とし、外部入力の表層語は検索・出典照合の\n    精度を落とさないため必要な範囲で原言語を保持する。\n    '

    並列安全 = True
    基底言語 = "ja"

    def __init__(self, 方針: 公開HDSコンパイラ方針 | None = None) -> None:
        self.方針 = 方針 or 公開HDSコンパイラ方針()
        self._legacy = 自然言語器()

    @staticmethod
    def _入力言語(text: str) -> str:
        kana = len(_かな.findall(text))
        latin = len(_英字.findall(text))
        han = len(_漢字.findall(text))
        if kana:
            return "ja"
        if han and not latin:
            return "zh"
        if latin:
            return "en"
        return "ja"

    @staticmethod
    def _正規化(text: str) -> str:
        value = unicodedata.normalize("NFKC", str(text))
        value = value.replace("⇒", "→").replace("=>", "→").replace("->", "→")
        value = value.replace("≧", "≥").replace("≦", "≤")
        return " ".join(value.split()).strip()

    @staticmethod
    def _焦点(text: str) -> str:
        parts = [part.strip(" ?？。!！") for part in _節分割.split(text) if part.strip()]
        focus = parts[-1] if parts else text.strip()
        focus = re.sub(r"^(?:which|what)\s+(?:of\s+the\s+following\s+)?", "", focus, flags=re.I)
        focus = re.sub(r"^(?:次のうち|以下のうち|どれが|どの)", "", focus)
        return " ".join(focus.split()).strip()

    @staticmethod
    def _ordered_terms(text: str) -> tuple[str, ...]:
        out: list[str] = []
        seen: set[str] = set()
        for 字句 in _語.findall(text):
            for term in sorted(意味語(字句)):
                key = term.casefold()
                if not term or key in seen:
                    continue
                seen.add(key)
                out.append(term)
        if not out:
            out.extend(sorted(意味語(text)))
        return tuple(out)

    def コンパイル(
        self,
        入力: str,
        *,
        前回結果: object = None,
        HDS履歴: tuple[HDSIR, ...] = (),
        文脈: HDS文脈 | None = None,
    ) -> HDSIR:
        raw = str(入力)
        normalized = self._正規化(raw)
        言語 = self._入力言語(normalized)
        coords: list[HDS座標] = [
            HDS座標("src", '情報源_text', raw, 由来='公開HDS 構文化器'),
            HDS座標("normalized", '言語.normalized', normalized, 由来='公開HDS 構文化器'),
            HDS座標('言語', "文脈.言語", 言語, 由来='公開HDS 構文化器'),
        ]
        relations: list[HDS関係] = []
        residuals: list[HDS残差] = []
        operations: list[HDS意味作用] = [
            HDS意味作用(
                "normalize",
                "表層正規化",
                ("src",),
                ("normalized",),
                "NFKC・空白・方向記号を正規化",
                保持構造=("原文", "入力言語"),
                検証=("原文保持",),
            )
        ]
        coordinate_index: dict[tuple[str, str], str] = {
            ('情報源_text', raw): "src",
            ('言語.normalized', normalized): "normalized",
            ("文脈.言語", 言語): '言語',
        }
        counters: dict[str, int] = {}

        def add_coord(
            kind: str,
            content: object,
            *,
            状態: 値状態 = 値状態.確定,
            origin: str = '公開HDS 構文化器',
        ) -> str:
            value = " ".join(str(content).split()).strip()
            key = (kind, value)
            if key in coordinate_index:
                return coordinate_index[key]
            prefix = kind.split(".", 1)[0]
            index = counters.get(prefix, 0)
            counters[prefix] = index + 1
            cid = f"{prefix}:{index}"
            coordinate_index[key] = cid
            coords.append(HDS座標(cid, kind, value, 状態, 由来=origin))
            return cid

        focus = self._焦点(normalized)
        if _疑問.search(normalized) and focus:
            add_coord("目的.検索焦点", focus)

        reverse_match = next((match for pattern in _反転規則 if (match := pattern.search(normalized))), None)
        add_coord("制御.選択意図", "反転" if reverse_match else "通常")
        if reverse_match is not None:
            add_coord("条件.検索極性", reverse_match.group(0))

        for pattern in _否定規則:
            match = pattern.search(normalized)
            if match:
                add_coord("状態.否定", match.group(0))
                break

        condition_surfaces: list[str] = []
        for pattern in _条件規則:
            for match in pattern.finditer(normalized):
                condition = " ".join(match.group(0).split()).strip()
                if not condition or condition in condition_surfaces:
                    continue
                condition_surfaces.append(condition)
                add_coord("条件.前提", condition)

        関係_count = 0

        def 関係を追加(kind: str, 主体: str, predicate_surface: str, object_: str, *, reverse: bool = False) -> None:
            nonlocal 関係_count
            if 関係_count >= self.方針.最大関係数:
                return
            主体 = _条件表層除去(主体, tuple(condition_surfaces)).strip(" ,;:。！？")
            object_ = _条件表層除去(object_, tuple(condition_surfaces)).strip(" ,;:。！？")
            predicate_surface = predicate_surface.strip()
            if not 主体 or not object_ or not predicate_surface:
                return
            if reverse:
                主体, object_ = object_, 主体

            主体_未知, 主体_type = _未知端点(主体)
            object_未知, object_type = _未知端点(object_)
            if 主体_未知 and object_未知:
                residuals.append(
                    HDS残差(
                        f"residual:relation:{関係_count}",
                        "未解関係両端",
                        f"{主体} {predicate_surface} {object_}",
                        "関係の始点と終点がともに未観測",
                        解消条件=("Rまたは文脈で少なくとも一方の端点を確定する",),
                    )
                )
                関係_count += 1
                return

            query_predicate = _関係検索述語(kind, predicate_surface, 言語, reverse=reverse)
            関係_conditions: list[str] = [f"検索述語={query_predicate}"]
            関係_状態 = 値状態.確定

            if 主体_未知:
                sid = add_coord("目的.未知始点", 主体_type or "未特定", 状態=値状態.未観測)
                add_coord("目的.不足位置", "始点")
                if 主体_type:
                    add_coord("目的.要求型", 主体_type)
                oid = add_coord("対象.終点", object_)
                関係_conditions.append("不足位置=始点")
                関係_状態 = 値状態.未観測
            elif object_未知:
                sid = add_coord("対象.始点", 主体)
                oid = add_coord("目的.未知終点", object_type or "未特定", 状態=値状態.未観測)
                add_coord("目的.不足位置", "終点")
                if object_type:
                    add_coord("目的.要求型", object_type)
                関係_conditions.append("不足位置=終点")
                関係_状態 = 値状態.未観測
            else:
                sid = add_coord("対象.始点", 主体)
                oid = add_coord("対象.終点", object_)

            add_coord("関係.述語", predicate_surface)
            relations.append(
                HDS関係(
                    f"rel:{関係_count}",
                    (sid,),
                    (oid,),
                    kind,
                    条件=tuple(関係_conditions),
                    値状態=関係_状態,
                    由来='公開HDS 構文化器',
                )
            )
            関係_count += 1

        symbol_map = {
            "→": "方向",
            ">": "比較.大",
            "<": "比較.小",
            ">=": "比較.以上",
            "≥": "比較.以上",
            "<=": "比較.以下",
            "≤": "比較.以下",
            "=": "等価",
            "!=": "不同",
            "≠": "不同",
        }
        for match in _記号関係.finditer(normalized):
            関係を追加(symbol_map[match.group("op")], match.group("s"), match.group("op"), match.group("o"))

        for kind, pattern, reverse in (*_英語関係規則, *_日本語関係規則):
            for match in pattern.finditer(normalized):
                関係を追加(kind, match.group("s"), match.group("v"), match.group("o"), reverse=reverse)

        for index, match in enumerate(_数量.finditer(normalized)):
            value_id = add_coord("値.数量", _数量値を正規化(match.group("value")))
            unit = match.group("unit")
            if unit:
                unit_id = add_coord("属性.単位", unit)
                relations.append(
                    HDS関係(
                        f"quantity:{index}",
                        (value_id,),
                        (unit_id,),
                        "数量単位",
                        値状態=値状態.確定,
                        由来='公開HDS 構文化器',
                    )
                )

        役割_terms: set[str] = set()
        for coord in coords:
            if coord.種別.startswith(("対象.", "関係.", "条件.", "状態.", "属性.", "値.", "目的.")):
                役割_terms.update(意味語(coord.内容))

        topic_count = 0
        for term in self._ordered_terms(normalized):
            if term in 役割_terms:
                continue
            add_coord("対象.主題語", term)
            topic_count += 1
            if topic_count >= self.方針.最大主題語数:
                break

        文脈_focus = getattr(文脈, "現在焦点", None) if 文脈 is not None else 前回結果
        coreference = _共参照.search(normalized)
        文脈_refs: tuple[str, ...] = ()
        if coreference:
            if 文脈_focus is not None:
                ref_id = add_coord("文脈.参照先", 文脈_focus, 状態=値状態.推定)
                pronoun_id = add_coord("文脈.指示語", coreference.group(0), 状態=値状態.推定)
                relations.append(
                    HDS関係(
                        '文脈:coreference',
                        (pronoun_id,),
                        (ref_id,),
                        "共参照",
                        値状態=値状態.推定,
                        由来='公開HDS 構文化器',
                    )
                )
                if 文脈 is not None:
                    文脈_refs = tuple(str(x) for x in getattr(文脈, "記憶引用", ()) if str(x))
            else:
                residuals.append(
                    HDS残差(
                        '残差:coreference',
                        "未解共参照",
                        coreference.group(0),
                        "参照先文脈が存在しない",
                        解消条件=("Trinity文脈で参照先を与える",),
                    )
                )

        operations.append(
            HDS意味作用(
                "project",
                "役割射影",
                ("normalized",),
                tuple(coord.座標ID for coord in coords if coord.座標ID not in {"src", "normalized"}),
                "対象・関係・状態・条件・目的・数量を有限射影",
                保持構造=("関係方向", "否定", "数量", "単位", "検索焦点", "不足スロット"),
                損失=tuple(item.理由 for item in residuals),
                検証=("ベンチ固有規則なし", "正解情報参照なし"),
            )
        )

        plan = self._legacy.計画(normalized, 文脈参照=文脈_focus)
        return HDSIR(
            原文=raw,
            正規化文=normalized,
            認知世界ID='minidora:public-hds-構文化器',
            座標=tuple(coords),
            関係=tuple(dict.fromkeys(relations)),
            残差=tuple(residuals),
            意味作用履歴=tuple(operations),
            実行核=HDS実行核(
                plan.種別,
                (),
                "結果",
                境界=("HDS-IR", "日本語基底"),
                検証=('公開構文化器',),
            ),
            初期状態=dict(plan.初期状態),
            参照必須=bool(plan.参照必須),
            種別=plan.種別,
            閉包状態="作用閉包",
            表現状態="構造化公開射影",
            保持状態="全領域有効",
            暫定性状態="原則暫定",
            手順=plan.手順,
            入力言語=言語,
            出力言語=言語,
            文脈引用=文脈_refs,
        )

    def 問題IR(self, question: str, choices: Sequence[str]) -> HDSIR:
        if len(choices) < 2:
            raise ValueError("選択問題には2件以上の候補が必要")
        if len(choices) > 26:
            raise ValueError('公開構文化器の選択ラベル上限は26件')
        base = self.コンパイル(question)
        選択肢_coords = tuple(
            HDS座標(
                f"選択肢:{chr(ord('A') + index)}",
                "目的.候補",
                str(text),
                値状態.確定,
                由来="選択問題入力",
            )
            for index, text in enumerate(choices)
        )
        return replace(
            base,
            座標=base.座標 + 選択肢_coords,
            参照必須=True,
            種別="knowledge_query",
            実行核=HDS実行核(
                'HDS_選択肢_selection',
                (),
                "結果",
                境界=("NO_GUESS", "gold非参照"),
                検証=("全候補対称",),
            ),
            手順=None,
            閉包状態='CLOSED_FOR_意味_TRANSFER',
        )


# 旧実装は現行Kernelの意味抽出前段としてだけ保持する。公開入口としては使わない。
公開HDS基礎コンパイラ = 公開HDSコンパイラ

def 公開HDSコンパイラ(*args, **kwargs):
    """互換入口。公開Compilerは常に現行MINIDORA Kernelを返す。"""
    from .HDS構文化器_v1 import 公開HDSコンパイラ as 現行HDSコンパイラ
    return 現行HDSコンパイラ(*args, **kwargs)

__all__ = ["公開HDSコンパイラ方針", "公開HDS基礎コンパイラ", "公開HDSコンパイラ"]

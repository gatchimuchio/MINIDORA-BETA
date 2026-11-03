from __future__ import annotations

from dataclasses import replace
import re

from .HDS構文化記録_v1_1 import HDS暗黙知記録
from .HDS中間表現 import HDSIR, HDS座標, HDS関係, 値状態


_JA_DEF = (
    re.compile(r"(?P<s>[^。！？、]{1,80}?)とは(?P<o>[^。！？]{1,140}?)(?:を指す|である|という意味(?:である)?)(?:。|$)"),
    re.compile(r"(?P<s>[^。！？、]{1,80}?)を(?P<o>[^。！？]{1,140}?)と定義する"),
)
_EN_DEF = (
    re.compile(r"(?P<s>[^?!.;,]{1,100}?)\s+(?:is\s+)?defined\s+as\s+(?P<o>[^?!.;]{1,160})", re.I),
    re.compile(r"(?P<s>[^?!.;,]{1,100}?)\s+(?:means|refers\s+to)\s+(?P<o>[^?!.;]{1,160})", re.I),
)
_JA_PREMISE = (
    re.compile(r"(?P<body>[^。！？]{1,180}?)(?:と仮定する|を前提とする|を前提にする)"),
)
_EN_PREMISE = (
    re.compile(r"(?:assume|assuming|suppose|given that)\s+(?P<body>[^?!.;]{1,180})", re.I),
)
_JA_範囲 = (
    re.compile(r"(?P<body>[^。！？]{1,180}?)(?:に限る|のみ有効|のみ適用|条件下のみ|対象外|非適用)"),
)
_EN_範囲 = (
    re.compile(r"(?P<body>[^?!.;]{1,180}?)(?:only|limited to|applies only to|out of scope|not applicable)", re.I),
)
_UNCERTAINTY_MARKERS = (
    ("仮説", re.compile(r"(?:仮説|hypothesis)", re.I)),
    ("予測", re.compile(r"(?:予測|forecast|prediction|predict)", re.I)),
    ("推定", re.compile(r"(?:推定|estimate|estimated|inference)", re.I)),
    ("可能性", re.compile(r"(?:可能性|かもしれ|may|might|could|likely|unlikely)", re.I)),
    ("印象", re.compile(r"(?:印象|impression)", re.I)),
    ("願望", re.compile(r"(?:願望|希望|wish|hope)", re.I)),
    ("未確定", re.compile(r"(?:不確実|未確定|暫定|uncertain|provisional)", re.I)),
)


def _clean(value: str | None) -> str | None:
    if value is None:
        return None
    text = " ".join(str(value).split()).strip(" ,、:;。！？?!")
    return text or None


def HDS暗黙知抽出(text: str) -> tuple[HDS暗黙知記録, ...]:
    情報源 = " ".join(str(text).split()).strip()
    records: list[HDS暗黙知記録] = []

    for pattern in (*_JA_DEF, *_EN_DEF):
        for match in pattern.finditer(情報源):
            主体 = _clean(match.group("s"))
            content = _clean(match.group("o"))
            if 主体 and content:
                records.append(
                    HDS暗黙知記録(
                        f"tacit:def:{len(records):03d}",
                        "定義",
                        主体,
                        content,
                        再開放条件=("定義変更・別文脈・反例で再監査する",),
                    )
                )

    for pattern in (*_JA_PREMISE, *_EN_PREMISE):
        for match in pattern.finditer(情報源):
            body = _clean(match.group("body"))
            if body:
                records.append(
                    HDS暗黙知記録(
                        f"tacit:premise:{len(records):03d}",
                        "前提",
                        None,
                        body,
                        分類="明示前提",
                        再開放条件=("前提撤回・観測更新で再監査する",),
                    )
                )

    for pattern in (*_JA_範囲, *_EN_範囲):
        for match in pattern.finditer(情報源):
            body = _clean(match.group("body"))
            if body:
                records.append(
                    HDS暗黙知記録(
                        f"tacit:scope:{len(records):03d}",
                        "射程",
                        None,
                        body,
                        分類="限定射程",
                        適用範囲=(body,),
                        再開放条件=("対象・時間・条件境界の変更で再監査する",),
                    )
                )

    for classification, pattern in _UNCERTAINTY_MARKERS:
        for match in pattern.finditer(情報源):
            sentence_start = max(情報源.rfind("。", 0, match.start()), 情報源.rfind(".", 0, match.start())) + 1
            ends = [pos for pos in (情報源.find("。", match.end()), 情報源.find(".", match.end())) if pos >= 0]
            sentence_end = min(ends) if ends else len(情報源)
            body = _clean(情報源[sentence_start:sentence_end])
            if body:
                records.append(
                    HDS暗黙知記録(
                        f"tacit:uncertainty:{len(records):03d}",
                        "不確実性",
                        None,
                        body,
                        分類=classification,
                        不確実性=classification,
                        再開放条件=("証拠強度・観測条件・時点変更で再監査する",),
                    )
                )

    dedup: list[HDS暗黙知記録] = []
    seen: set[tuple[str, str | None, str, str | None]] = set()
    for record in records:
        key = (record.種別, record.主語, record.内容, record.分類)
        if key in seen:
            continue
        seen.add(key)
        dedup.append(record)
    return tuple(dedup)


def HDS暗黙知IR射影(ir: HDSIR, records: tuple[HDS暗黙知記録, ...]) -> HDSIR:
    if not records:
        return ir
    coords = list(ir.座標)
    relations = list(ir.関係)
    existing = {(str(coord.種別), str(coord.内容)): coord.座標ID for coord in coords}

    def add(kind: str, content: str, *, 状態: 値状態 = 値状態.確定) -> str:
        key = (kind, content)
        cid = existing.get(key)
        if cid is not None:
            return cid
        cid = f"archv11:tacit:{len(existing):03d}"
        coords.append(HDS座標(cid, kind, content, 状態, 由来='公開HDS 構文化器 v1.1', 再開放条件=("新観測・文脈変更で再監査する",)))
        existing[key] = cid
        return cid

    for record in records:
        if record.種別 == "定義" and record.主語:
            sid = add("暗黙知.定義対象", record.主語)
            oid = add("暗黙知.定義内容", record.内容)
            relations.append(HDS関係(f"archv11:{record.記録ID}", (sid,), (oid,), "定義", 値状態=値状態.確定, 由来='公開HDS 構文化器 v1.1'))
        elif record.種別 == "前提":
            add("暗黙知.前提", record.内容, 状態=値状態.推定)
        elif record.種別 == "射程":
            add("暗黙知.射程", record.内容, 状態=値状態.確定)
        elif record.種別 == "不確実性":
            add("暗黙知.不確実性", record.分類 or record.内容, 状態=値状態.推定)

    return replace(ir, 座標=tuple(coords), 関係=tuple(dict.fromkeys(relations)))


__all__ = ["HDS暗黙知抽出", "HDS暗黙知IR射影"]

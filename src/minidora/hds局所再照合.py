from __future__ import annotations

from dataclasses import dataclass
import re
import unicodedata
from typing import Iterable, Sequence

from .HDS中間表現 import HDSIR
from .意味字句 import 意味語
from .参照 import 参照記録


_文分割 = re.compile(r"(?<=[.!?。！？;；])\s+|\n+")


@dataclass(frozen=True, slots=True)
class HDS局所Window:
    参照: 参照記録
    内容: str
    問い一致数: int
    候補差分一致数: int
    候補一致数: int
    順位値: tuple[int, int, int, int, str]


def _normalize(text: object) -> str:
    return " ".join(unicodedata.normalize("NFKC", str(text)).split()).strip()


def _choices(ir: HDSIR) -> tuple[tuple[str, str], ...]:
    out: list[tuple[str, str]] = []
    for coord in ir.座標:
        if coord.座標ID.startswith('選択肢:'):
            out.append((coord.座標ID.split(":", 1)[1], str(coord.内容)))
    return tuple(sorted(out))


def _distinctive(choices: Sequence[tuple[str, str]]) -> dict[str, frozenset[str]]:
    signatures = {label: 意味語(text) for label, text in choices}
    out: dict[str, frozenset[str]] = {}
    for label, _text in choices:
        others: set[str] = set()
        for other, terms in signatures.items():
            if other != label:
                others.update(terms)
        diff = signatures[label] - others
        out[label] = diff or signatures[label]
    return out


def _segments(text: str) -> tuple[str, ...]:
    raw = _normalize(text)
    if not raw:
        return ()
    base = [segment.strip() for segment in _文分割.split(raw) if len(segment.strip()) >= 24]
    if not base:
        return ()
    out: list[str] = []
    seen: set[str] = set()
    for index, segment in enumerate(base):
        candidates = [segment]
        if index + 1 < len(base):
            candidates.append(segment + " " + base[index + 1])
        for value in candidates:
            value = value[:1200].strip()
            key = value.casefold()
            if len(value) < 24 or key in seen:
                continue
            seen.add(key)
            out.append(value)
    return tuple(out)


def HDS局所Window候補(
    question_ir: HDSIR,
    references: Iterable[参照記録],
    *,
    上限: int = 12,
) -> tuple[HDS局所Window, ...]:
    '問題語と候補差分語が同じ局所窓に残る箇所を、候補対称に選ぶ。\n\n    ここでは真偽を決めない。window選択にgold・候補ラベルの優劣・domain規則を使わず、\n    全候補の差分語集合を対称に扱う。元情報源 identityは参照記録をそのまま保持する。\n    全文そのものと同一のwindowは再解析対象にしない。\n    '
    if 上限 <= 0:
        return ()
    choices = _choices(question_ir)
    if len(choices) < 2:
        return ()
    distinctive = _distinctive(choices)
    question_terms = 意味語(question_ir.原文)
    if not question_terms:
        return ()

    rows: list[HDS局所Window] = []
    for record in references:
        full = _normalize(record.内容)
        for segment in _segments(record.内容):
            if segment.casefold() == full.casefold():
                continue
            terms = 意味語(segment)
            if not terms:
                continue
            q_hits = len(question_terms & terms)
            if q_hits <= 0:
                continue
            label_hits: list[tuple[str, int]] = []
            total_distinctive = 0
            for label, _選択肢 in choices:
                count = len(distinctive[label] & terms)
                if count > 0:
                    label_hits.append((label, count))
                    total_distinctive += count
            if not label_hits:
                continue

            # 一候補だけへ局所化するwindowを優先するが、多候補windowも捨てず大域再照合へ残す。
            候補_count = len(label_hits)
            specificity = 2 if 候補_count == 1 else 1
            rank = (
                specificity,
                total_distinctive,
                q_hits,
                int(round(float(record.信頼) * 1000)),
                str(record.識別子),
            )
            rows.append(
                HDS局所Window(
                    record,
                    segment,
                    q_hits,
                    total_distinctive,
                    候補_count,
                    rank,
                )
            )

    # まずsource多様性を確保し、その後同sourceの二番手以降を埋める。
    rows.sort(key=lambda item: (-item.順位値[0], -item.順位値[1], -item.順位値[2], -item.順位値[3], item.順位値[4], item.内容))
    selected: list[HDS局所Window] = []
    used_text: set[tuple[str, str]] = set()
    used_情報源: set[str] = set()
    for pass_no in (0, 1):
        for row in rows:
            if len(selected) >= 上限:
                break
            情報源 = str(row.参照.識別子)
            key = (情報源, row.内容.casefold())
            if key in used_text:
                continue
            if pass_no == 0 and 情報源 in used_情報源:
                continue
            if pass_no == 1 and 情報源 not in used_情報源:
                continue
            selected.append(row)
            used_text.add(key)
            used_情報源.add(情報源)
        if len(selected) >= 上限:
            break
    return tuple(selected)


__all__ = ["HDS局所Window", "HDS局所Window候補"]

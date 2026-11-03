from __future__ import annotations

from dataclasses import dataclass
import re
import unicodedata


_SPLIT = re.compile(r"(?<=[?!.。？！])\s+|\n+")
_EXCEPTION_PATTERNS = (
    re.compile(r"\bexcept\b", re.I),
    re.compile(r"\bwhich\b.{0,120}\b(?:is|are)\s+(?:incorrect|false)\b", re.I),
    re.compile(r"\bwhich\b.{0,120}\bincorrectly\s+(?:describes|states|identifies|matches|predicts|assigns|pairs)\b", re.I),
    re.compile(r"\bwhich\b.{0,120}\b(?:does|do|is|are|will|would|can)\s+not\b", re.I),
    re.compile(r"\bwhich\b.{0,120}\bcannot\b", re.I),
    re.compile(r"\b(?:select|choose|identify)\b.{0,100}\b(?:incorrect|false)\s+(?:statement|option|claim|answer|choice)\b", re.I),
    re.compile(r"\bleast\s+(?:likely|probable|expected|consistent|compatible|supported|plausible)\b", re.I),
    re.compile(r"\bmost\s+unlikely\b", re.I),
    re.compile(r"(?:誤っている|正しくない|該当しない|当てはまらない|許可されない|認められない)(?:もの|選択肢|記述|文|項目)?(?:は)?(?:どれ|どの|何れ)", re.I),
    re.compile(r"(?:除く|以外)", re.I),
    re.compile(r"(?:最も|もっとも)(?:可能性が低い|起こりにくい|考えにくい|ありそうにない|整合しない|適合しない|支持されない)", re.I),
)


@dataclass(frozen=True, slots=True)
class HDS選択意図:
    種別: str
    焦点: str
    根拠: tuple[str, ...] = ()


def _焦点(text: str) -> str:
    raw = unicodedata.normalize("NFKC", str(text)).strip()
    if not raw:
        return ""
    segments = [segment.strip() for segment in _SPLIT.split(raw) if segment.strip()]
    if not segments:
        return raw
    # 採否方向は背景説明から伝染させず、最後の実質問文だけで決める。
    for segment in reversed(segments):
        if "?" in segment or "？" in segment:
            return segment[-800:]
    return segments[-1][-800:]


def HDS選択意図判定(text: str) -> HDS選択意図:
    """選択問題の採否方向を、最終問いの選択述語へ結び付く表層論理だけから判定する。

    ベンチ名・分野・正解候補は参照しない。`false` / `incorrect` / `not` が質問中の
    対象内容を修飾するだけでは反転させず、「どの候補が誤りか」という選択方向に結び付く場合だけ
    `EXCEPTION` とする。`least likely` 系は他候補より成立しにくい1件を求めるため同じ扱いにする。
    単なる `least` / `smallest` / `minimum` は数値比較と混同するため拾わない。
    """
    focus = _焦点(text)
    matched = tuple(pattern.pattern for pattern in _EXCEPTION_PATTERNS if pattern.search(focus))
    if matched:
        return HDS選択意図("EXCEPTION", focus, matched)
    return HDS選択意図("POSITIVE", focus, ())


__all__ = ["HDS選択意図", "HDS選択意図判定"]

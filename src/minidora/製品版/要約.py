from __future__ import annotations
from collections import Counter
import re
from .型 import 能力結果, 参照資料

要約版 = 'extractive-要約-v2'
_SENTENCE = re.compile(r"(?<=[。！？!?])\s+|\n+")
_WORD = re.compile(r"[一-龥ぁ-んァ-ヶA-Za-z0-9]{2,}")
_STOP = {"これ", "それ", "ため", "こと", "もの", "よう", "です", "ます", "した", "する", "いる", "ある", "から", "まで", "について", "として"}

def _sentences(text: str) -> list[str]:
    text = re.sub(r"\s+", " ", text.strip())
    parts = [x.strip() for x in _SENTENCE.split(text) if x.strip()]
    if len(parts) == 1 and "。" in text:
        parts = [x.strip()+"。" for x in text.split("。") if x.strip()]
    return parts

def _rank(text: str, limit: int) -> list[str]:
    sents = _sentences(text)
    if len(sents) <= limit:
        return sents
    words = [w.casefold() for w in _WORD.findall(text) if w.casefold() not in _STOP]
    freq = Counter(words)
    scored = []
    for i, s in enumerate(sents):
        ws = [w.casefold() for w in _WORD.findall(s) if w.casefold() not in _STOP]
        base = sum(freq[w] for w in ws) / max(len(ws), 1)
        pos = 1.2 if i == 0 else 1.0
        length_penalty = 0.75 if len(s) > 180 else 1.0
        scored.append((base * pos * length_penalty, i, s))
    top = sorted(scored, reverse=True)[:limit]
    return [s for _, _, s in sorted(top, key=lambda x: x[1])]

class 汎用要約モジュール:
    版 = 要約版
    def 実行(self, text: str, *, 行数: int = 3, 参照: tuple[参照資料, ...] = ()) -> 能力結果:
        if not text.strip():
            return 能力結果(False, "", 保留理由="要約対象がない")
        lines = _rank(text, max(1, min(8, 行数)))
        body = "\n".join(f"- {s}" for s in lines) if len(lines) > 1 else (lines[0] if lines else "")
        return 能力結果(True, body, 根拠=("入力文から抽出要約",), 参照=参照, データ={"抽出文数": len(lines), "入力文字数": len(text)})

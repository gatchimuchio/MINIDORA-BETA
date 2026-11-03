from __future__ import annotations
from collections import Counter
import re
from .型 import 能力結果

抽出版 = "information-extraction-v1"
_WORD = re.compile(r"[一-龥ぁ-んァ-ヶA-Za-z][一-龥ぁ-んァ-ヶA-Za-z0-9_-]{1,}")
_URL = re.compile(r"https?://[^\s)\]}>]+")
_NUM = re.compile(r"(?<![A-Za-z])[-+]?\d+(?:\.\d+)?%?")
_STOP = {"これ","それ","です","ます","する","した","ある","いる","ため","こと","もの","よう","から","まで","について","として","ユーザー","minidora"}

class 情報抽出モジュール:
    版 = 抽出版
    def 実行(self, instruction: str, 情報源: str) -> 能力結果:
        if not 情報源.strip():
            return 能力結果(False, "", 保留理由="抽出対象がない")
        c = instruction.casefold()
        if "url" in c or "リンク" in c:
            vals = _URL.findall(情報源)
            return 能力結果(True, "\n".join(vals) if vals else "URLは見つかりませんでした。", 根拠=("正規表現抽出",), データ={"件数":len(vals)})
        if "数字" in c:
            vals = _NUM.findall(情報源)
            return 能力結果(True, "、".join(vals) if vals else "数値は見つかりませんでした。", 根拠=("数値抽出",), データ={"件数":len(vals)})
        words = [w for w in _WORD.findall(情報源) if w.casefold() not in _STOP and len(w) >= 2]
        vals = [w for w, _ in Counter(words).most_common(8)]
        return 能力結果(True, "、".join(vals), 根拠=("頻度ベースのキーワード抽出",), データ={"件数":len(vals)})

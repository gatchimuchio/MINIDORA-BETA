from __future__ import annotations
import re
from .型 import 能力結果

変換版 = '文脈-transform-v1'

class 文脈変換モジュール:
    版 = 変換版
    def 実行(self, instruction: str, 情報源: str) -> 能力結果:
        if not 情報源.strip():
            return 能力結果(False, "", 保留理由="変換対象がない")
        c = re.sub(r"\s+", "", instruction).casefold()
        if any(x in c for x in ("箇条書き", "リスト")):
            chunks = [x.strip() for x in re.split(r"[。\n]+", 情報源) if x.strip()]
            return 能力結果(True, "\n".join(f"- {x}" for x in chunks[:12]), 根拠=("直前応答の再整形",), データ={"形式":"箇条書き"})
        if any(x in c for x in ("短く", "簡潔")):
            txt = 情報源.strip()
            if len(txt) <= 220:
                return 能力結果(True, txt, 根拠=("既に短い",), データ={"形式":"短文化"})
            cut = txt[:220]
            cut = cut.rsplit("。", 1)[0] + "。" if "。" in cut else cut
            return 能力結果(True, cut, 根拠=("直前応答のみを短文化",), データ={"形式":"短文化"})
        return 能力結果(False, "", 保留理由="対応する変換規則がない")

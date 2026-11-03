from __future__ import annotations
from .型 import 能力結果

基本会話版 = "basic-chat-v3"
class 基本会話モジュール:
    版 = 基本会話版
    def 実行(self, text: str) -> 能力結果:
        c = text.replace(" ","").casefold()
        if any(x in c for x in ("こんにちは","おはよう","こんばんは")):
            return 能力結果(True, "こんにちは。MINIDORAです。質問、Web検索、要約、ニュース取得、情報抽出、計算などを処理できます。")
        if "ありがとう" in c:
            return 能力結果(True, "どういたしまして。")
        if any(x in c for x in ("君は誰","あなたは誰","何者")):
            return 能力結果(True, 'MINIDORAです。日本語基底の非ニューラルLLM 模型核に、交換可能な能力モジュールを接続して動作します。')
        if any(x in c for x in ("何ができる","できること")):
            return 能力結果(True, '現在の製品版では、通常会話、ニュース参照、ローカルSearXNG経由のWeb検索、要約、文脈変換、情報抽出、計算、Wikipedia知識参照、既存MINIDORA 模型核への一般質問委譲を扱います。各応答は実行経路を監査できます。')
        return 能力結果(False, "", 保留理由="基本会話非該当")

"""統合された成功成果を、既存の会話照応と実HDSの要求計画へ接続する。"""
from __future__ import annotations

from .文脈照応 import 会話参照記憶
from .要求解釈 import 要求計画器
from .HDS構文化器 import 公開HDSコンパイラ
from .HDS中間表現 import HDSIR
from .多言語変換 import 対訳を変換


def 依頼を準備(original, materials, 言語, session, history, maximum):
    if type(original) is not str or not original.strip() or len(original) > 8192:
        raise ValueError("依頼文不正")
    if 言語 not in ("ja", "en"):
        raise ValueError("未対応言語")
    detail = {"原文": original, "入力言語": 言語, "翻訳": None, "解釈": None}
    text = original
    if 言語 == "en":
        translation = 対訳を変換(original, "en", "ja", 種別="文書依頼")
        detail["翻訳"] = translation
        if not translation.成立:
            return None, None, detail
        text = translation.本文
    # 成功した成果だけから既存会話状態を再構築する。擬似Compilerを作らない。
    memory = 会話参照記憶(session, 最大応答数=maximum)
    for row in history:
        memory.更新(memory.起点(), row["依頼"], "合格", 出力=row["出力"], 実行ハッシュ=row["実行ハッシュ"])
    snapshot = memory.起点()
    ir = 公開HDSコンパイラ().コンパイル(text, 文脈=snapshot.HDS文脈へ(), HDS履歴=snapshot.局所起点.IR履歴)
    if not isinstance(ir, HDSIR) or ir.原文 != text:
        raise ValueError("実HDS原文の不一致")
    interpreted = 要求計画器().コンパイル(ir, {} if materials is None else materials, 文脈=snapshot)
    detail["解釈"] = interpreted
    return interpreted, snapshot, detail

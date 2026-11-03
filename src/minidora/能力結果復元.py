"""JSON互換の能力結果を復元する。特定分野の推論器をロードしない。

応答構成.pyの既存復元契約を移植。成功・保留の矛盾も既存合成境界で拒否する。
"""
from __future__ import annotations
from copy import deepcopy
from datetime import datetime
from .能力合成 import _結果辞書
from .製品版.型 import 能力結果, 参照資料


def 能力結果を復元(raw: dict) -> 能力結果:
    if type(raw) is not dict or set(raw) != {"成立", "本文", "根拠", "参照", "データ", "保留理由"}:
        raise ValueError("能力結果の項目不一致")
    if type(raw["参照"]) not in (list, tuple) or type(raw["根拠"]) not in (list, tuple):
        raise ValueError("能力結果の配列型不正")
    refs = []
    for item in raw["参照"]:
        if type(item) is not dict or set(item) != set(参照資料.__dataclass_fields__):
            raise ValueError("参照資料の項目不一致")
        item = dict(item)
        if item["公開時刻"] is not None:
            if type(item["公開時刻"]) is not str:
                raise ValueError("公開時刻型不正")
            item["公開時刻"] = datetime.fromisoformat(item["公開時刻"])
        refs.append(参照資料(**item))
    結果 = 能力結果(raw["成立"], raw["本文"], tuple(raw["根拠"]), tuple(refs),
                        deepcopy(raw["データ"]), raw["保留理由"])
    _結果辞書(結果)
    return 結果

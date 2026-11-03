"""静的本文取得の記録にある情報欠落を、命題化の前に検査する。

本文hashの一致は元ページの意味網羅性ではない。未記録の欠落を保証しない。
この検査はネットワークを使わず、読取権限を付与しない。
"""
from __future__ import annotations
from hashlib import sha256
from .公開本文取得 import 取得本文

# 実行コード・装飾・文書メタデータの除外は既存の静的本文契約の範囲内。
# 表・図・未閉鎖域など、意味を持ち得る除外域は無関係と決め付けない。
_静的本文契約内 = frozenset({'head', 'script', 'style'})


def 取得本文の意味境界を検査(本文: 取得本文) -> None:
    if type(本文) is not 取得本文:
        raise ValueError('取得本文の型不正')
    if (type(本文.本文) is not str or not 本文.本文.strip()
            or len(本文.本文) > 32000
            or sha256(本文.本文.encode('utf-8')).hexdigest() != 本文.本文SHA256):
        raise ValueError('意味入力の本文・hash・長さ不正')
    if (type(本文.除外要素) is not tuple
            or any(type(x) is not str for x in 本文.除外要素)):
        raise ValueError('除外記録の型不正')
    欠落 = sorted(set(本文.除外要素) - _静的本文契約内)
    if 欠落:
        raise ValueError('取得意味欠落:' + ','.join(欠落) + '。完全な資料として命題判定しない')

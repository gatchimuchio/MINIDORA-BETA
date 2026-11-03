"""互換入口。旧多言語表面化の公開経路を保持する。"""
from __future__ import annotations
from typing import Any

def 表面化(値: Any, 状態: str, 理由: tuple[str, ...], 言語: str = "ja") -> str:
    from .出力系.互換 import 既存多言語値を表現
    return 既存多言語値を表現(値, 状態, 理由, 言語)

__all__ = ["表面化"]

"""旧名互換。active pathは .適応記憶.HDS適応記憶 を使用する。"""
from .適応記憶 import HDS適応記憶

HDS一時適応キャッシュ = HDS適応記憶

__all__ = ["HDS適応記憶", "HDS一時適応キャッシュ"]

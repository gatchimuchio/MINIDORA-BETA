"""互換入口の実表現本体。旧意味採否・語尾・長さ境界を変更しない。"""
from __future__ import annotations
from typing import Any
from ..コア.内容計画 import 内容計画, 内容計画を検査


def 既存内容計画を表現(計画: 内容計画, *, 詳細=False, 最大文字数=100000) -> str:
    if type(詳細) is not bool or type(最大文字数) is not int or not 1 <= 最大文字数 <= 100000:
        raise ValueError("内容表現条件不正")
    if not 内容計画を検査(計画):
        raise ValueError("内容計画不整合")
    parts = []
    for unit in 計画.単位:
        parts.append(unit.本文)
        parts.extend(unit.条件)
    parts.extend(計画.留保)
    if 詳細:
        parts.append("根拠・由来：\n" + "\n".join(計画.由来 or ("外部由来なし。",)))
    text = "\n".join(parts)
    if len(text) > 最大文字数:
        raise ValueError("必須内容を切断せず保留する")
    return text


def 既存日本語値を表現(値: Any, 状態: str, 理由: tuple[str, ...]) -> str:
    if 状態 == "保留":
        if "未解消矛盾" in 理由:
            return "判断を保留します。未解消の矛盾があります。"
        return "分かりません。確認できる根拠がありません。"
    if 状態 == "失敗":
        return "処理できません。"
    if 値 is None:
        return "分かりません。"
    if isinstance(値, bool):
        return "はい。" if 値 else "いいえ。"
    if isinstance(値, float) and 値.is_integer():
        値 = int(値)
    if isinstance(値, (int, float)):
        return f"{値}です。"
    if isinstance(値, str):
        return 値 if 値.endswith(("。", "！", "？", "!", "?")) else f"{値}。"
    return f"{値}。"


def 既存多言語値を表現(値: Any, 状態: str, 理由: tuple[str, ...], 言語: str = "ja") -> str:
    言語 = (言語 or "ja").casefold()
    if 言語.startswith("en"):
        if 状態 == "保留":
            return "I don't know. I don't have verified grounds."
        if 状態 == "失敗":
            return "I can't process that."
        if 値 is None:
            return "I don't know."
        if isinstance(値, bool):
            return "Yes." if 値 else "No."
        if isinstance(値, float) and 値.is_integer():
            値 = int(値)
        if isinstance(値, str) and 値.endswith((".", "!", "?")):
            return 値
        return f"{値}."

    if 言語.startswith("zh"):
        if 状態 == "保留":
            return "不知道。没有可确认的依据。"
        if 状態 == "失敗":
            return "无法处理。"
        if 値 is None:
            return "不知道。"
        if isinstance(値, bool):
            return "是。" if 値 else "否。"
        if isinstance(値, float) and 値.is_integer():
            値 = int(値)
        if isinstance(値, str) and 値.endswith(("。", "！", "？", "!", "?")):
            return 値
        return f"{値}。"

    if 状態 == "保留":
        return "判断を保留します。未解消の矛盾があります。" if "未解消矛盾" in 理由 else "分かりません。確認できる根拠がありません。"
    if 状態 == "失敗":
        return "処理できません。"
    if 値 is None:
        return "分かりません。"
    if isinstance(値, bool):
        return "はい。" if 値 else "いいえ。"
    if isinstance(値, float) and 値.is_integer():
        値 = int(値)
    if isinstance(値, (int, float)):
        return f"{値}です。"
    if isinstance(値, str):
        return 値 if 値.endswith(("。", "！", "？", "!", "?")) else f"{値}。"
    return f"{値}。"

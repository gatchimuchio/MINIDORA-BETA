"""運用状態の有限な交換表現。実行コード・秘密・任意型を保存しない。"""
from __future__ import annotations
from copy import deepcopy
from dataclasses import asdict, replace
from hashlib import sha256
import json
from ..能力合成 import 合成計画, 合成工程, 素材参照, _結果辞書, _符号化
from ..能力結果復元 import 能力結果を復元
from ..採否 import 実行状態
from ..製品版.型 import 能力結果

運用版 = "HDS-MINIDORA-全体運用-v8"


def 正準(value):
    """tuple/listを交換形式として正規化する。意味上の専用型は個別能力で保持する。"""
    return json.loads(_符号化(value))


def 指紋(value):
    return sha256(_符号化(value)).hexdigest()


def 結果を保存(value: 能力結果):
    raw = _結果辞書(value)
    status = value.状態
    if not isinstance(status, 実行状態) or value.成立 != (status == 実行状態.合格):
        raise ValueError("能力結果の成立・採否が矛盾")
    positions = []
    def visit(item, path):
        if type(item) is tuple:
            positions.append(path)
        if type(item) in (tuple, list):
            for index, child in enumerate(item):
                visit(child, [*path, index])
        elif type(item) is dict:
            for key, child in item.items():
                visit(child, [*path, key])
    visit(raw, [])
    return 正準({"内容": raw, "採否": status.value, "組位置": positions})


def 結果を復元(raw):
    if type(raw) is not dict or set(raw) != {"内容", "採否", "組位置"}:
        raise ValueError("運用結果の欄不一致")
    content = deepcopy(raw["内容"])
    positions = raw["組位置"]
    if type(positions) is not list or len(positions) > 100_000:
        raise ValueError("組位置の型・上限")
    if any(type(path) is not list or not path or len(path) > 32
           or any(type(x) not in (str, int) for x in path) for path in positions):
        raise ValueError("組位置の経路不正")
    seen = set()
    def member(target, key):
        if type(target) is list and type(key) is int and 0 <= key < len(target):
            return target[key]
        if type(target) is dict and type(key) is str and key in target:
            return target[key]
        raise ValueError("組位置が内容に存在しない")
    for path in sorted(positions, key=len, reverse=True):
        if tuple(path) in seen:
            raise ValueError("組位置の重複")
        seen.add(tuple(path))
        target = content
        for key in path[:-1]:
            target = member(target, key)
        item = member(target, path[-1])
        if type(item) is not list:
            raise ValueError("組位置が配列ではない")
        target[path[-1]] = tuple(item)
    value = replace(能力結果を復元(content), 採否状態=実行状態(raw["採否"]))
    結果を保存(value)
    return value


def 計画を保存(plan):
    if type(plan) is not 合成計画:
        raise TypeError("合成計画が必要")
    return 正準(asdict(plan))


def 計画を復元(raw):
    if type(raw) is not dict or set(raw) != {"工程", "出力工程"}:
        raise ValueError("運用計画の欄不一致")
    if type(raw["工程"]) is not list or len(raw["工程"]) > 64:
        raise ValueError("運用計画の工程数")
    steps = []
    for row in raw["工程"]:
        if type(row) is not dict or set(row) != set(合成工程.__dataclass_fields__):
            raise ValueError("運用工程の欄不一致")
        steps.append(合成工程(row["識別子"], tuple(row["能力候補"]), row["指示参照"],
                             tuple(素材参照(**ref) for ref in row["入力"]), row["設定参照"]))
    return 合成計画(tuple(steps), tuple(raw["出力工程"]))


def 文字を検査(value, name="入力", limit=8192):
    if type(value) is not str or not value.strip() or len(value) > limit or "\x00" in value:
        raise ValueError(name + "の型・範囲不正")
    value.encode("utf-8", errors="strict")
    return value


def 封緘(データ):
    データ = 正準(データ)
    return {"内容": データ, "SHA256": 指紋(データ)}


def 開封(raw):
    if type(raw) is not dict or set(raw) != {"内容", "SHA256"} or 指紋(raw["内容"]) != raw["SHA256"]:
        raise ValueError("保存記録の整合不一致")
    return deepcopy(raw["内容"])

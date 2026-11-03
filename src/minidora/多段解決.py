"""目的を下位目的へ分解し、検証失敗から別解法へ戻る純粋処理の探索器。

有限の宣言済み目的・解法だけを扱う。自由文理解・目的の発明・外部作用は担わない。
解法の出力宣言ではなく、実際の能力実行と目的ごとの検証成功で状態を進める。
"""
from __future__ import annotations

from collections.abc import Callable, Iterable, Mapping
from copy import deepcopy
from dataclasses import asdict, dataclass, replace
from hashlib import sha256
import json

from .能力合成 import (登録能力, 能力合成器, 合成計画, 合成工程, 素材参照,
                      _結果辞書, _JSON検証)
from .製品版.型 import 能力結果
from .製品版.能力契約 import 能力文脈

多段解決版 = "MINIDORA-多段解決-v0.1"


@dataclass(frozen=True, slots=True)
class 問題素材:
    領域: str
    識別子: str


@dataclass(frozen=True, slots=True)
class 解決目的:
    識別子: str
    検証能力: str
    検証指示参照: str
    検証設定参照: str | None = None
    検証資料参照: tuple[str, ...] = ()


@dataclass(frozen=True, slots=True)
class 解法:
    識別子: str
    目的ID: str
    下位目的: tuple[str, ...]
    能力: str
    指示参照: str
    入力: tuple[問題素材, ...]
    設定参照: str | None = None
    優先度: int = 0


@dataclass(frozen=True, slots=True)
class 多段問題:
    最終目的: tuple[str, ...]
    目的群: tuple[解決目的, ...]
    解法群: tuple[解法, ...]


def _符号(value: object) -> bytes:
    _JSON検証(value)
    return json.dumps(value, ensure_ascii=False, sort_keys=True,
                      separators=(",", ":"), allow_nan=False).encode("utf-8")


def _指紋(value: object) -> str:
    return sha256(_符号(value)).hexdigest()


def _名前(value: str) -> None:
    if type(value) is not str or not value.strip() or len(value) > 128:
        raise ValueError("識別子不正")
    if value != value.strip() or any(ord(c) < 32 or ord(c) == 127 for c in value):
        raise ValueError("識別子の空白・制御文字")
    value.encode("utf-8")


def 多段問題を復元(raw: dict) -> 多段問題:
    if type(raw) is not dict or set(raw) != set(多段問題.__dataclass_fields__):
        raise ValueError("多段問題の項目不一致")
    for key in raw:
        if type(raw[key]) not in (tuple, list):
            raise ValueError("多段問題の配列型不正")
    goals, methods = [], []
    for row in raw["目的群"]:
        if type(row) is not dict or set(row) != set(解決目的.__dataclass_fields__):
            raise ValueError("目的の項目不一致")
        value = dict(row)
        if type(value["検証資料参照"]) not in (list, tuple):
            raise ValueError("検証資料型不正")
        value["検証資料参照"] = tuple(value["検証資料参照"])
        goals.append(解決目的(**value))
    for row in raw["解法群"]:
        if type(row) is not dict or set(row) != set(解法.__dataclass_fields__):
            raise ValueError("解法の項目不一致")
        value = dict(row)
        for key in ("下位目的", "入力"):
            if type(value[key]) not in (list, tuple):
                raise ValueError("解法の配列型不正")
        if any(type(r) is not dict or set(r) != set(問題素材.__dataclass_fields__) for r in value["入力"]):
            raise ValueError("入力素材型不正")
        value["入力"] = tuple(問題素材(**r) for r in value["入力"])
        value["下位目的"] = tuple(value["下位目的"])
        methods.append(解法(**value))
    return 多段問題(tuple(raw["最終目的"]), tuple(goals), tuple(methods))


@dataclass(frozen=True, slots=True)
class 多段結果:
    状態: str
    出力: tuple[tuple[str, 能力結果], ...]
    中間結果: tuple[tuple[str, 能力結果], ...]
    採用経路: tuple[dict, ...]
    履歴: tuple[dict, ...]
    理由: str
    展開数: int
    呼出数: int
    開始ハッシュ: str
    ハッシュ: str = ""

    @property
    def 成立(self) -> bool:
        return self.状態 == "合格"

    def 整合確認(self) -> bool:
        try:
            previous = self.開始ハッシュ
            for row in self.履歴:
                value = dict(row)
                actual = value.pop("ハッシュ")
                if value["前ハッシュ"] != previous or _指紋(value) != actual:
                    return False
                previous = actual
            return self.ハッシュ == _終端(self)
        except (TypeError, ValueError, KeyError, RecursionError):
            return False


def _終端(結果: 多段結果) -> str:
    return _指紋({"状態": 結果.状態, "理由": 結果.理由,
        "開始": 結果.開始ハッシュ,
        "末尾": 結果.履歴[-1]["ハッシュ"] if 結果.履歴 else 結果.開始ハッシュ,
        "出力": [(k, _結果辞書(v)) for k, v in 結果.出力],
        "中間結果": [(k, _結果辞書(v)) for k, v in 結果.中間結果],
        "採用経路": 結果.採用経路, "展開数": 結果.展開数, "呼出数": 結果.呼出数})


class _打切り(Exception):
    def __init__(self, 状態: str, 理由: str):
        super().__init__(理由)
        self.状態 = 状態


class 多段解決器:
    """解法の有限なAND/OR分解と後戻り。純粋性は登録側の明示契約であり隔離ではない。"""

    def __init__(self, 能力: Iterable[登録能力], *, 純粋作用確認: bool = False):
        if 純粋作用確認 is not True:
            raise ValueError("登録能力に外部・永続副作用がないことの明示確認が必要")
        self._能力 = {}
        for r in 能力:
            if not isinstance(r, 登録能力) or r.外部読取 is not False:
                raise ValueError("外部読取能力は探索対象外")
            _名前(r.モジュール.名前)
            _名前(r.モジュール.版)
            if r.モジュール.名前 in self._能力:
                raise ValueError("能力名重複")
            # 既存契約で呼出インターフェースも検証する。
            能力合成器((r,))
            self._能力[r.モジュール.名前] = (r, r.モジュール.版)

    def _検証(self, p: 多段問題, 資料: Mapping[str, 能力結果]):
        if not isinstance(p, 多段問題) or not isinstance(資料, Mapping):
            raise ValueError('多段問題または資料型不正')
        for key, value in 資料.items():
            _名前(key)
            _結果辞書(value)
        if type(p.目的群) is not tuple or not 1 <= len(p.目的群) <= 64:
            raise ValueError("目的数範囲外")
        if type(p.解法群) is not tuple or not 0 <= len(p.解法群) <= 128:
            raise ValueError("解法数範囲外")
        goals, methods = {}, {}

        def 能力(name):
            _名前(name)
            if name not in self._能力:
                raise ValueError("未登録能力")

        def 参照(name):
            _名前(name)
            if name not in 資料:
                raise ValueError('必要資料欠落')

        for goal in p.目的群:
            if not isinstance(goal, 解決目的):
                raise ValueError("目的型不正")
            _名前(goal.識別子)
            if goal.識別子 in goals:
                raise ValueError("目的ID重複")
            能力(goal.検証能力)
            参照(goal.検証指示参照)
            if goal.検証設定参照 is not None:
                参照(goal.検証設定参照)
            if type(goal.検証資料参照) is not tuple or len(goal.検証資料参照) > 16:
                raise ValueError("検証資料数範囲外")
            for key in goal.検証資料参照:
                参照(key)
            goals[goal.識別子] = goal
        if type(p.最終目的) is not tuple or not 1 <= len(p.最終目的) <= 16:
            raise ValueError("最終目的数範囲外")
        for name in p.最終目的:
            _名前(name)
            if name not in goals:
                raise ValueError("最終目的未宣言")
        if len(set(p.最終目的)) != len(p.最終目的):
            raise ValueError("最終目的重複")
        for m in p.解法群:
            if not isinstance(m, 解法):
                raise ValueError("解法型不正")
            _名前(m.識別子)
            if m.識別子 in methods or m.目的ID not in goals:
                raise ValueError("解法ID重複または目的未宣言")
            能力(m.能力)
            参照(m.指示参照)
            if m.設定参照 is not None:
                参照(m.設定参照)
            if type(m.優先度) is not int:
                raise ValueError("優先度型不正")
            if type(m.下位目的) is not tuple or len(m.下位目的) > 16:
                raise ValueError("下位目的数範囲外")
            for name in m.下位目的:
                _名前(name)
                if name not in goals:
                    raise ValueError("下位目的未宣言")
            if len(set(m.下位目的)) != len(m.下位目的):
                raise ValueError("下位目的重複")
            if type(m.入力) is not tuple or len(m.入力) > 32:
                raise ValueError("素材数範囲外")
            used = set()
            for r in m.入力:
                if not isinstance(r, 問題素材):
                    raise ValueError("素材型不正")
                _名前(r.識別子)
                if r.領域 == "入力":
                    参照(r.識別子)
                elif r.領域 == "目的" and r.識別子 in m.下位目的:
                    used.add(r.識別子)
                else:
                    raise ValueError("素材領域不正または未宣言依存")
            if used != set(m.下位目的):
                raise ValueError("結果を使わない下位目的")
            methods[m.識別子] = m
        return goals, methods

    def 実行(self, 問題: 多段問題, 初期資料: Mapping[str, 能力結果], *,
             最大展開数: int = 256, 最大呼出数: int = 512, 最大深さ: int = 32,
             停止要求: Callable[[], bool] | None = None) -> 多段結果:
        log, expansions, calls = [], 0, 0
        start = _指紋({"版": 多段解決版, "状態": "準備前"})

        def 停止確認():
            if 停止要求 is not None:
                try:
                    value = 停止要求()
                except Exception as exc:
                    raise _打切り("失敗", "停止判定故障:" + type(exc).__name__) from exc
                if type(value) is not bool:
                    raise _打切り("失敗", "停止要求型不正")
                if value:
                    raise _打切り("中止", "停止要求")

        def 記録(kind, goal="", method="", **extra):
            if len(log) >= 8192:
                raise _打切り("保留", "探索記録数上限")
            row = {"作用": kind, "目的": goal, "解法": method, **extra,
                   "前ハッシュ": log[-1]["ハッシュ"] if log else start}
            row["ハッシュ"] = _指紋(row)
            log.append(row)

        def 終了(status, reason="", 状態=None, 経路=()):
            状態 = 状態 or {}
            outputs = tuple((key, 状態[key]) for key in 問題.最終目的) if status == "合格" else ()
            結果 = 多段結果(status, deepcopy(outputs), deepcopy(tuple(sorted(状態.items()))),
                             deepcopy(tuple(経路)), deepcopy(tuple(log)), reason, expansions, calls, start)
            return replace(結果, ハッシュ=_終端(結果))

        try:
            for n, upper in ((最大展開数, 1024), (最大呼出数, 2048), (最大深さ, 48)):
                if type(n) is not int or not 1 <= n <= upper:
                    raise ValueError("探索上限範囲外")
            goals, methods = self._検証(問題, 初期資料)
            p, 資料 = deepcopy(問題), deepcopy(dict(初期資料))
            declaration = {"版": 多段解決版, "問題": asdict(p),
                '初期資料': {k: _結果辞書(v) for k, v in 資料.items()},
                "能力": sorted((k, version) for k, (_, version) in self._能力.items()),
                "予算": [最大展開数, 最大呼出数, 最大深さ]}
            if len(_符号(declaration)) > 1000000:
                raise ValueError("入力サイズ上限")
            start = _指紋(declaration)
            choices = {key: sorted((m for m in methods.values() if m.目的ID == key),
                                   key=lambda m: (m.優先度, m.識別子)) for key in goals}

            def 呼出(name, instruction, setting, materials, goal, method, 役割):
                nonlocal calls
                停止確認()
                if calls >= 最大呼出数:
                    raise _打切り("保留", "能力呼出数上限")
                registration, version = self._能力[name]
                if registration.モジュール.名前 != name or registration.モジュール.版 != version:
                    raise _打切り("失敗", "登録能力の名前・版変更")
                情報源 = {"指示": deepcopy(資料[instruction])}
                if setting is not None:
                    情報源["設定"] = deepcopy(資料[setting])
                for i, value in enumerate(materials):
                    情報源[f"素材:{i}"] = deepcopy(value)
                plan = 合成計画((合成工程("呼出", (name,), "指示",
                     tuple(素材参照("入力", f"素材:{i}") for i in range(len(materials))),
                     "設定" if setting is not None else None),), ("呼出",))
                calls += 1
                結果 = 能力合成器((registration,)).実行(plan, 情報源,
                    文脈=能力文脈("", "多段解決"), 停止要求=停止要求)
                停止確認()
                if registration.モジュール.名前 != name or registration.モジュール.版 != version:
                    raise _打切り("失敗", "実行中の能力名・版変更")
                記録("能力呼出", goal, method, 役割=役割, 能力=name, 版=version,
                      状態=結果.状態, 理由=結果.理由, 合成記録=結果.ルートハッシュ,
                      詳細=[{"能力": r.能力, "状態": r.状態, "理由": r.理由, "実行済": r.実行済}
                            for r in 結果.履歴])
                if 結果.状態 == "中止":
                    raise _打切り("中止", 結果.理由)
                # 制御機構の故障・未復元の入力契約違反は代替経路で隠さない。
                if not 結果.監査整合():
                    raise _打切り("失敗", "合成記録の整合違反")
                if 結果.状態 == "失敗" and 結果.理由.startswith("停止"):
                    raise _打切り("失敗", 結果.理由)
                value = 結果.出力[0][1] if 結果.成立 else None
                return value, 結果.ルートハッシュ

            def 複数目的を解決(required, index, 状態, 経路, active, depth):
                停止確認()
                if index == len(required):
                    yield 状態, 経路
                    return
                for new_状態, new_経路 in 目的を解決(required[index], 状態, 経路, active, depth):
                    yield from 複数目的を解決(required, index + 1, new_状態, new_経路, active, depth)

            def 目的を解決(key, 状態, 経路, active, depth):
                nonlocal expansions
                停止確認()
                if key in 状態:
                    記録("成果再利用", key, 出力ハッシュ=_指紋(_結果辞書(状態[key])))
                    yield 状態, 経路
                    return
                if key in active:
                    記録("循環経路除外", key)
                    return
                if depth > 最大深さ:
                    raise _打切り("保留", "分解深さ上限")
                if not choices[key]:
                    記録("解法なし", key)
                    return
                for method in choices[key]:
                    停止確認()
                    if expansions >= 最大展開数:
                        raise _打切り("保留", "解法展開数上限")
                    expansions += 1
                    記録("目的分解", key, method.識別子, 下位目的=list(method.下位目的))
                    for partial, prefix in 複数目的を解決(method.下位目的, 0, 状態, 経路,
                                                       active | {key}, depth + 1):
                        inputs = tuple((資料 if r.領域 == "入力" else partial)[r.識別子] for r in method.入力)
                        候補, call_hash = 呼出(method.能力, method.指示参照,
                            method.設定参照, inputs, key, method.識別子, "処理")
                        if 候補 is None:
                            記録("実行不成立", key, method.識別子)
                            continue
                        target = goals[key]
                        checked, check_hash = 呼出(target.検証能力, target.検証指示参照,
                            target.検証設定参照, (候補,) + tuple(資料[k] for k in target.検証資料参照),
                            key, method.識別子, "目的検証")
                        if checked is None:
                            記録("目的条件未達", key, method.識別子,
                                  候補ハッシュ=_指紋(_結果辞書(候補)))
                            continue
                        accepted = {**partial, key: deepcopy(候補)}
                        if len(_符号({k: _結果辞書(v) for k, v in accepted.items()})) > 1000000:
                            raise _打切り("保留", "枝状態サイズ上限")
                        step = {"目的": key, "解法": method.識別子,
                                "下位目的": list(method.下位目的), "処理記録": call_hash,
                                "検証記録": check_hash, "出力ハッシュ": _指紋(_結果辞書(候補))}
                        記録("局所成立", key, method.識別子, 出力ハッシュ=step["出力ハッシュ"])
                        yield accepted, prefix + (step,)
                        # 親・後続目的が不成立だった場合、ここへ戻って子の別解も列挙する。
                        記録("後続から再開", key, method.識別子)
                    記録("解法探索終了", key, method.識別子)

            solutions = 複数目的を解決(p.最終目的, 0, {}, (), frozenset(), 1)
            try:
                winning = next(solutions, None)
                停止確認()
            finally:
                solutions.close()
            if winning is None:
                return 終了("保留", "宣言された有限解法で全目的を完了できない")
            状態, 経路 = winning
            記録("全目的成立", 最終目的=list(p.最終目的))
            return 終了("合格", 状態=状態, 経路=経路)
        except _打切り as exc:
            return 終了(exc.状態, str(exc))
        except Exception as exc:
            return 終了("失敗", "多段解決契約・制御違反:" + type(exc).__name__)

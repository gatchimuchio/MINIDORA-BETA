"""同一尺度の差分関係から比較を導く。宣言された前提の論理であり世界の真偽ではない。

実数上の有限な差分制約の連言を扱う。整数制約・因果・集合包含は扱わない。
"""
from __future__ import annotations

from copy import deepcopy
from dataclasses import asdict, dataclass, replace
from datetime import date
from fractions import Fraction
from hashlib import sha256
import json
import re
from typing import Callable

from .能力合成 import _結果辞書, _参照結合
from .製品版.型 import 能力結果, 参照資料

関係制約版 = "MINIDORA-関係制約-v0.1"
_反転 = {"一致": "不一致", "不一致": "一致", "以下": "超", "超": "以下", "以上": "未満", "未満": "以上"}


@dataclass(frozen=True, slots=True)
class 関係式:
    識別子: str
    左項: str
    比較: str
    右項: str
    差: str = "0"


@dataclass(frozen=True, slots=True)
class 関係根拠:
    制約ID: str
    参照ID: str
    開始: int
    終了: int


@dataclass(frozen=True, slots=True)
class 関係問題:
    変数: tuple[str, ...]
    制約: tuple[関係式, ...]
    問い: tuple[関係式, ...]
    属性: str = "順序尺度"
    単位: str = "同一尺度"
    条件: str | None = None
    時点: str | None = None
    未解釈: tuple[str, ...] = ()
    根拠: tuple[関係根拠, ...] = ()


def _文字(value: str) -> None:
    if type(value) is not str or not value.strip() or len(value) > 128 or value != value.strip():
        raise ValueError("識別文字列不正")
    if any(ord(c) < 32 or ord(c) == 127 for c in value):
        raise ValueError("制御文字不正")
    value.encode("utf-8")


def _数(value: str) -> Fraction:
    if type(value) is not str or not re.fullmatch(r"[+-]?[0-9]{1,64}(?:\.[0-9]{1,64}|/[1-9][0-9]{0,63})?", value):
        raise ValueError("差は整数・小数・有理数の文字列")
    return Fraction(value)


def _符号(value: object) -> bytes:
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()


def 関係問題を復元(raw: dict) -> 関係問題:
    if type(raw) is not dict or set(raw) != set(関係問題.__dataclass_fields__):
        raise ValueError("関係問題の項目不一致")
    values = deepcopy(raw)
    for field in ("変数", "制約", "問い", "未解釈", "根拠"):
        if type(values[field]) not in (list, tuple):
            raise ValueError("関係問題の配列型不正")
    for field in ("制約", "問い", "根拠"):
        cls = 関係根拠 if field == "根拠" else 関係式
        rows = values[field]
        if any(type(row) is not dict or set(row) != set(cls.__dataclass_fields__) for row in rows):
            raise ValueError("関係式または根拠の項目不一致")
        values[field] = tuple(cls(**row) for row in rows)
    values["変数"], values["未解釈"] = tuple(values["変数"]), tuple(values["未解釈"])
    return 関係問題(**values)


def _検証(p: 関係問題, refs: tuple[参照資料, ...]) -> None:
    if not isinstance(p, 関係問題):
        raise ValueError("関係問題型不正")
    if type(p.変数) is not tuple or not 1 <= len(p.変数) <= 24:
        raise ValueError("変数数範囲外")
    for name in (*p.変数, p.属性, p.単位):
        _文字(name)
    if len(set(p.変数)) != len(p.変数):
        raise ValueError("変数重複")
    if p.条件 is not None:
        _文字(p.条件)
    if p.時点 is not None:
        if type(p.時点) is not str or not re.fullmatch(r"[0-9]{4}-[0-9]{2}-[0-9]{2}", p.時点):
            raise ValueError("時点形式不正")
        date.fromisoformat(p.時点)
    ids = set()
    for seq, lower, upper in ((p.制約, 0, 128), (p.問い, 1, 16)):
        if type(seq) is not tuple or not lower <= len(seq) <= upper:
            raise ValueError("制約数・問い数範囲外")
        for row in seq:
            if not isinstance(row, 関係式):
                raise ValueError("関係式型不正")
            _文字(row.識別子)
            if row.識別子.startswith("@") or row.識別子 in ids:
                raise ValueError("式ID重複または予約接頭辞")
            ids.add(row.識別子)
            if row.左項 not in p.変数 or row.右項 not in p.変数 or row.比較 not in _反転:
                raise ValueError("未宣言変数または未対応比較")
            _数(row.差)
    if type(p.未解釈) is not tuple or len(p.未解釈) > 128:
        raise ValueError("未解釈情報型不正")
    for text in p.未解釈:
        _文字(text)
    _結果辞書(能力結果(True, "", 参照=refs))
    sources = {s.識別子: s for s in _参照結合(refs)}
    if type(p.根拠) is not tuple or len(p.根拠) > 256:
        raise ValueError("関係根拠型不正")
    fact_ids = {r.識別子 for r in p.制約}
    for link in p.根拠:
        if not isinstance(link, 関係根拠) or link.制約ID not in fact_ids or link.参照ID not in sources:
            raise ValueError("根拠の参照先不正")
        if type(link.開始) is not int or type(link.終了) is not int or not 0 <= link.開始 < link.終了 <= len(sources[link.参照ID].本文):
            raise ValueError("根拠の原文範囲不正")
    if len(_符号({"問題": asdict(p), "参照": [s.辞書化() for s in refs]})) > 1000000:
        raise ValueError("関係入力サイズ上限")


@dataclass(frozen=True, slots=True)
class _上界:
    値: Fraction
    厳密: bool
    由来: frozenset[str]

    def より強い(self, other: _上界 | None) -> bool:
        return other is None or self.値 < other.値 or self.値 == other.値 and self.厳密 and not other.厳密


def _閉包(variables: tuple[str, ...], rows: tuple[関係式, ...], tick: Callable[[], None]):
    """有理数上界と厳密性を分け、微小epsilonや丸めで比較しない。"""
    names = {name: i for i, name in enumerate(sorted(variables))}
    n = len(names)
    bounds: list[list[_上界 | None]] = [[None] * n for _ in range(n)]
    for i in range(n):
        bounds[i][i] = _上界(Fraction(0), False, frozenset())
    exclusions = []
    for row in sorted(rows, key=lambda r: r.識別子):
        x, y, c = names[row.左項], names[row.右項], _数(row.差)
        op = row.比較
        if op == "不一致":
            exclusions.append(row)
            continue
        edges = []
        if op in ("一致", "以下", "未満"):
            edges.append((y, x, c, op == "未満"))
        if op in ("一致", "以上", "超"):
            edges.append((x, y, -c, op == "超"))
        for start, end, value, strict in edges:
            b = _上界(value, strict, frozenset((row.識別子,)))
            if b.より強い(bounds[start][end]):
                bounds[start][end] = b
    for k in range(n + 1):
        tick()
        for i in range(n):
            b = bounds[i][i]
            if b.値 < 0 or b.値 == 0 and b.厳密:
                return False, {"種類": "矛盾閉路", "使用制約": sorted(b.由来)}, bounds, names
        if k == n:
            break
        for i in range(n):
            left = bounds[i][k]
            if left is None:
                continue
            for j in range(n):
                right = bounds[k][j]
                if right is None:
                    continue
                tick()
                b = _上界(left.値 + right.値, left.厳密 or right.厳密, left.由来 | right.由来)
                if b.より強い(bounds[i][j]):
                    bounds[i][j] = b
    # 有限個の非等式は、凸な可解領域がその一つの超平面に含まれる場合だけ
    # 全域を除去できる。実数領域に限定する理由。整数領域には転用しない。
    for row in exclusions:
        x, y, c = names[row.左項], names[row.右項], _数(row.差)
        a, b = bounds[y][x], bounds[x][y]
        if a is not None and b is not None and a.値 == c and b.値 == -c:
            used = a.由来 | b.由来 | {row.識別子}
            return False, {"種類": "強制等値と非等式の競合", "使用制約": sorted(used)}, bounds, names
    return True, {}, bounds, names


class _中断(Exception):
    pass


class 関係制約器:
    def 実行(self, 問題: 関係問題, 参照: tuple[参照資料, ...] = (), *,
             最大演算数: int = 500000, 停止要求: Callable[[], bool] | None = None) -> 能力結果:
        count = 0
        def tick():
            nonlocal count
            count += 1
            if count > 最大演算数:
                raise _中断("関係演算数上限")
            if 停止要求 is not None and (count == 1 or count % 64 == 0):
                value = 停止要求()
                if type(value) is not bool:
                    raise ValueError("停止要求型不正")
                if value:
                    raise _中断("停止要求")
        try:
            if type(最大演算数) is not int or not 1 <= 最大演算数 <= 5000000:
                raise ValueError("演算上限不正")
            _検証(問題, 参照)
            p, refs = deepcopy(問題), tuple(sorted(_参照結合(参照), key=lambda s: s.識別子))
            ok, conflict, matrix, names = _閉包(p.変数, p.制約, tick)
            answers = []
            for query in p.問い:
                tick()
                状態, proof = "未確定", {}
                if not ok:
                    状態, proof = "前提矛盾", conflict
                elif p.未解釈:
                    状態 = "保留"
                else:
                    neg = replace(query, 識別子="@否定:" + query.識別子, 比較=_反転[query.比較])
                    pos = replace(query, 識別子="@肯定:" + query.識別子)
                    neg_ok, no, _, _ = _閉包(p.変数, p.制約 + (neg,), tick)
                    if not neg_ok:
                        状態, proof = "導出", {**no, "仮定": asdict(neg)}
                    else:
                        pos_ok, no, _, _ = _閉包(p.変数, p.制約 + (pos,), tick)
                        if not pos_ok:
                            状態, proof = "反証", {**no, "仮定": asdict(pos)}
                used = [x for x in proof.get("使用制約", []) if not x.startswith("@")]
                links = []
                for link in p.根拠:
                    if link.制約ID in used:
                        情報源 = next(s for s in refs if s.識別子 == link.参照ID)
                        links.append({**asdict(link), "原文": 情報源.本文[link.開始:link.終了]})
                answers.append({"問い": asdict(query), "判定": 状態, "根拠制約": used,
                                "反証検査": proof, "原文対応": links})
            body = "\n".join(f'{json.dumps(a["問い"]["識別子"], ensure_ascii=True)}: {a["判定"]}' for a in answers)
            body += "\n宣言された同一尺度・条件・時点の制約からの判定。事実認定ではありません。"
            資料 = {"版": 関係制約版, "問題": asdict(p), "前提整合": ok, "競合": conflict,
                    "回答": answers, "演算数": count, "最大演算数": 最大演算数,
                    "事実認定": "未実施", "入力意味": "構造化制約の意味・適用範囲は呼出側の宣言"}
            結果 = 能力結果(True, body, 根拠=tuple(sorted({x for a in answers for x in a["根拠制約"]})), 参照=refs, データ=資料)
            資料["記録SHA256"] = sha256(_符号(_結果辞書(結果))).hexdigest()
            if len(_符号(_結果辞書(結果))) > 1500000:
                raise ValueError("関係記録サイズ上限")
            # 小さい計算も返却直前の中止を観測する。
            if 停止要求 is not None:
                value = 停止要求()
                if type(value) is not bool:
                    raise ValueError("停止要求型不正")
                if value:
                    raise _中断("停止要求")
            return 結果
        except _中断 as exc:
            return 能力結果(False, "", 保留理由=str(exc), データ={"演算数": count})
        except Exception as exc:
            return 能力結果(False, "", 保留理由=f"関係制約契約・制御違反:{type(exc).__name__}")


def 関係記録整合(結果: 能力結果) -> bool:
    try:
        if not isinstance(結果, 能力結果) or not 結果.成立:
            return False
        p = 関係問題を復元(結果.データ["問題"])
        expected = 関係制約器().実行(p, 結果.参照, 最大演算数=結果.データ["最大演算数"])
        value = {**_結果辞書(結果), "参照": [s.辞書化() for s in sorted(_参照結合(結果.参照), key=lambda s: s.識別子)]}
        return expected.成立 and _符号(value) == _符号(_結果辞書(expected))
    except (ValueError, TypeError, KeyError, AttributeError, RecursionError):
        return False


def 関係判定を採用(結果: 能力結果, 問いID: str, 期待: str = "導出") -> 能力結果:
    if 期待 not in ("導出", "反証") or not 関係記録整合(結果):
        return 能力結果(False, "", 保留理由="関係報告の整合違反または採用条件不正")
    row = next((a for a in 結果.データ["回答"] if a["問い"]["識別子"] == 問いID), None)
    if row is None or row["判定"] != 期待:
        return 能力結果(False, "", 保留理由="問いの確定判定が採用条件を満たさない")
    return 能力結果(True, 期待, 根拠=tuple(row["根拠制約"]), 参照=結果.参照,
        データ={"判定": 期待, "問い": deepcopy(row["問い"]), "属性": 結果.データ["問題"]["属性"],
                "単位": 結果.データ["問題"]["単位"], "条件": 結果.データ["問題"]["条件"],
                "時点": 結果.データ["問題"]["時点"], "関係記録SHA256": 結果.データ["記録SHA256"]})

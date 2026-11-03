from __future__ import annotations

from dataclasses import dataclass, replace
from hashlib import sha256
import json
from typing import Iterable


def _stable(prefix: str, value: object) -> str:
    raw = json.dumps(value, ensure_ascii=False, sort_keys=True, default=str, separators=(",", ":"))
    return prefix + sha256(raw.encode("utf-8")).hexdigest()[:20]


@dataclass(frozen=True, slots=True)
class HDS形成観測:
    """一回の実行で「何が不足し、何をして、状態が変わったか」だけを記録する。"""

    runID: str
    入力署名: str
    作用: str
    前残差: tuple[str, ...]
    後残差: tuple[str, ...]
    前状態署名: str
    後状態署名: str
    進展: bool
    根拠ID: tuple[str, ...] = ()

    @property
    def 形成キー(self) -> tuple[tuple[str, ...], str]:
        return tuple(sorted(set(self.前残差))), self.作用


@dataclass(frozen=True, slots=True)
class HDS形成候補:
    候補ID: str
    前残差: tuple[str, ...]
    作用: str
    観測数: int
    進展数: int
    非進展数: int
    進展率: float
    独立入力数: int
    状態: str = "PROVISIONAL"
    承認理由: tuple[str, ...] = ()
    再開放条件: tuple[str, ...] = (
        "反例増加",
        "入力分布変化",
        "作用実装変更",
        "上位規定変更",
    )


class HDS形成台帳:
    """実行履歴から再利用可能な作用順序を形成するが、自動で推論規則へ昇格させない。

    Apertus/Grok等の形成循環をMINIDORA向けに非ニューラル化したもの。
    世界知識を埋め込むのではなく「この残差に対してこの既存作用が実際に進展を生んだ」
    という運用関係だけを形成する。
    """

    def __init__(self) -> None:
        self._観測: list[HDS形成観測] = []
        self._承認: dict[str, HDS形成候補] = {}

    @property
    def 観測(self) -> tuple[HDS形成観測, ...]:
        return tuple(self._観測)

    @property
    def 承認済み(self) -> tuple[HDS形成候補, ...]:
        return tuple(sorted(self._承認.values(), key=lambda x: x.候補ID))

    def 記録(self, 観測: HDS形成観測) -> None:
        if not 観測.runID or not 観測.入力署名 or not 観測.作用:
            raise ValueError("形成観測にはrunID・入力署名・作用が必要")
        self._観測.append(観測)

    def 候補群(self, *, 最小観測数: int = 3, 最小独立入力数: int = 2, 最小進展率: float = 0.75) -> tuple[HDS形成候補, ...]:
        grouped: dict[tuple[tuple[str, ...], str], list[HDS形成観測]] = {}
        for row in self._観測:
            grouped.setdefault(row.形成キー, []).append(row)

        out: list[HDS形成候補] = []
        for (residuals, 作用), rows in sorted(grouped.items(), key=lambda x: repr(x[0])):
            observed = len(rows)
            progressed = sum(1 for row in rows if row.進展 and row.前状態署名 != row.後状態署名)
            inputs = len({row.入力署名 for row in rows})
            rate = progressed / observed if observed else 0.0
            状態 = "ELIGIBLE" if (
                observed >= max(1, int(最小観測数))
                and inputs >= max(1, int(最小独立入力数))
                and rate >= float(最小進展率)
            ) else "PROVISIONAL"
            cid = _stable("FORM-", (residuals, 作用))
            approved = self._承認.get(cid)
            if approved is not None:
                状態 = "APPROVED"
            out.append(HDS形成候補(
                cid,
                residuals,
                作用,
                observed,
                progressed,
                observed - progressed,
                rate,
                inputs,
                状態,
                approved.承認理由 if approved is not None else (),
                approved.再開放条件 if approved is not None else HDS形成候補.__dataclass_fields__["再開放条件"].default,
            ))
        return tuple(out)

    def 承認(self, 候補ID: str, *, 理由: Iterable[str]) -> HDS形成候補:
        候補 = next((row for row in self.候補群() if row.候補ID == str(候補ID)), None)
        if 候補 is None:
            raise KeyError("形成候補が存在しない")
        if 候補.状態 not in {"ELIGIBLE", "APPROVED"}:
            raise ValueError("十分な実測がない形成候補は承認できない")
        reasons = tuple(dict.fromkeys(str(x) for x in 理由 if str(x)))
        if not reasons:
            raise ValueError("形成候補の承認には理由が必要")
        approved = replace(候補, 状態="APPROVED", 承認理由=reasons)
        self._承認[approved.候補ID] = approved
        return approved

    def 再開放(self, 候補ID: str, *, 理由: str) -> None:
        reason = str(理由).strip()
        if not reason:
            raise ValueError("再開放理由は空にできない")
        self._承認.pop(str(候補ID), None)

    def 推奨作用(self, residuals: Iterable[str]) -> tuple[str, ...]:
        target = tuple(sorted(set(str(x) for x in residuals if str(x))))
        rows = [row for row in self.承認済み if row.前残差 == target]
        rows.sort(key=lambda row: (-row.進展率, -row.独立入力数, row.作用))
        return tuple(dict.fromkeys(row.作用 for row in rows))


__all__ = ["HDS形成観測", "HDS形成候補", "HDS形成台帳"]

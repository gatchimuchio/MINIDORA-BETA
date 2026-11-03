from __future__ import annotations

from dataclasses import dataclass
import math
from typing import Mapping

from .HDS資料K import HDS証拠事実
from .HDS中間表現 import HDSIR, 値状態
from .K3機能 import 候補, K3相当能力核
from .意味字句 import 意味語


_BLOCKING_PROVENANCE = {
    'value_状態:未確定',
    'value_状態:未観測',
    'value_状態:矛盾',
    'value_状態:留保',
}
_HYPOTHESIS_ORIGIN = "HDS候補代入仮説"
_ASSERTION_ORIGINS = frozenset({'公開HDS 構文化器', "共有言語基底P"})
_GENERIC_RELATIONS = {
    "意味原子→節",
    "談話順序",
    "節→述語",
    "候補→集合",
    "問い×候補→選択目的",
    "共参照",
    "数量単位",
}


@dataclass(frozen=True, slots=True)
class HDS直接関係診断:
    候補: str
    得点: float
    独立出典数: int
    根拠事実ID: tuple[str, ...]
    仮説一致出典数: int = 0
    命題一致出典数: int = 0


@dataclass(frozen=True, slots=True)
class _候補辺:
    関係: str
    始点語: frozenset[str]
    終点語: frozenset[str]
    種別: str


def _coverage(query: frozenset[str], 証拠: frozenset[str]) -> float:
    if not query:
        return 0.0
    return len(query & 証拠) / len(query)


def _関係名(predicate: str) -> str | None:
    prefix = 'hds_関係_'
    if not str(predicate).startswith(prefix):
        return None
    return str(predicate)[len(prefix):].replace("_", " ")


def _fact_blocked(fact: object) -> bool:
    provenance = {str(x) for x in getattr(fact, "provenance", ())}
    return bool(provenance & _BLOCKING_PROVENANCE) or any(
        item.startswith('残差_blocked:') for item in provenance
    )


def _情報源ID(fact: object) -> str:
    provenance = tuple(str(x) for x in getattr(fact, "provenance", ()))
    if "HDS-IR" in provenance:
        情報源 = provenance[:provenance.index("HDS-IR")]
        if 情報源:
            return "|".join(情報源)
    fid = str(getattr(fact, "fact_id", ""))
    return "fact:" + (fid or str(id(fact)))


def _否定候補(ir: HDSIR) -> bool:
    return any(str(coord.種別) == "状態.否定" for coord in ir.座標)


def _候補辺群(ir: HDSIR) -> tuple[_候補辺, ...]:
    'K射影後の候補IRから直接検証可能な有向辺だけを読む。\n\n    共有言語基底Pの関係は構文化器と同じ意味正本資産から生じるため、K射影で範囲安全性を\n    通過した後は公開構文化器関係と同じ assertion として扱う。実行系/J専用由来はここへ\n    許可しない。\n    '
    coords = ir.座標辞書()
    out: list[_候補辺] = []
    negative = _否定候補(ir)
    for 関係 in ir.関係:
        origin = str(関係.由来)
        if origin == _HYPOTHESIS_ORIGIN:
            mode = "hypothesis"
            if 関係.値状態 not in {値状態.推定, 値状態.確定}:
                continue
        elif origin in _ASSERTION_ORIGINS:
            # 否定候補を肯定命題として直接証明しない。K射影で未対応scope辺は既に除外済み。
            if negative or 関係.値状態 != 値状態.確定:
                continue
            if str(関係.種別) in _GENERIC_RELATIONS:
                continue
            mode = "assertion"
        else:
            continue

        starts = [coords[cid] for cid in 関係.始点 if cid in coords]
        ends = [coords[cid] for cid in 関係.終点 if cid in coords]
        for start in starts:
            for end in ends:
                start_terms = 意味語(start.内容)
                end_terms = 意味語(end.内容)
                if start_terms and end_terms:
                    edge = _候補辺(str(関係.種別), start_terms, end_terms, mode)
                    if edge not in out:
                        out.append(edge)
    return tuple(out)


def _fact_edges(fact: object) -> tuple[str, frozenset[str], frozenset[str]] | None:
    if _fact_blocked(fact):
        return None
    関係 = _関係名(str(getattr(fact, "predicate", "")))
    if 関係 is None:
        return None
    args = tuple(str(x) for x in getattr(fact, "args", ()))
    if "→" not in args:
        return None
    split = args.index("→")
    starts = tuple(x for x in args[:split] if x)
    ends = tuple(x for x in args[split + 1:] if x)
    if not starts or not ends:
        return None
    start_terms = 意味語(" ".join(starts))
    end_terms = 意味語(" ".join(ends))
    if not start_terms or not end_terms:
        return None
    return 関係, start_terms, end_terms


def HDS直接関係検証(
    模型核: K3相当能力核,
    candidates: Mapping[str, HDSIR],
    *,
    最小端点被覆: float = 0.60,
    最小優位差: float = 0.15,
) -> tuple[候補 | None, tuple[HDS直接関係診断, ...]]:
    '候補の有向HDS関係と資料関係が直接一致する場合だけ候補証拠を返す。\n\n    二つの経路を扱う。\n    - 問いの未知端点へ実体候補を代入した仮説関係: 1独立情報源から直接検証可能。\n    - 候補自身が表す完全命題の関係: 検索自己確認を避けるため2独立情報源以上を要求。\n\n    候補語の共起、検索hit数、文書全体の語集合は使わない。関係種別・始点・終点が同時に\n    一致したFactだけを情報源単位で集約し、同等の対抗候補が残る場合は候補を返さない。\n    '
    facts = tuple(HDS証拠事実(模型核))
    fact_edges: list[tuple[object, str, frozenset[str], frozenset[str]]] = []
    for fact in facts:
        edge = _fact_edges(fact)
        if edge is not None:
            関係, starts, ends = edge
            fact_edges.append((fact, 関係, starts, ends))

    diagnostics: list[HDS直接関係診断] = []
    eligible: dict[str, bool] = {}
    for label, 候補_ir in sorted(candidates.items()):
        候補_edges = _候補辺群(候補_ir)
        per_情報源: dict[str, tuple[float, str, str]] = {}
        for expected in 候補_edges:
            for fact, fact_関係, actual_start, actual_end in fact_edges:
                if expected.関係 != fact_関係:
                    continue
                start_cov = _coverage(expected.始点語, actual_start)
                end_cov = _coverage(expected.終点語, actual_end)
                if start_cov < 最小端点被覆 or end_cov < 最小端点被覆:
                    continue
                信頼度 = max(0.0, min(1.0, float(getattr(fact, '信頼度', 1.0))))
                score = math.sqrt(start_cov * end_cov) * 信頼度
                情報源 = _情報源ID(fact)
                fid = str(getattr(fact, "fact_id", ""))
                old = per_情報源.get(情報源)
                if old is None or score > old[0] or (score == old[0] and expected.種別 == "hypothesis"):
                    per_情報源[情報源] = (score, fid, expected.種別)

        ranked_sources = sorted(per_情報源.values(), key=lambda row: (-row[0], row[1], row[2]))
        aggregate = 0.0
        for index, (score, _, _) in enumerate(ranked_sources[:3]):
            aggregate += score * (1.0 if index == 0 else 0.35 if index == 1 else 0.15)
        proof = tuple(fid for _, fid, _ in ranked_sources[:3] if fid)
        hypothesis_sources = sum(mode == "hypothesis" for _, _, mode in per_情報源.values())
        assertion_sources = sum(mode == "assertion" for _, _, mode in per_情報源.values())
        is_eligible = hypothesis_sources >= 1 or assertion_sources >= 2
        eligible[str(label)] = is_eligible
        diagnostics.append(
            HDS直接関係診断(
                str(label), aggregate, len(per_情報源), proof,
                hypothesis_sources, assertion_sources,
            )
        )

    ranked = sorted(diagnostics, key=lambda item: (-item.得点, -item.独立出典数, item.候補))
    if not ranked:
        return None, tuple(diagnostics)
    top = ranked[0]
    if not eligible.get(top.候補, False) or top.得点 < 最小端点被覆 or not top.根拠事実ID:
        return None, tuple(diagnostics)
    second = ranked[1].得点 if len(ranked) > 1 else 0.0
    if top.得点 - second < 最小優位差:
        return None, tuple(diagnostics)

    信頼度 = min(0.995, 0.80 + min(0.19, top.得点 * 0.10))
    確定候補 = 候補(
        answer=top.候補,
        関係='HDS_directed_関係_verification',
        信頼度=信頼度,
        expert='HDS_direct_関係_検証器',
        proof_fact_ids=top.根拠事実ID,
        provenance=(
            "HDS-IR",
            "K",
            "DIRECTED_ENDPOINT_MATCH",
            '情報源_DEDUPLICATED',
            "NO_GUESS",
            "hypothesis_sources:" + str(top.仮説一致出典数),
            "assertion_sources:" + str(top.命題一致出典数),
        ),
    )
    return 確定候補, tuple(diagnostics)


__all__ = ["HDS直接関係診断", "HDS直接関係検証"]

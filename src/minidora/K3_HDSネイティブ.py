from __future__ import annotations

from dataclasses import dataclass
import math
from typing import Mapping

from .選択意図 import HDS選択意図判定
from .HDS候補再照合 import HDS候補証拠, HDS候補横断調停
from .HDS資料K import HDS証拠事実
from .HDS探索方針 import HDS探索方針選択
from .HDS関係図推論 import HDS意味経路探索
from .HDS中間表現 import HDSIR, 値状態
from .K3機能 import 候補, HDSJudge, JudgeDecision, K3相当能力核, 意味Frame
from .意味字句 import 意味語


_SURFACE_ONLY_KINDS = {
    '情報源_text',
    '言語.input',
    '言語.normalized',
    "対象.原文保持",
    "文脈.言語",
}
_GENERIC_RELATIONS = {
    "意味原子→節",
    "談話順序",
    "候補→集合",
    "問い×候補→選択目的",
}
_SIGNATURE_BLOCKING_STATES = {
    値状態.未確定,
    値状態.未観測,
    値状態.矛盾,
    値状態.留保,
}
_BLOCKING_PROVENANCE = {'value_状態:' + 状態.value for 状態 in _SIGNATURE_BLOCKING_STATES}


def _choices(ir: HDSIR) -> tuple[tuple[str, str], ...]:
    out: list[tuple[str, str]] = []
    for coord in ir.座標:
        if coord.座標ID.startswith('選択肢:'):
            label = coord.座標ID.split(":", 1)[1]
            out.append((label, str(coord.内容)))
    return tuple(sorted(out, key=lambda x: x[0]))


def _facts(模型核: K3相当能力核) -> tuple[object, ...]:
    store = getattr(模型核.K, "_facts", {})
    証拠 = HDS証拠事実(模型核)
    if not 証拠:
        return tuple(store.values())

    証拠_ids = {str(getattr(fact, "fact_id", "")) for fact in 証拠}
    canonical_non_hds = []
    for fact in store.values():
        fid = str(getattr(fact, "fact_id", ""))
        provenance = tuple(str(x) for x in getattr(fact, "provenance", ()))
        if fid in 証拠_ids:
            continue
        if "HDS-IR" in provenance:
            continue
        canonical_non_hds.append(fact)
    return tuple(canonical_non_hds) + tuple(証拠)


def _fact_blocked(fact: object) -> bool:
    provenance = {str(x) for x in getattr(fact, "provenance", ())}
    return bool(provenance & _BLOCKING_PROVENANCE)


def _fact_text(模型核: K3相当能力核, fact: object) -> str:
    predicate = str(getattr(fact, "predicate", ""))
    args = tuple(getattr(fact, "args", ()))
    labels = [模型核.R.label(str(arg)) for arg in args]
    return " ".join((predicate, *labels))


def _述語から関係名(predicate: str) -> str | None:
    prefix = 'hds_関係_'
    if not predicate.startswith(prefix):
        return None
    return predicate[len(prefix):].replace("_", " ")


def _document_group_id(fact: object) -> str | None:
    provenance = tuple(str(x) for x in getattr(fact, "provenance", ()))
    if "HDS-IR" not in provenance:
        return None
    split = provenance.index("HDS-IR")
    情報源 = provenance[:split]
    if not 情報源:
        return None
    return "document:" + "|".join(情報源)


def _情報源群ID(fact: object) -> str:
    document = _document_group_id(fact)
    if document is not None:
        return document
    fid = str(getattr(fact, "fact_id", ""))
    return "fact:" + (fid or str(id(fact)))


def _事実情報源対応(模型核: K3相当能力核) -> dict[str, str]:
    out: dict[str, str] = {}
    for fact in _facts(模型核):
        fid = str(getattr(fact, "fact_id", ""))
        if fid:
            out[fid] = _情報源群ID(fact)
    return out


@dataclass(frozen=True, slots=True)
class HDS関係辺署名:
    関係: str
    始点語: frozenset[str]
    終点語: frozenset[str]


@dataclass(frozen=True, slots=True)
class HDS意味署名:
    語: frozenset[str]
    関係種別: frozenset[str]
    座標種別: frozenset[str]
    関係辺: tuple[HDS関係辺署名, ...] = ()


@dataclass(frozen=True, slots=True)
class _証拠群:
    群ID: str
    出典ID: str
    語: frozenset[str]
    関係種別: frozenset[str]
    座標種別: frozenset[str]
    事実ID: tuple[str, ...]
    信頼度: float
    範囲: str = "fact"
    関係阻害: bool = False
    関係辺: tuple[HDS関係辺署名, ...] = ()


@dataclass(frozen=True, slots=True)
class HDS候補診断:
    候補: str
    合計得点: float
    証拠得点: float
    関係図得点: float
    関係図補正係数: float
    独立出典数: int
    採用証拠数: int
    関係図深さ: int | None
    根拠事実数: int
    識別語数: int = 0
    識別一致出典数: int = 0


def _辺署名(関係: str, 始点: object, 終点: object) -> HDS関係辺署名 | None:
    start_terms = 意味語(始点)
    end_terms = 意味語(終点)
    if not start_terms or not end_terms:
        return None
    return HDS関係辺署名(str(関係), start_terms, end_terms)


def _意味署名(ir: HDSIR, *, 代替経路_text: str = "") -> HDS意味署名:
    terms: set[str] = set()
    kinds: set[str] = set()
    relations: set[str] = set()
    edges: list[HDS関係辺署名] = []
    coords = ir.座標辞書()

    for coord in ir.座標:
        kind = str(coord.種別)
        if kind in _SURFACE_ONLY_KINDS or coord.座標ID.startswith('選択肢:'):
            continue
        if coord.値状態 in _SIGNATURE_BLOCKING_STATES:
            continue
        coord_terms = 意味語(coord.内容)
        if coord_terms:
            terms.update(coord_terms)
            kinds.add(kind)

    for 関係 in ir.関係:
        if 関係.値状態 in _SIGNATURE_BLOCKING_STATES:
            continue
        関係_type = str(関係.種別)
        if 関係_type in _GENERIC_RELATIONS:
            continue
        relations.add(関係_type)
        starts = [
            coords[coordinate_id]
            for coordinate_id in 関係.始点
            if coordinate_id in coords and coords[coordinate_id].値状態 not in _SIGNATURE_BLOCKING_STATES
        ]
        ends = [
            coords[coordinate_id]
            for coordinate_id in 関係.終点
            if coordinate_id in coords and coords[coordinate_id].値状態 not in _SIGNATURE_BLOCKING_STATES
        ]
        for start in starts:
            for end in ends:
                edge = _辺署名(関係_type, start.内容, end.内容)
                if edge is not None and edge not in edges:
                    edges.append(edge)

    if not terms:
        terms.update(意味語(代替経路_text or ir.原文))
    return HDS意味署名(frozenset(terms), frozenset(relations), frozenset(kinds), tuple(edges))


def _候補識別語(signatures: Mapping[str, HDS意味署名]) -> dict[str, frozenset[str]]:
    labels = tuple(signatures)
    out: dict[str, frozenset[str]] = {}
    for label in labels:
        others: set[str] = set()
        for other in labels:
            if other != label:
                others.update(signatures[other].語)
        out[label] = frozenset(signatures[label].語 - others)
    return out


def _fact_signature(
    模型核: K3相当能力核,
    fact: object,
) -> tuple[set[str], set[str], set[str], tuple[HDS関係辺署名, ...]]:
    if _fact_blocked(fact):
        return set(), set(), set(), ()

    predicate = str(getattr(fact, "predicate", ""))
    args = tuple(str(x) for x in getattr(fact, "args", ()))
    terms: set[str] = set()
    relations: set[str] = set()
    kinds: set[str] = set()
    edges: list[HDS関係辺署名] = []

    関係 = _述語から関係名(predicate)
    if 関係 is not None:
        if 関係 not in _GENERIC_RELATIONS:
            relations.add(関係)
        terms.update(意味語(" ".join(x for x in args if x != "→")))
        if 関係 not in _GENERIC_RELATIONS and "→" in args:
            split = args.index("→")
            starts = tuple(x for x in args[:split] if x)
            ends = tuple(x for x in args[split + 1:] if x)
            for start in starts:
                for end in ends:
                    edge = _辺署名(関係, start, end)
                    if edge is not None and edge not in edges:
                        edges.append(edge)
    elif predicate == "hds_coordinate" and len(args) >= 2:
        kind = args[0]
        if kind not in _SURFACE_ONLY_KINDS:
            kinds.add(kind)
            terms.update(意味語(args[1]))
    elif predicate != 'hds_残差':
        terms.update(意味語(_fact_text(模型核, fact)))
        relations.add(predicate)
    return terms, relations, kinds, tuple(edges)


def _証拠群を作る(模型核: K3相当能力核) -> tuple[_証拠群, ...]:
    結果: list[_証拠群] = []
    document_terms: dict[str, set[str]] = {}
    document_relations: dict[str, set[str]] = {}
    document_kinds: dict[str, set[str]] = {}
    document_edges: dict[str, list[HDS関係辺署名]] = {}
    document_fact_ids: dict[str, list[str]] = {}
    document_confidences: dict[str, list[float]] = {}
    document_blocked_relations: set[str] = set()

    for fact in _facts(模型核):
        predicate = str(getattr(fact, "predicate", ""))
        if predicate == 'hds_残差':
            continue

        group_id = _document_group_id(fact)
        情報源_id = _情報源群ID(fact)
        if _fact_blocked(fact):
            if group_id is not None and _述語から関係名(predicate) is not None:
                document_blocked_relations.add(group_id)
            continue

        fid = str(getattr(fact, "fact_id", ""))
        信頼度 = float(getattr(fact, '信頼度', 1.0))
        terms, relations, kinds, edges = _fact_signature(模型核, fact)

        if terms or relations or kinds or edges:
            結果.append(
                _証拠群(
                    fid or str(id(fact)), 情報源_id, frozenset(terms), frozenset(relations), frozenset(kinds),
                    (fid,) if fid else (), 信頼度, "fact", False, edges,
                )
            )

        if group_id is None or not (terms or relations or kinds or edges):
            continue
        document_terms.setdefault(group_id, set()).update(terms)
        document_relations.setdefault(group_id, set()).update(relations)
        document_kinds.setdefault(group_id, set()).update(kinds)
        edge_rows = document_edges.setdefault(group_id, [])
        for edge in edges:
            if edge not in edge_rows:
                edge_rows.append(edge)
        if fid:
            ids = document_fact_ids.setdefault(group_id, [])
            if fid not in ids:
                ids.append(fid)
        document_confidences.setdefault(group_id, []).append(信頼度)

    for group_id in sorted(document_terms):
        ids = tuple(document_fact_ids.get(group_id, ()))
        if len(ids) < 2:
            continue
        relations = frozenset(document_relations.get(group_id, set()))
        関係_blocked = group_id in document_blocked_relations and not relations
        confidences = document_confidences.get(group_id, [1.0])
        結果.append(
            _証拠群(
                group_id, group_id, frozenset(document_terms[group_id]), relations,
                frozenset(document_kinds.get(group_id, set())), ids,
                sum(confidences) / len(confidences), "document", 関係_blocked,
                tuple(document_edges.get(group_id, ())),
            )
        )
    return tuple(結果)


def _coverage(query: frozenset[str], 証拠: frozenset[str]) -> float:
    if not query:
        return 0.0
    return len(query & 証拠) / len(query)


def _関係類似度(query: frozenset[str], 証拠: frozenset[str]) -> float:
    if not query or not 証拠:
        return 0.0
    return len(query & 証拠) / math.sqrt(len(query) * len(証拠))


def _kind_similarity(query: frozenset[str], 証拠: frozenset[str]) -> float:
    if not query or not 証拠:
        return 0.0
    return len(query & 証拠) / math.sqrt(len(query) * len(証拠))


def _edge_similarity(
    query: tuple[HDS関係辺署名, ...],
    証拠: tuple[HDS関係辺署名, ...],
) -> float:
    """関係種別だけでなく、始点→終点の方向を保った一致率を返す。"""
    if not query or not 証拠:
        return 0.0
    best = 0.0
    for query_edge in query:
        for 証拠_edge in 証拠:
            if query_edge.関係 != 証拠_edge.関係:
                continue
            start = _coverage(query_edge.始点語, 証拠_edge.始点語)
            end = _coverage(query_edge.終点語, 証拠_edge.終点語)
            if start <= 0 or end <= 0:
                continue
            best = max(best, math.sqrt(start * end))
    return best


def _group_score(
    question: HDS意味署名,
    候補: HDS意味署名,
    証拠: _証拠群,
    *,
    識別語: frozenset[str] = frozenset(),
) -> float:
    if 証拠.関係阻害:
        return 0.0
    full_coverage = _coverage(候補.語, 証拠.語)
    if full_coverage <= 0:
        return 0.0
    question_coverage = _coverage(question.語, 証拠.語)
    if question.語 and question_coverage <= 0:
        return 0.0

    if 識別語:
        distinctive_coverage = _coverage(識別語, 証拠.語)
        候補_coverage = 0.35 * full_coverage + 0.65 * distinctive_coverage
    else:
        候補_coverage = full_coverage

    関係_match = max(
        _関係類似度(question.関係種別, 証拠.関係種別),
        _関係類似度(候補.関係種別, 証拠.関係種別),
    )
    kind_match = max(
        _kind_similarity(question.座標種別, 証拠.座標種別),
        _kind_similarity(候補.座標種別, 証拠.座標種別),
    )
    question_direction_match = _edge_similarity(question.関係辺, 証拠.関係辺)
    候補_direction_match = _edge_similarity(候補.関係辺, 証拠.関係辺)
    direction_match = 候補_direction_match if 候補.関係辺 else question_direction_match
    structural_multiplier = 1.0 + 1.5 * 関係_match + 0.5 * kind_match + 2.0 * direction_match
    範囲_multiplier = 1.0
    if 証拠.範囲 == "document":
        範囲_multiplier = 0.62
        if 関係_match <= 0 and kind_match <= 0 and direction_match <= 0:
            範囲_multiplier *= 0.65
    return 証拠.信頼度 * (4.0 * 候補_coverage + 2.0 * question_coverage) * structural_multiplier * 範囲_multiplier


def _例外消去候補(
    choices: tuple[tuple[str, str], ...],
    scored: list[tuple[float, 候補]],
    diagnostics: tuple[HDS候補診断, ...],
) -> 候補 | None:
    labels = tuple(label for label, _ in choices)
    候補_by_label = {候補.answer: 候補 for _, 候補 in scored}
    diagnostic_by_label = {diagnostic.候補: diagnostic for diagnostic in diagnostics}

    supported = [
        label
        for label in labels
        if label in 候補_by_label
        and diagnostic_by_label.get(label) is not None
        and diagnostic_by_label[label].独立出典数 >= 1
        and diagnostic_by_label[label].識別一致出典数 >= 1
        and diagnostic_by_label[label].根拠事実数 >= 1
    ]
    unsupported = [label for label in labels if label not in supported]
    if len(unsupported) != 1 or len(supported) != len(labels) - 1:
        return None

    proof_ids: list[str] = []
    confidences: list[float] = []
    for label in supported:
        候補値 = 候補_by_label[label]
        confidences.append(候補値.信頼度)
        for fid in 候補値.proof_fact_ids:
            if fid and fid not in proof_ids:
                proof_ids.append(fid)
    if not proof_ids or not confidences:
        return None

    return 候補(
        answer=unsupported[0],
        関係='HDS_選択肢_exception_elimination',
        信頼度=min(confidences),
        expert="HDS_exception_elimination",
        proof_fact_ids=tuple(proof_ids),
        provenance=("HDS-IR", "K", "EXCEPTION_INTENT", "N_MINUS_ONE_DISTINCTIVE_ELIMINATION", "NO_GUESS"),
    )


@dataclass(frozen=True, slots=True)
class HDSK3結果:
    状態: str
    回答ラベル: str | None
    判定: JudgeDecision
    候補: tuple[候補, ...]
    根拠事実数: int
    理由: tuple[str, ...]
    努力水準: str = "low"
    探索深さ上限: int = 6
    証拠上限: int = 3
    候補診断: tuple[HDS候補診断, ...] = ()


class HDSIRネイティブ適合器:
    def __init__(self, 模型核: K3相当能力核 | None = None, judge: HDSJudge | None = None) -> None:
        self.模型核 = 模型核 or K3相当能力核()
        self.judge = judge or self.模型核.J

    def 実行(
        self,
        ir: HDSIR,
        *,
        候補IR: Mapping[str, HDSIR] | None = None,
        努力: str | None = None,
    ) -> HDSK3結果:
        choices = _choices(ir)
        if not choices:
            decision = JudgeDecision("SUSPEND", None, ('HDS_NO_選択肢_SET',))
            return HDSK3結果("SUSPEND", None, decision, (), 0, decision.reason_codes)

        探索方針 = HDS探索方針選択(ir, 候補IR, 指定水準=努力, controller=self.模型核.policy_controller)
        question_signature = _意味署名(ir, 代替経路_text=ir.原文)
        証拠_groups = _証拠群を作る(self.模型核)
        facts = _facts(self.模型核)
        fact_sources = _事実情報源対応(self.模型核)

        候補_signatures: dict[str, HDS意味署名] = {}
        for label, option in choices:
            候補_ir = (候補IR or {}).get(label)
            候補_signatures[label] = (
                _意味署名(候補_ir, 代替経路_text=option)
                if 候補_ir is not None
                else HDS意味署名(意味語(option), frozenset(), frozenset())
            )
        distinctive_terms = _候補識別語(候補_signatures)
        distinctive_sources: dict[str, set[str]] = {label: set() for label, _ in choices}

        raw_証拠: list[HDS候補証拠] = []
        for label, option in choices:
            候補_signature = 候補_signatures[label]
            distinctive = distinctive_terms.get(label, frozenset())

            parsed = self.模型核.R.parse(option)
            parsed_fact = getattr(parsed, "fact", None)
            if parsed_fact is not None:
                for fact in facts:
                    if _fact_blocked(fact):
                        continue
                    if str(getattr(fact, "predicate", "")) != parsed_fact.predicate:
                        continue
                    if tuple(getattr(fact, "args", ())) != parsed_fact.args:
                        continue
                    if bool(getattr(fact, '極性', True)) != parsed_fact.極性:
                        continue
                    fid = str(getattr(fact, "fact_id", ""))
                    情報源_id = _情報源群ID(fact)
                    distinctive_sources[label].add(情報源_id)
                    raw_証拠.append(
                        HDS候補証拠(label, 情報源_id, 8.0 * float(getattr(fact, '信頼度', 1.0)), (fid,) if fid else (), "direct")
                    )

            for 証拠 in 証拠_groups:
                if distinctive and (distinctive & 証拠.語):
                    distinctive_sources[label].add(証拠.出典ID)
                if 候補_signature.関係辺 and _edge_similarity(候補_signature.関係辺, 証拠.関係辺) > 0:
                    distinctive_sources[label].add(証拠.出典ID)
                score = _group_score(question_signature, 候補_signature, 証拠, 識別語=distinctive)
                if score <= 0:
                    continue
                raw_証拠.append(HDS候補証拠(label, 証拠.出典ID, score, 証拠.事実ID, 証拠.範囲))

        labels = tuple(label for label, _ in choices)
        reconciled = HDS候補横断調停(labels, raw_証拠, 証拠重み=探索方針.証拠重み, 証拠上限=探索方針.証拠上限)

        scored: list[tuple[float, 候補]] = []
        diagnostics: list[HDS候補診断] = []
        for label, option in choices:
            候補_signature = 候補_signatures[label]
            distinctive = distinctive_terms.get(label, frozenset())
            証拠_結果 = reconciled[label]
            aggregate = 証拠_結果.合計得点
            証拠_score = aggregate
            proof_ids: list[str] = []
            selected_sources = {item.出典ID for item in 証拠_結果.採用証拠}
            for item in 証拠_結果.採用証拠:
                for fid in item.事実ID:
                    if fid and fid not in proof_ids:
                        proof_ids.append(fid)

            preferred_relations = question_signature.関係種別 | 候補_signature.関係種別
            関係図_target = distinctive or 候補_signature.語
            path = HDS意味経路探索(self.模型核, question_signature.語, 関係図_target, preferred_relations, 最大深さ=4)
            if path.得点 <= 0 and 探索方針.関係図深さ上限 > 4:
                path = HDS意味経路探索(
                    self.模型核, question_signature.語, 関係図_target, preferred_relations, 最大深さ=探索方針.関係図深さ上限
                )

            関係図_score = 0.0
            関係図_factor = 0.0
            関係図_sources = {fact_sources[fid] for fid in path.事実ID if fid in fact_sources}
            if path.得点 > 0:
                if 関係図_sources:
                    novel = 関係図_sources - selected_sources
                    novelty = len(novel) / len(関係図_sources)
                    関係図_factor = 0.45 + 0.55 * novelty
                else:
                    関係図_factor = 0.65
                関係図_score = 2.5 * path.得点 * 関係図_factor
                aggregate += 関係図_score
                for fid in path.事実ID:
                    if fid and fid not in proof_ids:
                        proof_ids.append(fid)

            independent_sources = selected_sources | 関係図_sources
            matched_distinctive_sources = selected_sources & distinctive_sources.get(label, set())
            if distinctive and path.得点 > 0:
                matched_distinctive_sources |= 関係図_sources

            diagnostics.append(
                HDS候補診断(
                    候補=label,
                    合計得点=aggregate,
                    証拠得点=証拠_score,
                    関係図得点=関係図_score,
                    関係図補正係数=関係図_factor,
                    独立出典数=len(independent_sources),
                    採用証拠数=len(証拠_結果.採用証拠),
                    関係図深さ=path.深さ,
                    根拠事実数=len(proof_ids),
                    識別語数=len(distinctive),
                    識別一致出典数=len(matched_distinctive_sources),
                )
            )

            if proof_ids and aggregate > 0:
                信頼度 = min(0.999, 0.50 + aggregate / (20.0 + aggregate))
                scored.append(
                    (
                        aggregate,
                        候補(
                            answer=label,
                            関係='HDS_選択肢_selection',
                            信頼度=信頼度,
                            expert='HDS_IR_structural_関係図',
                            proof_fact_ids=tuple(proof_ids),
                            provenance=(
                                "HDS-IR", "K", 'STRUCTURAL_関係図_MATCH', '情報源_AWARE_RECONCILE',
                                '候補_DISTINCTIVE_WEIGHT', 'DIRECTED_関係_MATCH',
                                '計算量:' + 探索方針.水準,
                                "sources:" + str(len(independent_sources)),
                                "distinctive_sources:" + str(len(matched_distinctive_sources)),
                            ),
                        ),
                    )
                )

        diagnostic_tuple = tuple(sorted(diagnostics, key=lambda item: item.候補))
        scored.sort(key=lambda item: (-item[0], -item[1].信頼度, item[1].answer))
        candidates = tuple(候補 for _, 候補 in scored)

        intent = HDS選択意図判定(ir.原文)
        frame = 意味Frame(
            kind="question", intent="knowledge_query", raw=ir.原文, predicate='HDS_選択肢_selection', args=(None,),
            tags=(
                "HDS-IR", '選択肢', 'structural_関係図', '情報源_aware_reconcile',
                '候補_distinctive', 'directed_関係', intent.種別,
            ),
            言語=getattr(ir, "入力言語", "en") or "en",
        )

        if intent.種別 == "EXCEPTION":
            eliminated = _例外消去候補(choices, scored, diagnostic_tuple)
            if eliminated is None:
                decision = JudgeDecision("SUSPEND", None, ("EXCEPTION_NOT_RESOLVED", "NO_GUESS"))
                proof_count = len({fid for 候補 in candidates for fid in 候補.proof_fact_ids})
                return HDSK3結果(
                    "SUSPEND", None, decision, candidates, proof_count, decision.reason_codes,
                    探索方針.水準, 探索方針.関係図深さ上限, 探索方針.証拠上限, diagnostic_tuple,
                )
            decision = self.judge.decide(frame, (eliminated,))
            selected = decision.selected_候補.answer if decision.selected_候補 else None
            all_candidates = (eliminated, *tuple(候補 for 候補 in candidates if 候補.answer != eliminated.answer))
            return HDSK3結果(
                decision.status, selected, decision, all_candidates, len(set(eliminated.proof_fact_ids)), decision.reason_codes,
                探索方針.水準, 探索方針.関係図深さ上限, 探索方針.証拠上限, diagnostic_tuple,
            )

        if not scored:
            decision = JudgeDecision("SUSPEND", None, ('NO_KNOWLEDGE_証拠', "NO_GUESS"))
            return HDSK3結果(
                "SUSPEND", None, decision, (), 0, decision.reason_codes,
                探索方針.水準, 探索方針.関係図深さ上限, 探索方針.証拠上限, diagnostic_tuple,
            )

        top_score, top_候補 = scored[0]
        if len(scored) > 1:
            second_score = scored[1][0]
            margin = top_score - second_score
            if margin <= max(0.12, top_score * 0.02):
                decision = JudgeDecision("SUSPEND", None, ('AMBIGUOUS_証拠', "NO_GUESS"))
                proof_count = len({fid for 候補 in candidates for fid in 候補.proof_fact_ids})
                return HDSK3結果(
                    "SUSPEND", None, decision, candidates, proof_count, decision.reason_codes,
                    探索方針.水準, 探索方針.関係図深さ上限, 探索方針.証拠上限, diagnostic_tuple,
                )

        decision = self.judge.decide(frame, (top_候補,))
        selected = decision.selected_候補.answer if decision.selected_候補 else None
        proof_count = len({fid for 候補 in candidates for fid in 候補.proof_fact_ids})
        return HDSK3結果(
            decision.status, selected, decision, candidates, proof_count, decision.reason_codes,
            探索方針.水準, 探索方針.関係図深さ上限, 探索方針.証拠上限, diagnostic_tuple,
        )


__all__ = ["HDS関係辺署名", "HDS意味署名", "HDS候補診断", "HDSK3結果", 'HDSIRネイティブ適合器']

from __future__ import annotations

from collections.abc import Iterable
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass, replace

from .HDS探索方針 import HDS努力水準
from .HDS中間表現 import HDSIR
from .HDS観測計画 import HDS参照観測要求, HDS参照観測要求群
from .参照 import 参照供給器, 参照記録, 参照取得診断, 参照検索を診断


@dataclass(frozen=True, slots=True)
class HDS参照予算:
    努力水準: str
    取得上限: int
    一問合せ上限: int
    最大問合せ並列: int


@dataclass(frozen=True, slots=True)
class _HDS問合せ仕様:
    問合せ: str
    種別: str
    候補: str | None = None
    観測ID: str | None = None
    関係ID: str | None = None
    必須被覆: bool = False


def HDS参照予算選択(ir: HDSIR) -> HDS参照予算:
    level = HDS努力水準(ir)
    if level == "max":
        return HDS参照予算(level, 16, 4, 4)
    if level == "high":
        return HDS参照予算(level, 12, 4, 4)
    return HDS参照予算(level, 6, 3, 2)


def _切詰め(text: str, limit: int) -> str:
    value = " ".join(str(text).split()).strip()
    if len(value) <= limit:
        return value
    parts = value.split()
    if not parts or limit <= 0:
        return ""
    head_予算 = max(1, int(limit * 0.58))
    tail_予算 = max(1, limit - head_予算 - 1)
    head: list[str] = []
    size = 0
    split_index = 0
    for index, part in enumerate(parts):
        extra = len(part) + (1 if head else 0)
        if size + extra > head_予算:
            split_index = index
            break
        head.append(part)
        size += extra
        split_index = index + 1
    tail_rev: list[str] = []
    size = 0
    for part in reversed(parts[split_index:]):
        extra = len(part) + (1 if tail_rev else 0)
        if size + extra > tail_予算:
            break
        tail_rev.append(part)
        size += extra
    tail = list(reversed(tail_rev))
    return " ".join(head) if not tail else " ".join((*head, *tail))


def _仕様化(request: HDS参照観測要求) -> _HDS問合せ仕様 | None:
    query = _切詰め(request.外部検索表層, 360)
    if not query:
        return None
    if request.候補ラベル is not None:
        kind = "選択肢" if request.段階 == "primary" else "代替経路_選択肢"
    elif request.関係ID is not None:
        kind = "関係" if request.段階 == "primary" else "代替経路_関係"
    elif request.ID.startswith("検索表層:"):
        kind = "外部検索表層"
    elif request.ID.startswith("監査表層:"):
        kind = "audit_probe"
    else:
        kind = "代替経路"
    return _HDS問合せ仕様(
        query,
        kind,
        request.候補ラベル,
        request.ID,
        request.関係ID,
        request.必須被覆,
    )


def _要求群(
    ir: HDSIR,
    観測要求: Iterable[HDS参照観測要求] | None = None,
) -> tuple[HDS参照観測要求, ...]:
    if 観測要求 is not None:
        return tuple(観測要求)
    return HDS参照観測要求群(ir)


def _問合せ仕様(
    ir: HDSIR,
    *,
    最大候補数: int = 6,
    観測要求: Iterable[HDS参照観測要求] | None = None,
) -> tuple[_HDS問合せ仕様, ...]:
    """Compilerが形成したprimary観測要求だけをprovider queryへ降下する。"""
    requests = tuple(request for request in _要求群(ir, 観測要求) if request.段階 == "primary")
    required = tuple(request for request in requests if request.必須被覆)
    optional = tuple(request for request in requests if not request.必須被覆)

    仕様予算 = max(max(0, int(最大候補数)), len(required))
    selected = (*required, *optional[:max(0, 仕様予算 - len(required))])
    specs = tuple(spec for request in selected if (spec := _仕様化(request)) is not None)
    return _重複仕様除去(specs)


def HDS参照問合せ候補(
    ir: HDSIR,
    *,
    最大候補数: int = 6,
    観測要求: Iterable[HDS参照観測要求] | None = None,
) -> tuple[str, ...]:
    return tuple(
        spec.問合せ
        for spec in _問合せ仕様(ir, 最大候補数=最大候補数, 観測要求=観測要求)
    )


def _縮退仕様(
    ir: HDSIR,
    *,
    観測要求: Iterable[HDS参照観測要求] | None = None,
) -> tuple[_HDS問合せ仕様, ...]:
    specs = tuple(
        spec
        for request in _要求群(ir, 観測要求)
        if request.段階 == "fallback"
        if (spec := _仕様化(request)) is not None
    )
    return _重複仕様除去(specs)


def HDS参照縮退問合せ候補(
    ir: HDSIR,
    *,
    観測要求: Iterable[HDS参照観測要求] | None = None,
) -> tuple[str, ...]:
    return tuple(spec.問合せ for spec in _縮退仕様(ir, 観測要求=観測要求))


def _重複仕様除去(specs: Iterable[_HDS問合せ仕様]) -> tuple[_HDS問合せ仕様, ...]:
    out: list[_HDS問合せ仕様] = []
    seen: set[tuple[str, str | None]] = set()
    for spec in specs:
        key = (spec.問合せ.casefold(), spec.観測ID)
        if key in seen:
            continue
        seen.add(key)
        out.append(spec)
    return tuple(out)


def _条件追加(record: 参照記録, spec: _HDS問合せ仕様) -> 参照記録:
    conditions = list(record.条件)
    additions: list[tuple[str, str]] = [("hds_query_kind", spec.種別)]
    if spec.候補 is not None:
        additions.append(("hds_query_選択肢", spec.候補))
    if spec.観測ID is not None:
        additions.append(("hds_observation_id", spec.観測ID))
    if spec.関係ID is not None:
        additions.append(("hds_query_relation", spec.関係ID))
    if spec.必須被覆:
        additions.append(("hds_observation_required", "true"))
    for item in additions:
        if item not in conditions:
            conditions.append(item)
    return replace(record, 条件=tuple(conditions))


def _query_pools(
    provider: 参照供給器,
    specs: tuple[_HDS問合せ仕様, ...],
    per_query_limit: int,
    *,
    max_parallel: int,
    診断収集: list[参照取得診断] | None = None,
) -> list[tuple[参照記録, ...]]:
    def run(spec: _HDS問合せ仕様) -> tuple[tuple[参照記録, ...], 参照取得診断]:
        記録群, 診断 = 参照検索を診断(provider, spec.問合せ, per_query_limit)
        return tuple(_条件追加(record, spec) for record in 記録群), 診断

    parallel_safe = bool(getattr(provider, "並列安全", False))
    if not parallel_safe or len(specs) <= 1 or max_parallel <= 1:
        pools: list[tuple[参照記録, ...]] = []
        for spec in specs:
            記録群, 診断 = run(spec)
            pools.append(記録群)
            if 診断収集 is not None:
                診断収集.append(診断)
        return pools

    workers = min(max(1, int(max_parallel)), len(specs))
    with ThreadPoolExecutor(max_workers=workers, thread_name_prefix="minidora-rq") as executor:
        futures = [executor.submit(run, spec) for spec in specs]
        pools = []
        for future in futures:
            try:
                記録群, 診断 = future.result()
            except Exception as exc:
                記録群 = ()
                診断 = 参照取得診断(
                    "", str(getattr(provider, "名称", type(provider).__name__)), "失敗", 0, 1,
                    f"{type(exc).__name__}: {exc}",
                )
            pools.append(tuple(記録群))
            if 診断収集 is not None:
                診断収集.append(診断)
        return pools


def _記録統合(old: 参照記録, new: 参照記録) -> 参照記録:
    conditions = list(old.条件)
    for condition in new.条件:
        if condition not in conditions:
            conditions.append(condition)
    return replace(old, 条件=tuple(conditions), 信頼=max(float(old.信頼), float(new.信頼)))


def _round_robin(pools: Iterable[tuple[参照記録, ...]], limit: int) -> tuple[参照記録, ...]:
    pools_tuple = tuple(pools)
    結果: list[参照記録] = []
    index_by_id: dict[str, int] = {}
    depth = 0
    while True:
        progressed = False
        for pool in pools_tuple:
            if depth >= len(pool):
                continue
            progressed = True
            record = pool[depth]
            existing = index_by_id.get(record.識別子)
            if existing is not None:
                結果[existing] = _記録統合(結果[existing], record)
                continue
            if len(結果) >= limit:
                continue
            index_by_id[record.識別子] = len(結果)
            結果.append(record)
        if not progressed:
            break
        depth += 1
    return tuple(結果)


def _観測取得被覆(records: Iterable[参照記録]) -> frozenset[str]:
    covered: set[str] = set()
    for record in records:
        for key, value in record.条件:
            if str(key) == "hds_observation_id" and str(value):
                covered.add(str(value))
    return frozenset(covered)


def _候補被覆(records: Iterable[参照記録]) -> frozenset[str]:
    """取得済みrecordのquery provenanceから候補ラベル被覆だけを読む互換境界。"""
    labels: set[str] = set()
    for record in records:
        for key, value in record.条件:
            if str(key) == "hds_query_選択肢" and str(value):
                labels.add(str(value))
    return frozenset(labels)


def _必須観測(
    ir: HDSIR,
    観測要求: Iterable[HDS参照観測要求] | None = None,
) -> frozenset[str]:
    return frozenset(request.ID for request in _要求群(ir, 観測要求) if request.必須被覆)


def _記録群統合(first: Iterable[参照記録], second: Iterable[参照記録], limit: int) -> tuple[参照記録, ...]:
    結果: list[参照記録] = []
    index_by_id: dict[str, int] = {}
    for record in (*tuple(first), *tuple(second)):
        existing = index_by_id.get(record.識別子)
        if existing is not None:
            結果[existing] = _記録統合(結果[existing], record)
            continue
        if len(結果) >= limit:
            continue
        index_by_id[record.識別子] = len(結果)
        結果.append(record)
    return tuple(結果)


def HDS参照検索(
    provider: 参照供給器,
    ir: HDSIR,
    *,
    上限: int | None = None,
    一問合せ上限: int | None = None,
    最大問合せ並列: int | None = None,
    観測要求: Iterable[HDS参照観測要求] | None = None,
    診断収集: list[参照取得診断] | None = None,
) -> tuple[参照記録, ...]:
    予算 = HDS参照予算選択(ir)
    total_limit = 予算.取得上限 if 上限 is None else max(0, int(上限))
    per_query = 予算.一問合せ上限 if 一問合せ上限 is None else max(1, int(一問合せ上限))
    parallel = 予算.最大問合せ並列 if 最大問合せ並列 is None else max(1, int(最大問合せ並列))
    if total_limit <= 0:
        return ()

    requests = _要求群(ir, 観測要求)
    primary_specs = _問合せ仕様(ir, 観測要求=requests)
    primary_records = (
        _round_robin(_query_pools(provider, primary_specs, per_query, max_parallel=parallel, 診断収集=診断収集), total_limit)
        if primary_specs
        else ()
    )
    required = _必須観測(ir, requests)
    coverage = set(_観測取得被覆(primary_records))
    missing = required - coverage

    if primary_records and not missing:
        return primary_records

    # primaryで必須観測が欠けたら、同じ実行内ですぐ局所検証・縮退へ降下する。
    # 外部取得揺れを未観測のまま次段へ送らず、候補局所queryで先に回収する。
    縮退仕様群 = _縮退仕様(ir, 観測要求=requests)
    if not 縮退仕様群:
        return primary_records
    filtered_specs = tuple(
        spec
        for spec in 縮退仕様群
        if not spec.必須被覆 or spec.観測ID in missing
    )
    if not filtered_specs:
        return primary_records
    縮退記録群 = _round_robin(
        _query_pools(provider, filtered_specs, per_query, max_parallel=parallel, 診断収集=診断収集),
        total_limit,
    )
    return _記録群統合(primary_records, 縮退記録群, total_limit)


__all__ = [
    "HDS参照予算",
    "HDS参照予算選択",
    "HDS参照問合せ候補",
    "HDS参照縮退問合せ候補",
    "HDS参照検索",
]

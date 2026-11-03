from __future__ import annotations

from dataclasses import dataclass, replace
from hashlib import sha256
import json
import unicodedata
from typing import Iterable, Sequence

from .意味字句 import 意味語
from .参照 import 参照記録


def _正規化(value: object) -> str:
    return " ".join(unicodedata.normalize("NFKC", str(value)).split()).strip()


def _安定ID(prefix: str, value: object) -> str:
    raw = json.dumps(value, ensure_ascii=False, sort_keys=True, default=str)
    return prefix + sha256(raw.encode("utf-8")).hexdigest()[:20]


def _参照revision(references: Sequence[参照記録]) -> str:
    payload = tuple(
        (
            str(record.識別子),
            sha256(str(record.内容).encode("utf-8")).hexdigest(),
            float(record.信頼),
        )
        for record in references
    )
    return _安定ID("RIDX-", payload)


@dataclass(frozen=True, slots=True)
class HDS参照索引Bucket:
    """正本参照を変更せずに持つ、検索専用の粗い索引bucket。

    `参照ID群` は正本へのpointerであり、内容そのものを代替しない。
    `末尾参照ID` はbucket圧縮時にも直近境界を失わないための補助情報である。
    """

    bucketID: str
    参照ID群: tuple[str, ...]
    意味語群: tuple[str, ...]
    末尾参照ID: str


@dataclass(frozen=True, slots=True)
class HDS参照索引:
    revision: str
    bucket幅: int
    bucket群: tuple[HDS参照索引Bucket, ...]
    正本参照数: int


@dataclass(frozen=True, slots=True)
class HDS参照計画:
    """同一request内で再利用できる参照選択結果。

    計画は参照IDだけを保持する。正本証拠は保持・圧縮・改変しない。
    `残存利用回数` が0、問い署名が変化、索引revisionが変化、または明示無効化された時点で失効する。
    """

    計画ID: str
    問い署名: str
    索引revision: str
    参照ID群: tuple[str, ...]
    利用上限: int
    残存利用回数: int
    無効理由: str | None = None

    @property
    def 有効(self) -> bool:
        return self.無効理由 is None and self.残存利用回数 > 0


def HDS参照索引圧縮(
    references: Iterable[参照記録],
    *,
    bucket幅: int = 4,
    bucket意味語上限: int = 64,
) -> HDS参照索引:
    """参照正本とは別に、複数参照を一つの粗い検索bucketへ圧縮する。

    圧縮対象は索引だけであり、元の`参照記録.内容`は一切変更しない。
    """

    records = tuple(references)
    width = max(1, int(bucket幅))
    term_limit = max(1, int(bucket意味語上限))
    buckets: list[HDS参照索引Bucket] = []

    for start in range(0, len(records), width):
        group = records[start : start + width]
        ids = tuple(str(record.識別子) for record in group)
        terms: set[str] = set()
        for record in group:
            terms.update(意味語(_正規化(record.内容)))
        ordered = tuple(sorted(terms)[:term_limit])
        tail = ids[-1] if ids else ""
        buckets.append(
            HDS参照索引Bucket(
                _安定ID("B-", (ids, ordered, tail)),
                ids,
                ordered,
                tail,
            )
        )

    return HDS参照索引(
        _参照revision(records),
        width,
        tuple(buckets),
        len(records),
    )


def _問い署名(question: object) -> str:
    normalized = _正規化(question)
    return _安定ID("Q-", (normalized, tuple(sorted(意味語(normalized)))))


def HDS参照計画作成(
    question: object,
    references: Iterable[参照記録],
    *,
    索引: HDS参照索引 | None = None,
    参照上限: int | None = None,
    利用上限: int = 4,
) -> HDS参照計画:
    """粗い索引で参照IDを選び、正確な証拠読取計画を固定する。

    `参照上限=None`では取得済み参照を捨てず、関連bucket順へ並べるだけにする。
    """

    records = tuple(references)
    index = 索引 or HDS参照索引圧縮(records)
    if index.revision != _参照revision(records):
        raise ValueError("参照索引と参照正本のrevisionが一致しない")

    query_terms = 意味語(_正規化(question))
    scored: list[tuple[int, int, HDS参照索引Bucket]] = []
    for position, bucket in enumerate(index.bucket群):
        overlap = len(query_terms.intersection(bucket.意味語群))
        scored.append((overlap, -position, bucket))
    scored.sort(key=lambda row: (-row[0], -row[1], row[2].bucketID))

    ordered_ids: list[str] = []
    seen: set[str] = set()
    for _overlap, _position, bucket in scored:
        for 情報源_id in bucket.参照ID群:
            if 情報源_id not in seen:
                seen.add(情報源_id)
                ordered_ids.append(情報源_id)

    limit = len(ordered_ids) if 参照上限 is None else max(0, int(参照上限))
    selected = tuple(ordered_ids[:limit])
    max_uses = max(1, int(利用上限))
    qsig = _問い署名(question)
    return HDS参照計画(
        _安定ID("RP-", (qsig, index.revision, selected, max_uses)),
        qsig,
        index.revision,
        selected,
        max_uses,
        max_uses,
    )


def HDS参照計画適用(
    plan: HDS参照計画,
    references: Iterable[参照記録],
) -> tuple[参照記録, ...]:
    """計画IDから正本参照を再読する。索引内容を証拠として返さない。"""

    records = tuple(references)
    if not plan.有効:
        raise ValueError("失効した参照計画は適用できない")
    revision = _参照revision(records)
    if revision != plan.索引revision:
        raise ValueError("参照正本が変化しているため計画を再利用できない")

    by_id = {str(record.識別子): record for record in records}
    missing = tuple(情報源_id for 情報源_id in plan.参照ID群 if 情報源_id not in by_id)
    if missing:
        raise KeyError("参照計画の正本参照が欠落している: " + ",".join(missing))
    return tuple(by_id[情報源_id] for 情報源_id in plan.参照ID群)


def HDS参照計画再利用可能(
    plan: HDS参照計画,
    question: object,
    references: Iterable[参照記録],
) -> bool:
    records = tuple(references)
    return bool(
        plan.有効
        and plan.問い署名 == _問い署名(question)
        and plan.索引revision == _参照revision(records)
    )


def HDS参照計画消費(plan: HDS参照計画) -> HDS参照計画:
    if not plan.有効:
        return plan
    return replace(plan, 残存利用回数=max(0, plan.残存利用回数 - 1))


def HDS参照計画無効化(plan: HDS参照計画, 理由: str) -> HDS参照計画:
    reason = _正規化(理由) or "参照条件変化"
    return replace(plan, 残存利用回数=0, 無効理由=reason)


__all__ = [
    "HDS参照索引Bucket",
    "HDS参照索引",
    "HDS参照計画",
    "HDS参照索引圧縮",
    "HDS参照計画作成",
    "HDS参照計画適用",
    "HDS参照計画再利用可能",
    "HDS参照計画消費",
    "HDS参照計画無効化",
]

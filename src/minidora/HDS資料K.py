from __future__ import annotations

from dataclasses import dataclass
from hashlib import sha256
import json
import re
from typing import Iterable

from .HDS構文化記録 import HDS_構文化器_META_PREFIXES
from .HDS中間表現 import HDSIR, HDS関係, 値状態
from .K3機能 import Fact, K3相当能力核


_SURFACE_ONLY_KINDS = {
    '情報源_text',
    '言語.input',
    '言語.normalized',
    "対象.原文保持",
    "文脈.言語",
}
_証拠_ATTR = '_hds_証拠_facts'
_関係図_REVISION_ATTR = '_hds_関係図_revision'
_関係図_CACHE_ATTR = '_hds_関係図_index_cache'
_関係_修飾_KEYS = frozenset({"様相", '条件範囲', "量化", '範囲', "条件作用"})


def _predicate(kind: str) -> str:
    normalized = re.sub(r"\s+", " ", str(kind)).strip()
    return 'hds_関係_' + (normalized or '未知')


def _text(value: object) -> str:
    if value is None:
        return ""
    return re.sub(r"\s+", " ", str(value)).strip()


def _信頼度(状態: 値状態) -> float:
    if 状態 == 値状態.確定:
        return 1.0
    if 状態 == 値状態.推定:
        return 0.86
    if 状態 in {値状態.未確定, 値状態.未観測, 値状態.留保}:
        return 0.55
    if 状態 == 値状態.矛盾:
        return 0.25
    return 0.5


def _情報源信頼度(value: float) -> float:
    return min(1.0, max(0.0, float(value)))


def _combined_信頼度(状態: 値状態, 情報源_信頼度: float) -> float:
    return _信頼度(状態) * _情報源信頼度(情報源_信頼度)


def _状態印(状態: 値状態) -> str:
    return 'value_状態:' + 状態.value


def _情報源印(value: float) -> str:
    return f"情報源_信頼度:{_情報源信頼度(value):.6f}"


def _関係条件(関係: HDS関係, key: str) -> str:
    prefix = key + "="
    for raw in 関係.条件:
        value = _text(raw)
        if value.startswith(prefix):
            return value[len(prefix):].strip()
    return ""


def _関係極性(関係: HDS関係) -> bool:
    value = _関係条件(関係, "極性")
    return value != "否定"


def _関係修飾(関係: HDS関係) -> tuple[tuple[str, str], ...]:
    'HDS 関係条件のうち、世界関係の意味identityに属する修飾だけを正規化する。'
    out: list[tuple[str, str]] = []
    seen: set[tuple[str, str]] = set()
    for raw in 関係.条件:
        value = _text(raw)
        key, sep, payload = value.partition("=")
        if not sep:
            continue
        key = key.strip()
        payload = payload.strip()
        if key not in _関係_修飾_KEYS or not payload:
            continue
        item = (key, payload)
        if item in seen:
            continue
        seen.add(item)
        out.append(item)
    return tuple(sorted(out))


def _極性_marker(value: bool) -> str:
    return '関係_極性:' + ("positive" if value else "negative")


def _修飾_markers(values: tuple[tuple[str, str], ...]) -> tuple[str, ...]:
    return tuple(f"関係_修飾:{key}={value}" for key, value in values)


def _qualified_fact_id(
    predicate: str,
    args: tuple[str, ...],
    極性: bool,
    修飾: tuple[tuple[str, str], ...],
    provenance: tuple[str, ...],
) -> str:
    raw = json.dumps((predicate, args, 極性, 修飾, provenance), ensure_ascii=False, sort_keys=True, default=str)
    return "HF-" + sha256(raw.encode("utf-8")).hexdigest()[:12]


@dataclass(frozen=True)
class HDS修飾Fact(Fact):
    '既存K Factへ無条件化せず保持するHDS関係Fact。\n\n    修飾はHDS証拠台帳上の意味identityであり、現行canonical Kの無条件推論へは投入しない。\n    '

    修飾: tuple[tuple[str, str], ...] = ()

    def __post_init__(self) -> None:
        normalized = tuple(sorted((str(k).strip(), str(v).strip()) for k, v in self.修飾 if str(k).strip() and str(v).strip()))
        object.__setattr__(self, '修飾', normalized)
        if not self.fact_id:
            object.__setattr__(
                self,
                "fact_id",
                _qualified_fact_id(self.predicate, self.args, self.極性, normalized, self.provenance),
            )

    def key(self) -> tuple[str, tuple[str, ...], bool, tuple[tuple[str, str], ...]]:
        return self.predicate, self.args, self.極性, self.修飾


def _証拠台帳(模型核: K3相当能力核) -> dict[str, Fact]:
    ledger = getattr(模型核.K, _証拠_ATTR, None)
    if ledger is None:
        ledger = {}
        setattr(模型核.K, _証拠_ATTR, ledger)
    return ledger


def _関係図索引無効化(模型核: K3相当能力核) -> None:
    revision = int(getattr(模型核.K, _関係図_REVISION_ATTR, 0)) + 1
    setattr(模型核.K, _関係図_REVISION_ATTR, revision)
    if hasattr(模型核.K, _関係図_CACHE_ATTR):
        delattr(模型核.K, _関係図_CACHE_ATTR)


def _normalize_requested_修飾(values: tuple[tuple[str, str], ...]) -> tuple[tuple[str, str], ...]:
    return tuple(sorted((str(k).strip(), str(v).strip()) for k, v in values if str(k).strip() and str(v).strip()))


def HDS証拠事実(
    模型核: K3相当能力核,
    *,
    極性: bool | None = True,
    修飾: tuple[tuple[str, str], ...] | None = (),
) -> tuple[Fact, ...]:
    'HDS証拠台帳を極性・関係修飾のviewで返す。\n\n    既定は positive かつ無修飾のみ。したがって現行候補比較・関係図・direct 検証器へ\n    否定Factやmodal/条件付きFactは流れない。\n\n    - `極性=None`: 正負を問わない\n    - `修飾=None`: 修飾を問わない\n    - `修飾=(("様相", "可能"),)`: その修飾identityだけ\n    '
    ledger = getattr(模型核.K, _証拠_ATTR, {})
    values = tuple(ledger.values())
    if 極性 is not None:
        values = tuple(fact for fact in values if bool(fact.極性) is bool(極性))
    if 修飾 is not None:
        requested = _normalize_requested_修飾(修飾)
        values = tuple(fact for fact in values if tuple(getattr(fact, '修飾', ())) == requested)
    return values


def HDS証拠状態複製(情報源: K3相当能力核, destination: K3相当能力核) -> None:
    ledger = getattr(情報源.K, _証拠_ATTR, None)
    if ledger is not None:
        setattr(destination.K, _証拠_ATTR, dict(ledger))
    revision = int(getattr(情報源.K, _関係図_REVISION_ATTR, 0))
    setattr(destination.K, _関係図_REVISION_ATTR, revision)
    if hasattr(destination.K, _関係図_CACHE_ATTR):
        delattr(destination.K, _関係図_CACHE_ATTR)


def _残差阻害(ir: HDSIR) -> tuple[bool, dict[str, tuple[str, ...]]]:
    情報源_blocked = any(item.種別 == '意味_loss' for item in ir.残差)
    impacted: dict[str, list[str]] = {}
    for 残差 in ir.残差:
        for coordinate_id in 残差.影響座標:
            impacted.setdefault(str(coordinate_id), []).append(str(残差.種別))
    return 情報源_blocked, {key: tuple(values) for key, values in impacted.items()}


def _残差marker(情報源_blocked: bool, kinds: Iterable[str]) -> tuple[str, ...]:
    kinds_tuple = tuple(str(kind) for kind in kinds)
    blocked = 情報源_blocked or bool(kinds_tuple)
    markers: list[str] = []
    if blocked:
        markers.append('value_状態:留保')
    if 情報源_blocked:
        markers.append('残差_blocked:意味_loss')
    markers.extend('残差_blocked:' + kind for kind in kinds_tuple)
    return tuple(dict.fromkeys(markers))


@dataclass(frozen=True, slots=True)
class HDS知識投入結果:
    追加事実数: int
    座標事実数: int
    関係事実数: int
    残差数: int
    意味_loss: bool
    証拠事実数: int = 0
    証拠阻害事実数: int = 0
    情報源_信頼度: float = 1.0
    修飾関係事実数: int = 0


class HDSIR知識適合器:
    'コンパイル済みHDS-IRをKへ投入する一般適合器。\n\n    無修飾のFactは従来canonical Kへ投入する。modal/条件/量化等の関係修飾を持つFactは\n    `HDS修飾Fact` としてHDS証拠台帳へ保持するが、無条件のcanonical Kへは投入しない。\n    これにより意味を失わず、既存推論へ誤混入させない。\n    '

    def __init__(self, 模型核: K3相当能力核) -> None:
        self.模型核 = 模型核

    def 投入(
        self,
        ir: HDSIR,
        *,
        provenance: Iterable[str] = (),
        信頼係数: float = 1.0,
    ) -> HDS知識投入結果:
        情報源 = tuple(str(x) for x in provenance)
        情報源_信頼度 = _情報源信頼度(信頼係数)
        情報源_marker = _情報源印(情報源_信頼度)
        coords = ir.座標辞書()
        facts: list[Fact] = []
        canonical_facts: list[Fact] = []
        coord_count = 0
        関係_count = 0
        qualified_関係_count = 0
        blocked_count = 0
        情報源_blocked, impacted = _残差阻害(ir)

        for coord in ir.座標:
            kind = _text(coord.種別)
            if kind in _SURFACE_ONLY_KINDS or kind.startswith(HDS_構文化器_META_PREFIXES):
                continue
            content = _text(coord.内容)
            if not content:
                continue
            残差_markers = _残差marker(情報源_blocked, impacted.get(coord.座標ID, ()))
            if 残差_markers:
                blocked_count += 1
            fact = Fact(
                "hds_coordinate",
                (kind, content),
                信頼度=_combined_信頼度(coord.値状態, 情報源_信頼度),
                provenance=情報源 + ("HDS-IR", coord.座標ID, _状態印(coord.値状態), 情報源_marker, *残差_markers, _text(coord.由来), _text(coord.暫定性)),
            )
            facts.append(fact)
            canonical_facts.append(fact)
            coord_count += 1

        for 関係 in ir.関係:
            starts = tuple(_text(coords[x].内容) for x in 関係.始点 if x in coords and _text(coords[x].内容))
            ends = tuple(_text(coords[x].内容) for x in 関係.終点 if x in coords and _text(coords[x].内容))
            if not starts and not ends:
                continue
            affected_kinds: list[str] = []
            for coordinate_id in (*関係.始点, *関係.終点):
                affected_kinds.extend(impacted.get(coordinate_id, ()))
            残差_markers = _残差marker(情報源_blocked, affected_kinds)
            if 残差_markers:
                blocked_count += 1
            極性 = _関係極性(関係)
            修飾 = _関係修飾(関係)
            provenance_value = 情報源 + (
                "HDS-IR", 関係.関係ID, _状態印(関係.値状態), 情報源_marker,
                *残差_markers, '関係_type:' + _text(関係.種別), _極性_marker(極性),
                *_修飾_markers(修飾), _text(関係.由来), _text(関係.暫定性),
            )
            if 修飾:
                fact = HDS修飾Fact(
                    _predicate(関係.種別),
                    starts + ("→",) + ends,
                    極性=極性,
                    信頼度=_combined_信頼度(関係.値状態, 情報源_信頼度),
                    provenance=provenance_value,
                    修飾=修飾,
                )
                qualified_関係_count += 1
            else:
                fact = Fact(
                    _predicate(関係.種別),
                    starts + ("→",) + ends,
                    極性=極性,
                    信頼度=_combined_信頼度(関係.値状態, 情報源_信頼度),
                    provenance=provenance_value,
                )
                canonical_facts.append(fact)
            facts.append(fact)
            関係_count += 1

        for 残差 in ir.残差:
            fact = Fact(
                'hds_残差',
                (_text(残差.種別), _text(残差.原文), _text(残差.理由), *tuple(_text(x) for x in 残差.影響座標)),
                信頼度=0.35 * 情報源_信頼度,
                provenance=情報源 + ("HDS-IR", 残差.残差ID, 'value_状態:留保', 情報源_marker, *tuple("impact:" + str(x) for x in 残差.影響座標)),
            )
            facts.append(fact)
            canonical_facts.append(fact)

        ledger = _証拠台帳(self.模型核)
        for fact in facts:
            ledger.setdefault(fact.fact_id, fact)

        added = self.模型核.K.add_many(canonical_facts)
        _関係図索引無効化(self.模型核)
        return HDS知識投入結果(
            追加事実数=added,
            座標事実数=coord_count,
            関係事実数=関係_count,
            残差数=len(ir.残差),
            意味_loss=情報源_blocked,
            証拠事実数=len(facts),
            証拠阻害事実数=blocked_count,
            情報源_信頼度=情報源_信頼度,
            修飾関係事実数=qualified_関係_count,
        )


__all__ = [
    "HDS知識投入結果",
    "HDS修飾Fact",
    'HDSIR知識適合器',
    "HDS証拠事実",
    "HDS証拠状態複製",
]

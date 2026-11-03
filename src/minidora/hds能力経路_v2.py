from __future__ import annotations

from dataclasses import replace
import unicodedata
from typing import Iterable, Mapping, TYPE_CHECKING

from .HDS選択実行系 import HDS選択実行結果
from .HDS中間表現 import HDSIR
from .HDS観測計画 import HDS参照観測要求群
from .HDS参照 import (
    HDS参照予算選択,
    HDS参照検索,
    _候補被覆,
    _query_pools,
    _round_robin,
    _縮退仕様,
    _記録統合,
)
from .HDS候補提案実行系 import HDS候補提案実行
from .hds参照拡張 import HDS参照検索強化
from .hds局所再照合 import HDS局所Window候補
if TYPE_CHECKING:
    from .K3機能 import K3相当能力核
from .模型 import MINIDORA模型核, 模型結果
from .能力状態差循環 import MINIDORA能力状態差模型核, 標準能力模型核
from .参照 import 参照供給器, 参照記録


def _正規化(text: object) -> str:
    return " ".join(unicodedata.normalize("NFKC", str(text)).split()).strip()


def _候補ラベル群(record: 参照記録) -> frozenset[str]:
    return frozenset(
        str(value)
        for key, value in record.条件
        if str(key) == 'hds_query_選択肢' and str(value)
    )


def HDS候補被覆優先統合(
    primary: Iterable[参照記録],
    extra: Iterable[参照記録],
    expected_labels: Iterable[str],
    limit: int,
) -> tuple[参照記録, ...]:
    '候補別queryで観測できた情報源を対称に残してから、残枠へ通常順を戻す。\n\n    `hds_query_選択肢` は検索経路情報であり、候補真偽の票としては使わない。\n    同一情報源は1件へ統合し、query provenanceだけを併合する。\n    '

    total_limit = max(0, int(limit))
    if total_limit <= 0:
        return ()

    combined: list[参照記録] = []
    index_by_id: dict[str, int] = {}
    for record in (*tuple(primary), *tuple(extra)):
        情報源_id = str(record.識別子)
        existing = index_by_id.get(情報源_id)
        if existing is not None:
            combined[existing] = _記録統合(combined[existing], record)
            continue
        index_by_id[情報源_id] = len(combined)
        combined.append(record)

    selected: list[参照記録] = []
    selected_ids: set[str] = set()
    for label in sorted({str(item) for item in expected_labels if str(item)}):
        if any(label in _候補ラベル群(record) for record in selected):
            continue
        候補 = next(
            (
                record
                for record in combined
                if str(record.識別子) not in selected_ids and label in _候補ラベル群(record)
            ),
            None,
        )
        if 候補 is None:
            continue
        selected.append(候補)
        selected_ids.add(str(候補.識別子))
        if len(selected) >= total_limit:
            return tuple(selected)

    for record in combined:
        情報源_id = str(record.識別子)
        if 情報源_id in selected_ids:
            continue
        selected.append(record)
        selected_ids.add(情報源_id)
        if len(selected) >= total_limit:
            break
    return tuple(selected)


def HDS参照検索V2(
    provider: 参照供給器,
    ir: HDSIR,
    *,
    上限: int | None = None,
    一問合せ上限: int | None = None,
    最大問合せ並列: int | None = None,
    最大候補補完回数: int = 1,
    観測要求=None,
) -> tuple[参照記録, ...]:
    """互換入口。R閉包の実体はCompiler Kernel観測要求対応の共通実装へ一本化する。"""
    return HDS参照検索強化(
        provider,
        ir,
        上限=上限,
        一問合せ上限=一問合せ上限,
        最大問合せ並列=最大問合せ並列,
        最大候補補完回数=最大候補補完回数,
        観測要求=観測要求,
    )


def HDS能力模型核V2(模型核: MINIDORA模型核 | None = None) -> MINIDORA模型核:
    '同資料の候補縮小再投票を無効化した能力模型核を返す。\n\n    外界観測が変わらない内部再作用は行わず、観測viewが変化した場合の再評価は\n    `HDS能力経路V2候補提案実行` が全候補で新しい評価Runとして行う。\n    '

    base = 模型核 or 標準能力模型核()
    if not isinstance(base, MINIDORA能力状態差模型核):
        return base
    return MINIDORA能力状態差模型核(
        base.関係群,
        言語対応_=base.言語対応,
        能力作用群=base.能力作用群,
        形成済み関係群=base.形成済み関係群,
        最大再作用回数=0,
    )


def HDS局所観測view(
    question_ir: HDSIR,
    references: tuple[参照記録, ...],
    *,
    上限: int = 12,
) -> tuple[tuple[参照記録, ...], int]:
    '同一情報源の全文viewを最上位local windowへ置換する。\n\n    情報源 ID・provider・origin・信頼度・query provenanceは保持し、独立情報源数を増やさない。\n    '

    windows = HDS局所Window候補(question_ir, references, 上限=max(0, int(上限)))
    best_by_情報源 = {}
    for row in windows:
        情報源_id = str(row.参照.識別子)
        if 情報源_id not in best_by_情報源:
            best_by_情報源[情報源_id] = row

    changed = 0
    projected: list[参照記録] = []
    for record in references:
        row = best_by_情報源.get(str(record.識別子))
        if row is None or _正規化(row.内容).casefold() == _正規化(record.内容).casefold():
            projected.append(record)
            continue
        conditions = list(record.条件)
        marker = ("hds_observation_view", "local")
        if marker not in conditions:
            conditions.append(marker)
        projected.append(replace(record, 内容=row.内容, 条件=tuple(conditions)))
        changed += 1
    return tuple(projected), changed


def _寄与参照ID群(結果: HDS選択実行結果) -> frozenset[str]:
    模型 = 結果.MINIDORA模型結果
    label = 結果.回答ラベル
    if 模型 is None or label is None:
        return frozenset()
    row = next((item for item in 模型.候補差 if item.候補ID == label), None)
    if row is None:
        return frozenset()

    情報源_ids: set[str] = set()
    for contribution in row.寄与:
        for raw in contribution.根拠:
            text = str(raw)
            if text.startswith("参照:"):
                payload = text[len("参照:"):]
                情報源_id = payload.rsplit(":", 1)[0] if ":" in payload else payload
                if 情報源_id:
                    情報源_ids.add(情報源_id)
            elif text.startswith("再照合:"):
                payload = text[len("再照合:"):]
                parts = payload.rsplit(":", 2)
                情報源_id = parts[0] if parts else payload
                if 情報源_id:
                    情報源_ids.add(情報源_id)
    return frozenset(情報源_ids)


def _提案強度(結果: HDS選択実行結果) -> tuple[int, int, int]:
    if 結果.状態 != "PROPOSE" or 結果.回答ラベル is None or 結果.MINIDORA模型結果 is None:
        return (-1, -10**9, -10**9)
    scores = 結果.MINIDORA模型結果.候補辞書()
    if 結果.回答ラベル not in scores:
        return (-1, -10**9, -10**9)
    top = int(scores[結果.回答ラベル])
    second = max((int(value) for key, value in scores.items() if key != 結果.回答ラベル), default=0)
    return (len(_寄与参照ID群(結果)), top - second, top)


def _局所再評価採用可能(initial: HDS選択実行結果, rechecked: HDS選択実行結果) -> bool:
    if rechecked.状態 != "PROPOSE" or rechecked.回答ラベル is None or rechecked.回答内容 is None:
        return False
    if initial.状態 != "PROPOSE" or initial.回答ラベル is None or initial.回答内容 is None:
        return True
    return _提案強度(rechecked) > _提案強度(initial)


def HDS能力経路V2候補提案実行(
    question_ir: HDSIR,
    references: tuple[参照記録, ...],
    *,
    コンパイル,
    基礎能力核: K3相当能力核 | None,
    候補意味IR: Mapping[str, HDSIR] | None = None,
    最大コンパイル並列: int = 4,
    模型核: MINIDORA模型核 | None = None,
    最大局所Window数: int = 12,
) -> HDS選択実行結果:
    'Rで閉じた資料をCが全候補評価し、実観測view変化時だけ再評価する。\n\n    C_execはPROPOSEまで。COMMIT/SUSPEND権限は持たない。\n    '

    initial = HDS候補提案実行(
        question_ir,
        references,
        コンパイル=コンパイル,
        基礎能力核=基礎能力核,
        候補意味IR=候補意味IR,
        最大コンパイル並列=最大コンパイル並列,
        模型核=HDS能力模型核V2(模型核),
    )
    initial = replace(
        initial,
        理由=tuple(dict.fromkeys(tuple(initial.理由) + (
            'C_SAME_資料_候補_NARROWING_DISABLED',
        ))),
    )

    local_references, changed = HDS局所観測view(
        question_ir,
        references,
        上限=最大局所Window数,
    )
    if changed <= 0:
        return initial

    rechecked = HDS候補提案実行(
        question_ir,
        local_references,
        コンパイル=コンパイル,
        基礎能力核=基礎能力核,
        候補意味IR=候補意味IR,
        最大コンパイル並列=最大コンパイル並列,
        模型核=HDS能力模型核V2(模型核),
    )
    common_reasons = (
        "C_OBSERVATION_VIEW_CHANGED",
        f"C_LOCAL_VIEW_SOURCE_COUNT:{changed}",
    )
    if _局所再評価採用可能(initial, rechecked):
        return replace(
            rechecked,
            理由=tuple(dict.fromkeys(tuple(rechecked.理由) + common_reasons + (
                "C_LOCAL_VIEW_RECHECK_SELECTED",
            ))),
            局所Window数=changed,
            局所再照合数=1,
        )
    return replace(
        initial,
        理由=tuple(dict.fromkeys(tuple(initial.理由) + common_reasons + (
            "C_LOCAL_VIEW_RECHECK_REJECTED",
        ))),
        局所Window数=changed,
        局所再照合数=1,
    )


__all__ = [
    "HDS候補被覆優先統合",
    "HDS参照検索V2",
    "HDS能力模型核V2",
    "HDS局所観測view",
    "HDS能力経路V2候補提案実行",
]

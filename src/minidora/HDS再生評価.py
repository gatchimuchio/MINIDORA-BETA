from __future__ import annotations

from collections import Counter
from collections.abc import Iterable, Mapping
from dataclasses import dataclass
from typing import Any

from .HDS資料K import HDSIR知識適合器
from .HDS再生 import HDSIR復元
from .K3機能 import K3相当能力核
from .K3_HDSネイティブ import HDSIRネイティブ適合器


@dataclass(frozen=True, slots=True)
class 再生Case結果:
    識別子: str
    状態: str
    予測: str | None
    gold: str | None
    正解: bool | None
    理由: tuple[str, ...]
    努力: str
    関係図深さ上限: int
    証拠上限: int
    根拠事実数: int
    K追加事実数: int
    K証拠事実数: int
    候補診断: tuple[dict[str, Any], ...]


def HDS再生Case評価(
    row: Mapping[str, Any],
    *,
    計算量: str | None = None,
    基礎能力核: K3相当能力核 | None = None,
) -> 再生Case結果:
    '1件の固定HDS 再生 rowをgold非入力で評価する。'
    payload = dict(row)
    gold_value = payload.pop("gold", None)
    gold = str(gold_value) if gold_value is not None else None
    question = HDSIR復元(payload["question_ir"])
    choices = {
        str(label): HDSIR復元(資料)
        for label, 資料 in dict(payload.get("choices_ir", {})).items()
    }

    模型核 = (基礎能力核 or K3相当能力核()).clone()
    ingest = HDSIR知識適合器(模型核)
    added = 0
    証拠 = 0
    for item in payload.get('資料', ()):
        ir = HDSIR復元(item["ir"])
        provenance = tuple(str(x) for x in item.get("provenance", ()))
        情報源_信頼度 = float(item.get('情報源_信頼度', 1.0))
        結果 = ingest.投入(
            ir,
            provenance=provenance,
            信頼係数=情報源_信頼度,
        )
        added += 結果.追加事実数
        証拠 += 結果.証拠事実数

    結果 = HDSIRネイティブ適合器(模型核).実行(question, 候補IR=choices, 努力=計算量)
    diagnostics = tuple(
        {
            "候補": item.候補,
            "合計得点": item.合計得点,
            "証拠得点": item.証拠得点,
            '関係図得点': item.関係図得点,
            '関係図補正': item.関係図補正係数,
            "独立出典数": item.独立出典数,
            "採用証拠数": item.採用証拠数,
            '関係図深さ': item.関係図深さ,
            "根拠事実数": item.根拠事実数,
        }
        for item in 結果.候補診断
    )
    predicted = 結果.回答ラベル
    return 再生Case結果(
        識別子=str(payload.get("id", "")),
        状態=結果.状態,
        予測=predicted,
        gold=gold,
        正解=(predicted == gold) if gold is not None else None,
        理由=tuple(結果.理由),
        努力=結果.努力水準,
        関係図深さ上限=結果.探索深さ上限,
        証拠上限=結果.証拠上限,
        根拠事実数=結果.根拠事実数,
        K追加事実数=added,
        K証拠事実数=証拠,
        候補診断=diagnostics,
    )


def HDS再生評価(
    rows: Iterable[Mapping[str, Any]],
    *,
    計算量: str | None = None,
    基礎能力核: K3相当能力核 | None = None,
) -> dict[str, Any]:
    details: list[再生Case結果] = []
    reason_counts: Counter[str] = Counter()
    計算量_counts: Counter[str] = Counter()
    correct = 0
    with_gold = 0
    answered = 0

    for row in rows:
        detail = HDS再生Case評価(row, 計算量=計算量, 基礎能力核=基礎能力核)
        details.append(detail)
        reason_counts.update(detail.理由)
        計算量_counts[detail.努力] += 1
        if detail.状態 == "APPROVE" and detail.予測 is not None:
            answered += 1
        if detail.gold is not None:
            with_gold += 1
            if detail.正解:
                correct += 1

    total = len(details)
    return {
        "契約形式": 'minidora.hds-選択肢-再生.結果.v2',
        "total": total,
        "with_gold": with_gold,
        "correct": correct if with_gold else None,
        "accuracy_percent": (100.0 * correct / with_gold) if with_gold else None,
        "answered": answered,
        "suspended": total - answered,
        "answer_rate_percent": (100.0 * answered / total) if total else 0.0,
        '計算量_override': 計算量,
        '計算量_counts': dict(sorted(計算量_counts.items())),
        "reason_counts": dict(sorted(reason_counts.items())),
        "details": [
            {
                "id": item.識別子,
                "status": item.状態,
                "predicted": item.予測,
                "gold": item.gold,
                "correct": item.正解,
                "reasons": list(item.理由),
                '計算量': item.努力,
                '関係図_depth_limit': item.関係図深さ上限,
                '証拠_limit': item.証拠上限,
                "proof_fact_count": item.根拠事実数,
                "k_facts_added": item.K追加事実数,
                '証拠_facts': item.K証拠事実数,
                '候補_diagnostics': list(item.候補診断),
            }
            for item in details
        ],
    }


__all__ = ['再生Case結果', 'HDS再生Case評価', 'HDS再生評価']

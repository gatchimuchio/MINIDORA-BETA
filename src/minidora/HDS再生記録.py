from __future__ import annotations

from collections.abc import Iterable, Mapping
from dataclasses import dataclass
import json
from pathlib import Path
from typing import Any

from .HDS適合器 import HDSコンパイラProtocol, HDS独立コンパイル
from .HDS参照 import HDS参照検索
from .HDS再生 import HDSIR辞書化
from .参照 import 参照供給器, 参照記録


@dataclass(frozen=True, slots=True)
class 再生入力問題:
    識別子: str
    問題文: str
    選択肢: Mapping[str, str]
    gold: str | None = None


@dataclass(frozen=True, slots=True)
class 再生収録統計:
    問題数: int
    選択肢コンパイル数: int
    資料件数: int
    資料コンパイル数: int
    資料コンパイル失敗数: int


def _問題IR(構文化器: HDSコンパイラProtocol, problem: 再生入力問題):
    '利用可能なら通常選択肢 実行系と同じ問題IR入口を使う。'
    builder = getattr(構文化器, "問題IR", None)
    if not callable(builder):
        return 構文化器.コンパイル(problem.問題文)

    ordered = tuple(sorted((str(label), str(text)) for label, text in problem.選択肢.items()))
    labels = tuple(label for label, _ in ordered)
    question_ir = builder(problem.問題文, tuple(text for _, text in ordered))
    generated = tuple(sorted(
        coord.座標ID.split(":", 1)[1]
        for coord in question_ir.座標
        if coord.座標ID.startswith('選択肢:')
    ))
    if generated != labels:
        raise ValueError(
            '再生入力の選択肢labelと構文化器問題IRの選択肢 labelが一致しません: '
            f"input={labels}, generated={generated}"
        )
    return question_ir


def _参照provenance(record: 参照記録) -> list[str]:
    '通常選択肢 実行系と同じ検索経路provenanceを再生へ固定する。'
    markers: list[str] = []
    for key, value in record.条件:
        k = str(key)
        if k == 'hds_query_選択肢':
            markers.append('query_選択肢:' + str(value))
        elif k == "hds_query_kind":
            markers.append("query_kind:" + str(value))
    return [record.供給器, record.由来, record.識別子, *dict.fromkeys(markers)]


def HDS選択肢再生収録(
    問題群: Iterable[再生入力問題],
    *,
    構文化器: HDSコンパイラProtocol,
    provider: 参照供給器 | None,
) -> tuple[tuple[dict[str, Any], ...], 再生収録統計]:
    '構文化器/Rを一度だけ使い、実行系比較用HDS-IR bundleを作る。\n\n    goldはIR生成・検索・資料コンパイルへ渡さず、最終行へ採点情報として付与するだけ。\n    外部資料は生文字列のまま保存せず、HDS-IR・情報源 信頼度・検索経路provenanceを保存する。\n    構文化器が選択肢問題専用入口を持つ場合は通常実行系と同じ問題IRを固定する。\n    '
    rows: list[dict[str, Any]] = []
    選択肢_compiled = 0
    資料_count = 0
    資料_compiled = 0
    資料_failed = 0

    for problem in 問題群:
        kernel_builder = getattr(構文化器, "問題コンパイル束", None)
        if callable(kernel_builder):
            ordered = tuple(sorted((str(label), str(text)) for label, text in problem.選択肢.items()))
            kernel = kernel_builder(problem.問題文, tuple(text for _, text in ordered))
            question_ir = kernel.意味IR
            observation_requests = tuple(kernel.参照観測要求)
        else:
            question_ir = _問題IR(構文化器, problem)
            observation_requests = None
        選択肢_irs: dict[str, Any] = {}
        for label, text in sorted(problem.選択肢.items(), key=lambda item: str(item[0])):
            選択中間表現 = HDS独立コンパイル(構文化器, str(text))
            選択肢_irs[str(label)] = HDSIR辞書化(選択中間表現)
            選択肢_compiled += 1

        references = (
            HDS参照検索(provider, question_ir, 観測要求=observation_requests)
            if provider is not None else ()
        )
        資料_rows: list[dict[str, Any]] = []
        資料_count += len(references)
        for record in references:
            try:
                資料_ir = HDS独立コンパイル(構文化器, record.内容)
            except Exception:
                資料_failed += 1
                continue
            資料_compiled += 1
            資料_rows.append(
                {
                    "provenance": _参照provenance(record),
                    '情報源_信頼度': float(record.信頼),
                    "ir": HDSIR辞書化(資料_ir),
                }
            )

        row: dict[str, Any] = {
            "契約形式": 'minidora.hds-選択肢-再生.v1',
            "id": problem.識別子,
            "question_ir": HDSIR辞書化(question_ir),
            "choices_ir": 選択肢_irs,
            '資料': 資料_rows,
        }
        if problem.gold is not None:
            row["gold"] = str(problem.gold)
        rows.append(row)

    return (
        tuple(rows),
        再生収録統計(
            問題数=len(rows),
            選択肢コンパイル数=選択肢_compiled,
            資料件数=資料_count,
            資料コンパイル数=資料_compiled,
            資料コンパイル失敗数=資料_failed,
        ),
    )


def 再生JSONL保存(rows: Iterable[Mapping[str, Any]], path: str | Path) -> None:
    target = Path(path)
    target.parent.mkdir(parents=True, exist_ok=True)
    with target.open("w", encoding="utf-8", newline="\n") as handle:
        for row in rows:
            handle.write(json.dumps(dict(row), ensure_ascii=False, separators=(",", ":")) + "\n")


__all__ = [
    '再生入力問題',
    '再生収録統計',
    'HDS選択肢再生収録',
    '再生JSONL保存',
]

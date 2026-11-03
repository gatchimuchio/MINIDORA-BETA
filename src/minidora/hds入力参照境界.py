from __future__ import annotations

from collections.abc import Callable, Sequence
from dataclasses import dataclass

from .HDS中間表現 import HDSIR
from .参照 import 参照記録


@dataclass(frozen=True, slots=True)
class HDS入力資料束:
    'HDS 構文化器で構文化した資料をMINIDORA入力へ整列する前段境界。'

    IR群: tuple[HDSIR, ...]
    出典ID群: tuple[str, ...]
    信頼群: tuple[float, ...]
    成功記録群: tuple[参照記録, ...]
    失敗数: int


def HDS入力出典ID(record: 参照記録) -> str:
    identifier = str(record.識別子).strip()
    if identifier:
        return identifier
    provider = str(record.供給器).strip()
    origin = str(record.由来).strip()
    代替経路 = ":".join(part for part in (provider, origin) if part)
    return 代替経路 or 'anonymous-参照'


def HDS入力資料本文(record: 参照記録) -> str:
    '型付き資料は対象・意味キー・値を保持し、未構造資料は原文をそのまま渡す。'
    if record.意味確定 and record.意味キー is not None:
        target = str(record.対象).strip() or "対象"
        key = str(record.意味キー).strip()
        return f"{target}。{key} は {record.表示値}。"
    return str(record.内容)


def HDS入力資料整列(
    references: Sequence[参照記録],
    payloads: Sequence[HDSIR | Exception],
    射影: Callable[[HDSIR], HDSIR],
) -> HDS入力資料束:
    '成功した資料だけをMINIDORA入力用に同一添字で保持する。\n\n    ここは前段入力境界であり、後段HDS判断主体の入力ではない。\n    '
    if len(references) != len(payloads):
        raise ValueError('参照記録と資料コンパイル結果は同数である必要がある')

    irs: list[HDSIR] = []
    ids: list[str] = []
    trusts: list[float] = []
    records: list[参照記録] = []
    failed = 0
    for record, payload in zip(references, payloads):
        if isinstance(payload, Exception):
            failed += 1
            continue
        irs.append(射影(payload))
        ids.append(HDS入力出典ID(record))
        trusts.append(float(record.信頼))
        records.append(record)
    return HDS入力資料束(tuple(irs), tuple(ids), tuple(trusts), tuple(records), failed)


__all__ = ['HDS入力資料束', "HDS入力出典ID", 'HDS入力資料本文', 'HDS入力資料整列']

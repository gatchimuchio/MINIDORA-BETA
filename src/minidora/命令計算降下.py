from __future__ import annotations

from .命令 import 手順, 作用
from .計算中間表現 import 計算中間表現, 計算作用, 計算値, 計算命令


_作用対応 = {item.value: 計算作用(item.value) for item in 作用}


def _値降下(value):
    if isinstance(value, str) and value.startswith("$"):
        address = value[1:]
        if not address:
            raise ValueError("空の状態値参照は計算中間表現へ降下できない")
        return 計算値.状態値(address)
    return 計算値.即値(value)


def 命令計算降下(手順_: 手順) -> 計算中間表現:
    """日本語命令形Pを計算専用中間表現へ降下する。

    Pの意味語彙を実行器へ持ち込まず、既に確定した作用・値参照・住所だけを型付きで渡す。
    """

    instructions: list[計算命令] = []
    for index, instruction in enumerate(手順_.命令列, start=1):
        operation = _作用対応[instruction.作用.value]
        if instruction.作用 == 作用.交換:
            operands = tuple(
                計算値.状態住所(value) if isinstance(value, str) else 計算値.即値(value)
                for value in instruction.引数
            )
        else:
            operands = tuple(_値降下(value) for value in instruction.引数)

        target = instruction.対象 if instruction.作用 == 作用.取得 else None
        instructions.append(
            計算命令(
                命令ID=f"{手順_.名称}:{index}",
                名称=instruction.名称,
                作用=operation,
                入力=operands,
                対象住所=target,
                出力住所=instruction.更新先,
                根拠=tuple(instruction.根拠),
            )
        )

    return 計算中間表現(
        名称=手順_.名称,
        命令列=tuple(instructions),
        出力住所="結果",
        由来=手順_.由来 or "日本語命令形P",
        由来参照=(手順_.名称,),
    )


__all__ = ["命令計算降下"]

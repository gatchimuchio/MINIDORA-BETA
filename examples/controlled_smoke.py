"""Controlled beta wiring example for English-speaking reviewers.

Run from the repository root:
    python -X utf8 examples/controlled_smoke.py

This intentionally uses a supplied action and Japanese sample input.
It is NOT an autonomous reasoning or English-language benchmark.
"""

from minidora.engine_beta import ミニドラβ実行機 as MiniDoraBeta
from minidora.HDS実行主体 import (
    HDS関数作用 as FunctionAction,
    HDS作用結果 as ActionResult,
    HDS作用状態 as ActionState,
)


def main() -> None:
    action = FunctionAction(
        "demo-action",
        lambda state: ActionResult(
            ActionState.成立,
            追加状態=frozenset({"done"}),
            成果=(("result", "completed"),),
        ),
        出力状態=("done",),
    )
    engine = MiniDoraBeta()
    result = engine.実行(
        "処理を実行",
        案件ID="case-001",
        入力ID="request-001",
        要求状態=("done",),
        追加作用=(action,),
        成果名="result",
    )
    print("Execution terminal:", result.駆動.終端.value)
    print("Rendered output:", result.表現.本文)
    print("Implicit external delivery: none")

    # To retrieve the same completed request, do not call .実行 again.
    reused = engine.結果を参照(案件ID="case-001", 入力ID="request-001")
    assert reused is result


if __name__ == "__main__":
    main()

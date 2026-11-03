# MINIDORA-BETA — Public API and Japanese-to-English Glossary

This is a **practical interface map** for non-Japanese-speaking readers. It documents **public observable code contracts only**; the English labels are explanatory, not formal replacements for the Japanese implementation, nor a reconstruction of private HDS theory.

## Imports

```python
from minidora.engine_beta import ミニドラβ実行機 as MiniDoraBeta
from minidora.HDS実行主体 import (
    HDS関数作用 as FunctionAction,
    HDS作用結果 as ActionResult,
    HDS作用状態 as ActionState,
)
```

The English names after `as` are ordinary local Python aliases. The **package actually exports the Japanese names**. There is no added `.run()` method.

## Public execution entry point

The beta facade is `minidora.engine_beta.ミニドラβ実行機` (English description: *MINIDORA beta executor*).

| Exact name (do not translate in code) | English explanation | Behavior |
|---|---|---|
| `ミニドラβ実行機()` | beta executor constructor | Creates or accepts the three existing machines |
| `入力機` | input machine | Optional constructor-injected `HDS入力系` |
| `本体` | engine core | Optional constructor-injected `HDS駆動コア` |
| `出力機` | output machine | Optional constructor-injected `HDS出力系` |
| `実行(...)` | execute one input version | Input → core execution → output composition/rendering; **no implicit external delivery** |
| `結果を参照(...)` | retrieve a stored result | Looks up a completed result from the same executor instance |

### `実行` arguments

| Exact keyword / parameter | English meaning | Default / constraint |
|---|---|---|
| `本文` | source text (first positional argument) | Required; semantics/language quality not universally guaranteed |
| `案件ID` | case ID | Required string |
| `入力ID` | input ID | Required string |
| `要求状態` | required states | `()` |
| `追加作用` | supplied actions | `()` |
| `成果名` | desired result name | `None` |
| `入力版` | input version | `1` |
| `出力ID` | output ID | `"回答"` |
| `判断版` | decision version | `1` |
| `表現` | rendering contract | `None` |
| `**駆動条件` | additional core-execution options | Passed to the existing core; do not invent unsupported options |

### Returned `β処理結果` fields

| Exact field | English explanation |
|---|---|
| `入力` | accepted input bundle |
| `駆動` | core execution result |
| `出力` | composed output bundle |
| `表現` | rendered output result |
| `駆動.終端.value` | terminal status value (for example `COMMIT` in the controlled sample) |
| `表現.本文` | rendered body text |

### Input versioning and failure cases

The deduplication key is `(案件ID, 入力ID, 入力版)` (*case ID, input ID, input version*).

- Repeating `実行` with the **same key on the same executor instance** raises `ValueError` **before input acceptance**.
- `結果を参照(案件ID=..., 入力ID=..., 入力版=1)` returns a stored completed result or raises `KeyError` if no completed result was stored.
- The engine marks a key before core execution. If execution fails partway through, the same instance will **not** silently execute that key again; do not assume the result lookup then succeeds.
- If you genuinely need a new attempt, explicitly advance `入力版` and manage new evidence/intent; never conceal a failure as an automatic retry.
- This is **not durable cross-process idempotency**. Executor instance memory does not survive a restart.
- External delivery is a separate explicit output-machine operation and is not called by this facade.

## Public machine terminology

| Japanese executable name | English explanatory label | Responsibility boundary |
|---|---|---|
| `HDS入力系` | Input machine | Accept and seal input; no second learning loop |
| `受理` | Accept | One input-acceptance operation |
| `HDS駆動コア` | MINIDORA execution core | Goal, action, state, learning, stopping, final adoption |
| `入力実行` | Execute accepted input | Consumes accepted input bundle |
| `入力選択実行` | Selection-input path | Existing specialized core path, not consolidated into beta facade |
| `入力関係実行` | Relation-input path | Existing specialized core path, not consolidated into beta facade |
| `HDS出力系` | Output machine | Render and explicitly deliver final output |
| `表現` | Render | Produce a presentation of the accepted output |
| `送達` | Deliver | Explicit external delivery; not automatic |
| `HDS関数作用` | Function action | Supply a controlled action to the existing core |
| `HDS作用結果` | Action result | State and result supplied by that action |
| `HDS作用状態.成立` | Action-state enum: established | Exact enum name in the controlled example |
| `追加状態` | New states | Additional state values emitted by a supplied action |
| `成果` | Results | Pairs such as `("result", "completed")` |
| `出力状態` | Declared output states | Action's declared state effect |

These descriptions do not define complete HDS theory, do not imply that English input is supported at equal quality, and do not change the underlying API. Keep all literals in source and runtime objects intact.

## Complete controlled example

Use [`examples/controlled_smoke.py`](../examples/controlled_smoke.py) or the code block in the [main README](../README.md). It uses explicit Japanese keyword arguments and English local aliases. It is intentionally a **controlled execution wiring test**, not a benchmark.

## API compatibility, evidence and translation

- Existing `minidora` and `minidora-product` command entry points remain.
- The isolated facade currently targets **general text input**; specialized core interfaces are inherited but not all routed through `ミニドラβ実行機.実行`.
- No English Python wrapper or private abstraction mapping has been added.
- For implementation/test provenance: [SOURCE_LOCK.md](../SOURCE_LOCK.md), [BETA_READINESS.md](../BETA_READINESS.md).
- For publication restrictions: [PUBLICATION_BOUNDARY.md](../PUBLICATION_BOUNDARY.md).

**Translation rule:** the English label is a navigation aid. If it conflicts with the executable Japanese API, inspect the code and report the discrepancy rather than inventing an equivalence.

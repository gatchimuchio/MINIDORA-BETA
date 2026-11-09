# MINIDORA-BETA — HDS-Driven Execution Engine

**Engine Beta 1 · `minidora-notnn==0.6.0b1` · English-first documentation**

MINIDORA is a general-purpose **execution engine organized around HDS-driven input, core execution, and output**. The engine is not defined by being a large language model (LLM) or a chatbot. Language generation and conversational applications are possible uses of an engine, not the entire scope of this repository.

This is an independently isolated beta of the public implementation from [NOTNN-LLM-MINIDORA](https://github.com/gatchimuchio/NOTNN-LLM-MINIDORA), pinned to upstream commit [`9307d7e`](https://github.com/gatchimuchio/NOTNN-LLM-MINIDORA/commit/9307d7e2b1799750ecef1957d85128fcbe403d02). **The implementation and public API names remain Japanese by design.** The documentation is provided in English for international reviewers.

> **Scope:** "General-purpose engine" describes the architecture and intended reuse; it is **not** an experimentally established claim of AGI, universal language understanding, or performance parity with any LLM. English documentation does **not** establish English-language input quality.

## Read this first

| If you want to… | Read |
|---|---|
| Explore the **R2 logical successor design, rejected R1 audit and remaining source/evidence gates** | [Whole-system redesign register](docs/redesign/README.md) |
| Understand the engine without reading Japanese | [English onboarding guide](docs/ENGLISH_ONBOARDING.md) |
| See the public interface and Japanese-to-English API glossary | [API and terminology guide](docs/API_GUIDE.md) |
| Check what passed, under what conditions | [Beta acceptance evidence](BETA_READINESS.md) |
| Understand the immutable beta distribution vs. later documentation | [Repository lock](REPOSITORY_LOCK.md) and [Documentation revision](DOCS_REVISION.md) |
| Contribute without altering the baseline | [Contributing](CONTRIBUTING.md) and [Agent instructions](AGENTS.md) |
| Review publication and licensing restrictions | [Publication boundary](PUBLICATION_BOUNDARY.md), [LICENSE](LICENSE), [NOTICE](NOTICE) |

## Future architecture: comprehensive redesign (design only)

The next proposed MINIDORA generation has a **single normative logical architecture specification**, reconsidered from first principles across inherited public capabilities and emerging requirements. **R1 failed independent design review**; R2 addresses its 22 identified contract flaws. A requirement trace and source/file metadata census now accompany R2, but semantic routes, algorithm alternatives and full acceptance are **not yet closed**. The objective is one connected execution system, not a chatbot-only feature backlog or an assumption that the current module tree is final.

- [Unified engine specification R2](docs/redesign/01_SYSTEM_SPECIFICATION.md): three-machine responsibility, **OS/CPU/format-independent abstract execution semantics**, typed records, native non-neural language/relation/learning, logical durable revisions, actual rendering/effect receipts, and scope-limited deterministic replay. **Python, SQLite, JSON, model implementation and numeric resource targets are not logical necessities**.
- [Provisional capability/gap register](docs/redesign/02_CAPABILITY_REGISTER.md), [93-item machine-readable trace](docs/redesign/03_REQUIREMENT_TRACE.tsv) and [414-source file census](docs/redesign/04_SOURCE_CENSUS.tsv): planned owners, exact source identity and explicit unknown actual routes/acceptance.
- [Verification contract](docs/redesign/05_VERIFICATION_AND_EVIDENCE.md) and [historical decision record](docs/redesign/06_LINEAGE_AND_DECISIONS.md): keep observed baseline, old experiments, proposals and current results distinct.
- [R1 audit correction ledger](docs/redesign/07_R1_AUDIT_CLOSURE.md) and [test-file census](docs/redesign/09_TEST_CENSUS.tsv): 22 named findings with correction/evidence separation and 359 upstream/147 beta test path identities.

**This is a documentation-only design candidate**, not a change to the frozen `0.6.0b1` runtime or a claim that every listed ability is implemented or accepted. The upstream repository is used for read-only evidence and is not changed by this work. New executable behavior requires a new, independently evidenced version.

### Broader source capability inventory

The inherited public source contains named implementations/interfaces for language processing, calculation, relations, structured documents, browser/reference acquisition, scientific capabilities, code, writing, conversation, memory, goal execution, trace and replay. **Source presence is not the same as verified availability through a general user request.** The redesign register accounts for those areas and for external speech, image, video, translation, software and tool adapters without attributing provider performance to the beta engine.

## Three machines, three responsibilities

| Machine | Public Python entry point | Owns | Does not own |
|---|---|---|---|
| **Input machine** (boiler analogy) | `HDS入力系.受理` | Accepting source text/context, one-time input interpretation, sealed input bundles | Independent learning or final adoption |
| **MINIDORA core** (turbine/generator) | `HDS駆動コア.入力実行` | Goal/state maintenance, action execution, learning state, final adoption and stopping | External presentation or implicit delivery |
| **Output machine** (transformer/transmission) | `HDS出力系.表現` and explicit delivery operations | Rendering, output validity/versioning, delivery | A second decision/learning loop |

The public beta facade, **`minidora.engine_beta.ミニドラβ実行機`**, connects these existing machines in order. It does not replace them or duplicate their decision cycles. This facade accepts **general text input**; specialized selection/relation APIs continue to exist separately.

## Install and run

Requires Python **3.11+**. The scoped acceptance evidence was gathered on **Linux/Python 3.12**.

```bash
python -m pip install .
python -X utf8 examples/controlled_smoke.py
python tools/beta_acceptance.py
```

Windows PowerShell (when `py` is installed):

```powershell
py -3.12 -m pip install .
py -3.12 -X utf8 examples/controlled_smoke.py
```

Pinned runtime dependencies: `SudachiPy==0.6.11` and `SudachiDict-core==20260723`. Existing `minidora` and `minidora-product` console commands are preserved. Cloud/search/external sending capabilities require their own explicit configuration and authorization.

### Minimal API example

The example below is a **controlled wiring test with a supplied action**, not proof that the engine autonomously solves the request or understands arbitrary English.

```python
from minidora.engine_beta import ミニドラβ実行機 as MiniDoraBeta
from minidora.HDS実行主体 import (
    HDS関数作用 as FunctionAction,
    HDS作用結果 as ActionResult,
    HDS作用状態 as ActionState,
)

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
    "処理を実行",  # Japanese sample text; English input quality is not asserted.
    案件ID="case-001",
    入力ID="request-001",
    要求状態=("done",),
    追加作用=(action,),
    成果名="result",
)
print(result.駆動.終端.value)
print(result.表現.本文)
```

### Duplicate-input and delivery contract

The logical input key is `(案件ID, 入力ID, 入力版)` — case ID, input ID, input version. A beta facade instance rejects a second execution with the same key; retrieve a completed result through `結果を参照`, or explicitly increment `入力版` for a new attempt. This guarantee is **in-memory within one instance**; it does not survive process restarts. `実行()` does **not** send output to an external system.

## Preservation and verification

- **414/414 upstream public source paths preserved.** At historical beta extraction: 413 byte-identical, one modified engine source, plus two beta entry points. In the **current GitHub English-documentation tree**, `src/README.md` also differs from the upstream blob: **412 byte-identical + one modified engine file + one translated documentation file + two beta entry points**. These are distinct snapshot comparisons, not contradictory engine-change counts.
- **313 selected regression tests passed** (Linux/Python 3.12, 2026-11-03). This is not a claim that all upstream tests passed; 147 test files were included in the beta bundle.
- The original wheel build, isolated installation and controlled input → core → output smoke test passed.
- The immutable original beta ZIP and its release manifest remain historical distribution evidence. This repository's **English documentation revision changes documentation bytes**, not the underlying engine source. Rebuilding a wheel from the revised checkout can therefore yield a **different distribution hash** from the original ZIP's wheel.

Read [SOURCE_LOCK.md](SOURCE_LOCK.md), [BETA_READINESS.md](BETA_READINESS.md), [RELEASE_MANIFEST.json](RELEASE_MANIFEST.json), and [DOCS_REVISION.md](DOCS_REVISION.md) before making byte-for-byte claims.

## What this beta does not establish

- Universal general reasoning, arbitrary-language performance, AGI, or LLM-equivalent output quality.
- A full run of the upstream public test suite or fresh GPQA/other benchmark scores.
- Cross-platform/cloud acceptance or production availability.
- Cross-process durable de-duplication, persistence, or implicit external delivery.

Historic performance records are about their **original model revision and evaluation conditions**. They are not new measurements of `0.6.0b1`.

## Language, authorship, licensing

Japanese is retained for the implementation, semantic identifiers and historical file paths. **English is the explanatory/documentation language of this repository.** See [API and terminology guide](docs/API_GUIDE.md) for readable aliases and boundaries; these are explanatory translations, not replacements for the original code contracts.

Original author: **がっちむち♂**. Software and explanatory documentation have different licenses; see [LICENSE](LICENSE), [NOTICE](NOTICE), and [CONTRIBUTING.md](CONTRIBUTING.md).

Upstream: https://github.com/gatchimuchio/NOTNN-LLM-MINIDORA

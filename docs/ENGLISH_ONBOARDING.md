# Getting Started with MINIDORA-BETA (English)

This guide is for readers who do **not** read Japanese Python identifiers. It explains the minimum needed to inspect and test MINIDORA without rewriting or guessing at its internals.

## What are you installing?

An isolated beta snapshot of an HDS-driven engine. Its public execution boundary consists of three existing components:

1. **Input machine:** accepts an input once and produces a sealed bundle.
2. **MINIDORA core:** runs supplied or existing actions against goals and state, and owns stopping, learning state, and final acceptance.
3. **Output machine:** presents the core result and handles explicit output/delivery operations.

Think of a power station: the input machine is the **boiler**, the MINIDORA core is the **turbine and generator**, and the output machine is the **transformer/transmission interface**. This analogy explains responsibility boundaries; it is **not** a technical claim about energy generation.

The engine is **not defined as a chatbot or LLM**. The existence of generic execution wiring does not, by itself, demonstrate general intelligence or arbitrary natural-language understanding.

## Installation

Use Python **3.12** when reproducing the recorded Linux/Python 3.12 acceptance environment (the declared minimum is Python 3.11).

```bash
git clone https://github.com/gatchimuchio/MINIDORA-BETA.git
cd MINIDORA-BETA
python -m pip install .
python -X utf8 examples/controlled_smoke.py
```

Windows PowerShell, if the Python launcher is installed:

```powershell
git clone https://github.com/gatchimuchio/MINIDORA-BETA.git
cd MINIDORA-BETA
py -3.12 -m pip install .
py -3.12 -X utf8 examples/controlled_smoke.py
```

The command examples use regular Git tooling for **third-party readers**. They do not authorize maintenance agents to rewrite or push to the upstream repository.

Python downloads the pinned public dependencies `SudachiPy==0.6.11` and `SudachiDict-core==20260723` when not already installed. The sample uses UTF-8 source encoding and requires a terminal/font that can display Japanese text, though understanding that text is unnecessary for running the test.

If your terminal displays the symbols as boxes, inspect the returned status or use a UTF-8-capable terminal. Do **not** rename the Japanese Python files as a workaround; they are importable Unicode identifiers.

## What the controlled sample actually proves

The sample at [`examples/controlled_smoke.py`](../examples/controlled_smoke.py):

- imports the public beta facade through an **English Python alias** (`MiniDoraBeta`);
- supplies a deterministic demonstration action;
- passes a Japanese demonstration text and an explicit desired state;
- executes the core once;
- displays terminal and rendered-output fields;
- retrieves the stored result without running the same input version twice.

It does **not** establish autonomous problem solving, text comprehension quality, external delivery, cross-process persistence, or English-language support.

## Run the scoped acceptance suite

```bash
python tools/beta_acceptance.py
```

The recorded beta snapshot passed **313 selected regression cases** on Linux/Python 3.12 (2026-11-03). Your result depends on your own environment and is new evidence only for what you ran. The full upstream public test suite was **not** included in this acceptance decision. A manually triggered multi-OS workflow is available but has **not** been reported as run.

## Reading the code without Japanese

You can import Japanese-named Python classes under descriptive English aliases:

```python
from minidora.engine_beta import ミニドラβ実行機 as MiniDoraBeta
```

**Do not translate method names or keyword arguments when calling them.** `engine.実行(...)` is the actual API; `engine.run(...)` is not supplied by this beta facade. Use the [API glossary](API_GUIDE.md) to match each label.

The README's code sample is sufficient for a basic controlled wiring check. For deeper maintenance, read [AGENTS.md](../AGENTS.md) and inspect the source; this guide does not reverse-engineer private HDS theory.

## Important reviewer cautions

- **Version:** engine package `0.6.0b1`; English documentation is a separate revision.
- **Language:** explanatory prose is English; Python identifiers and the source's semantic baseline are Japanese.
- **Safety/control:** supplied action completion is not a guarantee of correct reasoning.
- **History:** old GPQA/Cloud Run claims belong to older upstream checkpoints; they are not new beta results.
- **Integrity:** original ZIP/wheel and localized repository checkouts have different documented hash scopes.

See [Beta evidence](../BETA_READINESS.md), [Source lock](../SOURCE_LOCK.md), [Documentation revision](../DOCS_REVISION.md), [Licensing](../LICENSE), and [Publication boundary](../PUBLICATION_BOUNDARY.md).

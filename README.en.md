# MINIDORA Engine Beta 1

MINIDORA is a general-purpose **HDS-driven execution engine**. A language model or chatbot is an application of the engine, not its definition.

This repository is an isolated, source-installable beta derived from [NOTNN-LLM-MINIDORA](https://github.com/gatchimuchio/NOTNN-LLM-MINIDORA), fixed at upstream commit `9307d7e2b1799750ecef1957d85128fcbe403d02`.

## Architecture

| Component | Existing entry point | Responsibility |
|---|---|---|
| Input machine | `HDS入力系` | Input acquisition, one-time interpretation and sealed input bundles |
| MINIDORA core | `HDS駆動コア` | Goal-directed execution, state transitions, learning and final adoption |
| Output machine | `HDS出力系` | Rendering, invalidation, versioned delivery |
| Beta facade | `minidora.engine_beta.ミニドラβ実行機` | Connect the existing machines without duplicating their decisions |

The beta facade targets general text input. The original specialized selection and relation execution APIs remain present.

## Installation

Requires Python 3.11 or newer.

```bash
python -m pip install .
python -c "from minidora.engine_beta import ミニドラβ実行機; print(ミニドラβ実行機)"
```

Runtime dependencies are pinned to SudachiPy 0.6.11 and SudachiDict-core 20260723. Existing `minidora` and `minidora-product` commands remain available.

## Status and evidence

- Beta version: `0.6.0b1`
- All 414 upstream public source files are present: 413 byte-identical Git blobs, 1 deliberate modification, 2 additional files.
- 313 selected regression tests passed on Linux/Python 3.12.
- An independently installed wheel passed a controlled input → drive → output smoke test.
- This does **not** prove universal language/model ability or full upstream test-suite conformance.
- No automatic external delivery is performed by the beta facade.

See [README.md](README.md), [BETA_READINESS.md](BETA_READINESS.md), [SOURCE_LOCK.md](SOURCE_LOCK.md), [REPOSITORY_LOCK.md](REPOSITORY_LOCK.md), and [PUBLICATION_BOUNDARY.md](PUBLICATION_BOUNDARY.md).

Original author: がっちむち♂. Licensing is split by artifact type; see LICENSE and NOTICE.

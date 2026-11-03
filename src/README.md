# MINIDORA Source Implementation

`src/minidora/` contains the executable MINIDORA implementation. **Python filenames, symbols, semantic identifiers and protocol values remain Japanese.** This is intentional; public English documentation is an interface to the implementation, not a translation of its underlying source language.

## Public-scope boundary

This document describes file placement and established compatible entry points only. It does not publish private abstract definitions, private upper-level references or a complete private-theory-to-implementation mapping.

Historically, MINIDORA uses **Japanese as the canonical internal semantic language**, with foreign-language terms retained at required external compatibility boundaries. Former upstream language-contract files such as `設計/00_日本語基底規定_v1.md` and `設計/01_日本語正本語彙_v1.md` are **not bundled** as full theory documents here.

## Integrated execution

The public path maintains state ownership, action activation, result feedback, verification and termination in an integrated execution route. Do not infer a complete hidden theoretical decomposition from this short public overview.

Representative established entry points:

- `src/minidora/HDS実行主体.py`
- `src/minidora/HDS駆動コア.py`
- `src/minidora/統合駆動_v2/`
- Isolated beta facade: `src/minidora/engine_beta.py`, delegating to `src/minidora/β実行.py`

Public compatibility stub: `設計/60_MINIDORA_HDS自律接続_v3.md`.

## Capability and module layout

The public tree contains replaceable elements related to language-model machinery, capabilities, computation, documents, math, code and external references. Passing a local component test is not proof of final system-wide success.

Historical English/legacy identifiers can be kept for API compatibility. An identifier alone does not define private theory.

## Where to start in English

- [Engine overview](../README.md)
- [English onboarding](../docs/ENGLISH_ONBOARDING.md)
- [Beta facade API guide](../docs/API_GUIDE.md)
- [Scoped acceptance](../BETA_READINESS.md)
- [Publication restrictions](../PUBLICATION_BOUNDARY.md)

A separate published treatment of constitutive language-model requirements exists at `gatchimuchio/LLM-Constitutive-Specification`. Its relationship to MINIDORA does not redefine the MINIDORA execution engine as only an LLM.

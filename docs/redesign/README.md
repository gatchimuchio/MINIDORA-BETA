# MINIDORA — Whole-System Redesign R2 Register

**Status:** R2 logical design corrected after R1 rejection; **source-route and implementation acceptance pending**, 2026-11-09 (JST)  
**Target:** The next independently versioned MINIDORA implementation, developed in `MINIDORA-BETA`  
**Starting evidence:** Frozen engine `minidora-notnn==0.6.0b1` and the read-only upstream public repository  
**Change class:** Documentation only. No engine code, test, benchmark, distribution, deployment or upstream repository was modified by creating this register.

## Decision and scope

The **R1 architecture-completion claim was rejected** by a 22-finding audit. R2 now defines one versioned **OS/CPU/representation-independent logical machine** and repairs the known goal/receipt, state-transition, language, persistence, identity and decision conflicts in the **sole normative specification**. This is not a claim that all current source routes have been semantically inspected or that every proposed capability is implementable under a chosen runtime. Concrete programming language/compiler, model transition algorithm, storage adapter, serialization and numeric operating budgets require comparison against actual requirements and inherited behavior.

The redesign is a *candidate* for a future version. The historical beta distribution, its tests, source lock, integrity records and earlier evaluations remain separate and recoverable. A documented capability is not automatically an implemented, connected, accepted or independently demonstrated capability.

## Normative specification and supporting annexes

| File | Purpose | Owner of decision |
|---|---|---|
| [01 — Unified engine specification](01_SYSTEM_SPECIFICATION.md) | **Sole normative R2 logical design:** core/port/state/effect semantics, guarantee bounds and acceptance conditions | Architecture |
| [02 — Capability register](02_CAPABILITY_REGISTER.md) | **Provisional** 93 named inherited, historical and emerging obligations; status and test obligation | Coverage annex |
| [03 — 93-item machine-readable requirement trace](03_REQUIREMENT_TRACE.tsv) | One row per current obligation, planned owner, actual route unknown, separate implementation/acceptance status | Coverage annex |
| [04 — Source blob census](04_SOURCE_CENSUS.tsv) | All 414 fixed-upstream source paths plus 2 beta-specific paths, SHA comparison and honest semantic-review gaps | Source identity annex |
| [05 — Verification and evidence](05_VERIFICATION_AND_EVIDENCE.md) | E2E, learning, reference, regression, quality and performance contracts | Evidence annex |
| [06 — Lineage and decisions](06_LINEAGE_AND_DECISIONS.md) | Dated reference scope, historical evidence, rejected R1 decisions and outstanding observations | Lineage annex |
| [07 — R1 audit correction ledger](07_R1_AUDIT_CLOSURE.md) | Trace all 22 findings to R2 clauses, distinguish fixed text from unmet empirical/source gates | Audit annex |
| [08 — Wire-schema projection](08_WIRE_SCHEMA.json) | JSON Schema 2020-12 example representation of principal records, without claiming runtime or semantic equivalence | Contract-format annex |
| [09 — Upstream/beta test census](09_TEST_CENSUS.tsv) | All 359 upstream test paths, 147 beta test paths, source identities and coverage-unreviewed status | Test-population annex |

The former construction-plan and agent-work-order documents were retired from the current design in favor of direct architectural decisions in `01`. Their earlier content remains recoverable in Git history, not current design authority.

## Authority and precedence

1. User's active instruction, publication boundary and applicable repository instructions.
2. Historical baseline facts: [`SOURCE_LOCK.md`](../../SOURCE_LOCK.md), [`REPOSITORY_LOCK.md`](../../REPOSITORY_LOCK.md), [`BETA_READINESS.md`](../../BETA_READINESS.md), [`RELEASE_MANIFEST.json`](../../RELEASE_MANIFEST.json) and dated source/evaluation records.
3. **This design candidate** for a future version; it does not alter the frozen `0.6.0b1` contract or retroactively certify old performance.
4. Implementation choices must be justified by contract coverage, boundary preservation, measured effect and comparative alternatives. Existing code structure is reusable evidence, not a mandatory architectural endpoint.

Public documents describe **observable contracts**, not private HDS theory. Do not copy unpublished definitions, complete theory-to-source mappings or semantic teaching datasets into the repository. See [`PUBLICATION_BOUNDARY.md`](../../PUBLICATION_BOUNDARY.md).

## Initial observation limits

- Reviewed: current beta and fixed-upstream **entire public source/test metadata trees** (SHA/path counts), beta entry points and maintenance/lock/acceptance contracts; relevant upstream design/evaluation/canonical material; the full R1 specification and three annexes; 22 findings.
- **Precise source distinction:** At beta extraction, the historical source lock reports 413 identical and 1 modified. **Current GitHub** comparison shows 412 identical, **1 modified engine source + 1 later-translated `src/README.md`**, and 2 beta additions. This is not an implementation regression.
- Not completed: semantic/code-level inspection of every inherited path, source→capability→public route equivalence, applicability/execution of all 359 upstream test paths, executable successor, model comparison, or fresh general-chat/GPQA/Windows/multi-OS/Cloud acceptance.
- Consequently, `source present`, `historically measured`, `candidate` and `current E2E accepted` must never be treated as equivalent. The **file census is complete, capability/semantic-route census is not**. No `UNMAPPED` required capability may be concealed to manufacture a “specification complete” or “fully integrated” result.

## Release and storage policy

- This is a **GitHub-documentation revision**, not a rebuild of the original archive or wheel. Newly created design files are GitHub-only files, outside the 581-file imported-ZIP checksum index. Any edited imported text requires the corresponding `checksums.repo.sha256` entry to be updated.
- Do not mutate `RELEASE_MANIFEST.json`, `checksums.sha256`, source snapshots or previous acceptance records to make new work appear historically accepted.
- Do not write to `gatchimuchio/NOTNN-LLM-MINIDORA`. Any future executable redesign needs a separately identified version, source-diff evidence, appropriate tests and newly generated artifact hashes.


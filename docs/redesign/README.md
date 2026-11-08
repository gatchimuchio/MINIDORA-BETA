# MINIDORA — Whole-System Redesign Register

**Status:** Concrete successor design; not yet implemented, 2026-11-09 (JST)  
**Target:** The next independently versioned MINIDORA implementation, developed in `MINIDORA-BETA`  
**Starting evidence:** Frozen engine `minidora-notnn==0.6.0b1` and the read-only upstream public repository  
**Change class:** Documentation only. No engine code, test, benchmark, distribution, deployment or upstream repository was modified by creating this register.

## Decision

MINIDORA has been **redesigned from first principles as one system**, integrating every observed existing capability and every relevant capability proposed in the development history at the contract level. The architecture, responsibility boundaries, storage, language/learning core and execution semantics are decided in one specification. The present module tree, chatbot feature list and previous implementation order are not binding premises.

The redesign is a *candidate* for a future version. The historical beta distribution, its tests, source lock, integrity records and earlier evaluations remain separate and recoverable. A documented capability is not automatically an implemented, connected, accepted or independently demonstrated capability.

## Normative specification and supporting annexes

| File | Purpose | Owner of decision |
|---|---|---|
| [01 — Unified engine specification](01_SYSTEM_SPECIFICATION.md) | **Sole normative design:** complete core/port/store/API/effect architecture and acceptance boundary | Architecture |
| [02 — Capability register](02_CAPABILITY_REGISTER.md) | Inclusive inventory of 93 inherited, historically reported and emerging abilities; explicit status and verification obligation | Coverage annex |
| [05 — Verification and evidence](05_VERIFICATION_AND_EVIDENCE.md) | E2E, learning, reference, regression, quality and performance contracts | Evidence annex |
| [06 — Lineage and decisions](06_LINEAGE_AND_DECISIONS.md) | Actual reference scope, historic observations, supersession, resolved design decisions and non-claims | Lineage annex |

The former construction-plan and agent-work-order documents were retired from the current design in favor of direct architectural decisions in `01`. Their earlier content remains recoverable in Git history, not current design authority.

## Authority and precedence

1. User's active instruction, publication boundary and applicable repository instructions.
2. Historical baseline facts: [`SOURCE_LOCK.md`](../../SOURCE_LOCK.md), [`REPOSITORY_LOCK.md`](../../REPOSITORY_LOCK.md), [`BETA_READINESS.md`](../../BETA_READINESS.md), [`RELEASE_MANIFEST.json`](../../RELEASE_MANIFEST.json) and dated source/evaluation records.
3. **This design candidate** for a future version; it does not alter the frozen `0.6.0b1` contract or retroactively certify old performance.
4. Implementation choices must be justified by contract coverage, boundary preservation, measured effect and comparative alternatives. Existing code structure is reusable evidence, not a mandatory architectural endpoint.

Public documents describe **observable contracts**, not private HDS theory. Do not copy unpublished definitions, complete theory-to-source mappings or semantic teaching datasets into the repository. See [`PUBLICATION_BOUNDARY.md`](../../PUBLICATION_BOUNDARY.md).

## Initial observation limits

- Reviewed: accessible repository metadata, beta entry points and maintenance/lock/acceptance documents; upstream README, repository rules, public design index, capability-specification filenames, evaluation contract and canonical history; selected historical failure and improvement reports.
- Not completed: full byte-level source review of all 414 imported public source files, recovery of every old specification version, runtime connectivity tracing for every public capability, restoration of 359 upstream public test files, or fresh general-chat/GPQA/multi-OS/Cloud acceptance.
- Consequently, `source present`, `historically measured`, `candidate` and `current E2E accepted` must never be treated as equivalent. The future implementation must complete the unobserved **full census** before claiming total feature coverage and keep discrepancies visible.

## Release and storage policy

- This is a **GitHub-documentation revision**, not a rebuild of the original archive or wheel. Newly created design files are GitHub-only files, outside the 581-file imported-ZIP checksum index. Any edited imported text requires the corresponding `checksums.repo.sha256` entry to be updated.
- Do not mutate `RELEASE_MANIFEST.json`, `checksums.sha256`, source snapshots or previous acceptance records to make new work appear historically accepted.
- Do not write to `gatchimuchio/NOTNN-LLM-MINIDORA`. Any future executable redesign needs a separately identified version, source-diff evidence, appropriate tests and newly generated artifact hashes.


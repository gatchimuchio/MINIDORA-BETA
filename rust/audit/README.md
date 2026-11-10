# Public baseline identity audit

This directory records the identity and availability of the public predecessor population. It does **not** certify semantic inheritance or Rust acceptance. AST declarations, file presence and matching hashes are metadata evidence only.

The baseline is `gatchimuchio/MINIDORA-BETA` at `1c546e77007afa0a9eeb3a44f07da99d84c383eb`. The upstream extraction reference is `gatchimuchio/NOTNN-LLM-MINIDORA` at `9307d7e2b1799750ecef1957d85128fcbe403d02`. The original checkout, release manifest and historical checksums are unchanged.

## Observed population

| Population | Count | Current identity observation |
|---|---:|---|
| Upstream public source blobs | 414 | All 414 exact upstream blobs available |
| Beta source population including two additions | 416 | All 416 match the R2 beta census |
| Upstream test and fixture blob paths | 359 | All 359 exact upstream blobs available |
| Beta test and fixture blob paths | 147 | All 147 match the R2 beta census |
| Shared upstream/beta test paths | 146 | Membership recorded separately from execution |
| Upstream-only test and fixture paths | 213 | Available in the separate read-only upstream reference |
| Beta-only test path | 1 | Retained as a separate predecessor obligation |
| Current named capability obligations | 93 | Original identifiers and two independent status axes retained |

These counts are file-path populations. They are not numbers of executed test cases. The 359 paths include fixtures and package files; the 313 selected regression cases form a different population.

## Reproduce

With an existing read-only checkout of the exact upstream extraction commit:

```bash
cd /workspace/MINIDORA-BETA
python3 rust/audit/inventory.py --upstream /workspace/.minidora-cloud/upstream-reference
```

The script does not fetch, reset, modify or execute either predecessor. It reads public files and writes only its own reports under `rust/audit/`. An incorrect upstream HEAD or any blob mismatch fails the command. The `--upstream` argument can refer to another checkout of the same exact commit. Without it, upstream availability is explicitly unverified.

## Files and evidence limits

- `population.json` records counts, exact observed commit identities and explicit non-acceptance.
- `source_identity.tsv` records each source path, both expected and measured blob identities, SHA-256, byte length and top-level Python declarations. It does not infer function semantics or actual entry routes from those declarations.
- `test_identity.tsv` preserves every test/fixture identity and records availability. Applicability and Rust equivalents remain unreviewed until they receive separate, inspectable contract evidence.
- `requirements.tsv` preserves all 93 baseline requirements. `CAP-Dxx` denotes capability IDs; `ARCH-Dxx` denotes architecture decisions. Metadata generation leaves implementation and acceptance unverified.
- `unit-conversion-validation.json` records the scoped B02 mixed-length statistics implementation and its Linux component/ordinary-route checks. It does not close B02 or full successor acceptance.
- `unit-arithmetic-validation.json` records exact unit-aware binary addition/subtraction and saved-result explanation on the ordinary CLI route, plus workspace validation. It is partial B02 evidence; correction coverage and full successor acceptance remain limited.
- `unit-correction-validation.json` records a narrowly bounded operand correction from an actual saved calculation, retention of the original result and a typed stop when no actual result exists. It does not close B02 or full successor acceptance.
- `unit-binary-correction-validation.json` records actual-result-linked dimensionless multiplication, division, remainder and power corrections, retaining the original version. Nested/compound expressions, dimensional multiplication/division and full B02 acceptance remain open.
- `schema-migration-validation.json` records explicit alpha 7-to-alpha 8 store migration checks, legacy blob/revision retention and idempotence. It does not establish cross-target durability or full successor acceptance.
- `csv-statistics-validation.json` records the source-preserving named-column CSV statistics implementation and compile/lint/build checks. No test or CLI route was executed for this slice; B04 remains unverified.
- `json-statistics-validation.json` records exact statistics over a JSON Pointer-selected numeric array, source spans and compile/lint/build checks. No test or CLI route was executed for this slice; B04 remains unverified.
- `probability-normalization-validation.json` records exact normalization of explicitly supplied nonnegative finite weights and compile/lint/build checks. No test or CLI route was executed for this slice; B03 remains unverified.
- `finite-probability-statistics-validation.json` records exact expected value and variance for explicitly paired finite outcomes and weights, with compile/lint/build checks. No test or CLI route was executed for this slice; B03 remains unverified.
- `finite-event-probability-validation.json` records exact mass for an explicitly supplied event-value set over paired outcomes and weights, with compile/lint/build checks. No test or CLI route was executed for this slice; B03 remains unverified.
- `reference_documents.tsv` records exact public reference-document digests. A digest does not establish that a reader understood or implemented a document.

Do not replace these baseline observations with later acceptance claims. Record semantic reviews, Rust implementation paths, input/output observations and executed successor results separately, with their own code/test versions. No predecessor pass is a Rust pass.

## Historical evaluation contract recovered

The upstream `評価/評価契約_v3.md`, `評価/GPQA評価二系統契約_v1.json` and runner `tools/評価契約.py` were inspected. They require GPQA Diamond all 198 cases, dataset CSV SHA-256 `41d1213cd7a4998605a26c2798500652572007161b3a92817ba46b35befcd305`, shuffle seed 0, LIVE-only newly retrieved sources, Europe PMC/Crossref enabled, Wikipedia English and OpenAlex disabled. Gold is permitted only for scoring after execution, never learning. Independent parallel inheritance and same-Core serial learning each have a 90-minute full-population boundary; parallel inheritance has the historical 40/198 floor. No GPQA dataset was downloaded or evaluated during this identity audit.

The historical 24-question, 12-writing-task, ten-session and 30-transformation quality populations are requirements to reconstruct and fix, not current passing data. An exact item-level historical quality set has not been established by this audit. A separate, newly authored diagnostic with these counts and an item rubric is recorded under [`quality-v0.1.0`](quality-v0.1.0/README.md); it is not historical reconstruction and has no approved aggregate pass threshold. Its first Rust alpha CLI run yielded `SUSPEND` for all 116 response units. Alpha 2 yielded `COMMIT` for the 30 labelled meaning transformations and `SUSPEND` for the remaining 86 question, writing and conversation response units. Alpha 3 and 4 refined feature labeling, and alpha 5 broadened comparison phrasing while requiring explicit A/B statements. Alpha 6 normalizes Arabic, fullwidth and Kanji numeric counts for bounded comparisons. Alpha 7 adds exact decimal coefficients with Japanese scale units. The transformation and remaining-response counts are unchanged; all runs remain unscored. Resource, deployment and cross-platform acceptance are also unobserved.

## Additional source inspection

`source_contract_reviews.tsv` records 96 fully read source files: compatibility wrappers and small interface modules, exact polynomial/linear/statistical quantity mechanisms, finite hypothesis/causal models, bounded code execution, source-preserving composition/editing, and the existing scientific specialist family. Each row states its public contract and limitations. Candidate capability IDs are proposed links from inspected behavior, **not** proof of normal-input reachability or Rust equivalence. No predecessor tests were executed by that source-inspection step.

`static_graph.py` parses all available source/test Python modules to produce `static_imports.tsv`, `static_callables.tsv` and `static_summary.json`. The observed population has 758 parsed Python files and 18 other file types. Import references and lexical call syntax can be conditional, shadowed, dynamically replaced or unreachable. They are static discovery aids, not an executed call graph or semantic review of every body. No evaluated input/output corpus is extracted.

```bash
python3 rust/audit/static_graph.py --upstream /workspace/.minidora-cloud/upstream-reference
```

`implementation-obligations.md` preserves the full 93-requirement population, S01–S12, R2-01–R2-22, complete source/test accounting and external/platform prerequisites. Unresolved items remain required; this ledger does not authorize a reduced acceptance scope.

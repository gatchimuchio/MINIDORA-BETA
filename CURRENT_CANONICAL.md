# Historical Canonical-Lineage Reference (Imported Compatibility Note)

**This is a historical reference from the upstream MINIDORA project, not the current beta release's acceptance certificate.** It retains the original version boundaries and evidence labels. There is no single historical file that supersedes every implementation and evaluation lineage.

## Historical reference points

- **Implementation lineage:** 2026-10-01, three-system integration and goal-dependent execution; source reconstruction point `9bb8238bf6afb307ca04ff0c8a419ecbcd29e060`.
- **Performance lineage:** 2026-09-27, GPQA Diamond LIVE **40/198**; not remeasured on the 2026-10-01 implementation or this isolated beta.

Former upstream reference documents (some **not included** in this beta checkout): `正本系統.md` (lineages), `現行正本.md` (performance), `HDS_MINIDORA_現行正本.md` (public integrated implementation), `開発正本履歴.md` (development history).

## Later upstream development checkpoints (historical status at the stated date)

| Date | Reconstruction commit | Recorded local observation / limitation |
|---|---|---|
| 2026-10-11 | `5f80cd8eb16995a48cc187a3f81cf7f2fda9f5a7` | Added basic chat work, **not yet accepted**. Novel-topic quality acceptance and Cloud Run acceptance for a new MVP were incomplete; earlier implementation/performance references remained in force. |
| 2026-10-13 | `f9906b7e6818404adc8fc6f7f4c8f7c0000dc44f` | Added Google retrieval, conditional public transmission, document continuation and Cloud Run configuration. Local checks only; novel-topic and deployed service-account acceptance remained incomplete. |
| 2026-10-13 | `6a3b3faff1eae805f18872dc50961da816fcf8ea` | Connected definition markers in source material with the immediately following explanation; preserved grammatical agency/scope and prohibited filling in the subject of the defined term. **113** related checks passed; wider acceptance remained incomplete. |
| 2026-10-13 | `bf23d2c2b65709caa7b2522f31362bb33e575498` | Preserved direct adjective-like modifier + noun connection without expanding the source topic. **91** related checks passed; novel-topic and cloud acceptance incomplete. |
| 2026-10-13 | `81119451c290c3b7b1f4f06563cb3ce958d838fb` | Added bounded PDF-text acquisition via Google with pinned dependencies, document fingerprints, page links and parsing-warning rejection. **43** related checks passed; one older HTTP test failure also reproduced on the starting `main`. Novel quality/cloud acceptance incomplete. |
| 2026-10-13 | `fe567ba9b9d9ca087a360e7f0d0a05076428a216` | Limited diagnostics for invisible characters in conversation source text, retaining source characters, positions and content fingerprints; rejected ambiguous word-internal and evidence-crossing cases. **107** related checks passed; wider acceptance incomplete. |
| 2026-10-13 | `324919514ceba0202a3edaf8fb922d7ef3ce543f` | Restored document page records after JSON exchange, old HTML compatibility and rejection of uninterpreted PDFs. **141** related checks passed. Two known prompts reached HDS but were suspended for insufficient content; novel/cloud acceptance incomplete. |
| 2026-10-13 | `d63a9a472042a128521038865077528aac85281d` | Bounded extra retrieval from typed document insufficiency, preserving original request/evidence and auditing resource usage. **55** related checks plus **1** adjusted-expectation check passed; expanded a finite initial 6-candidate set to a 12-candidate bound. Wider acceptance incomplete. |
| 2026-10-13 | `45a383943df8fc58666689ee8e1e8f90de1c26be` | Bounded extra retrieval after a previous candidate's actual failure; retained earlier source/failure/resource information. **58** related plus **3** reinforcement and **3** final-impact checks passed. Re-evaluated up to 12 texts (2 searches × 6 texts). Wider acceptance incomplete. |

Each row is a **historical observation**, not a new test of `MINIDORA-BETA 0.6.0b1`. Past unaccepted states have intentionally not been rewritten based on later work.

## Historical deployed demonstrations (not beta acceptance)

- **D03, 2026-10-13:** upstream `main` `a38b793250c1dfa96e49af028fae58235324ddab`, HDS core `45a383943df8fc58666689ee8e1e8f90de1c26be`, Cloud Run revision `minidora-adk-verify-v4-a38b793250c1-80e504`. A single authenticated observation covered a web-content answer, citation-style follow-up, arithmetic-input change, suspension and refusal; the record describes 5 responses with 11 technical checks each and an independent v2 audit. Independent judge access, full general quality questions, completed public listing/video and formal submission were **not** established at that checkpoint.
- **D04, 2026-10-13:** upstream `main` `d2397cc3fc08330c9be59eb76211299c65f8e0ee`, Cloud Run revision `minidora-adk-verify-judge-d2397cc3fc08-7234f7`, same HDS core reconstruction point. Following user-authorized public invoker access, the record reports 19 HTTPS checks using Basic authentication only (0 owner OIDC token uses), 3 live responses with 11 checks each, and an independent v2 audit. It did **not** establish direct-browser credential entry to answer display, full general quality questions or formal submission.

Historical service URL: https://minidora-adk-verify-qp5ojmzmyq-an.a.run.app  
Historical 180-second D03 demonstration: https://youtu.be/q7hF26QBabI

**Neither historical Cloud Run demonstration updates the GPQA performance reference or constitutes acceptance of the newly isolated beta.** Service availability today has not been reverified.

## Compatibility and publication

The historical filename `HDS_MINIDORA_現行正本.md` is retained for existing links. No private internal theory is published; `PUBLICATION_BOUNDARY.md` governs public documentation. For the **current isolated beta**, use `BETA_READINESS.md`, `SOURCE_LOCK.md`, `RELEASE_MANIFEST.json`, and `DOCS_REVISION.md`, not the dates above.

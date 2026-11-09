# Author-created Japanese quality diagnostic v0.1.0

Created 2026-11-09 in response to the owner's request to author the missing evaluation items and scoring guidance. This is a **new, provisional diagnostic population**, not a reconstruction of the historical 24-question / 12-writing / 10-session / 30-transformation item sheets or their unknown acceptance thresholds. The historical source and rubric remain unidentified. Do not cite results from this population as historical scores or as full product acceptance.

## Contents and fixed population

| File | Population | Scored unit | Maximum |
|---|---:|---|---:|
| `questions-24.md` | 24 natural Japanese questions, five topic families | One response, 0–4 | 96 |
| `writing-12.md` | 12 constrained writing and editing tasks | One artifact, 0–10 | 120 |
| `conversations-10.md` | 10 conversations, five user turns each | Each of 50 assistant turns, 0–4 | 200 |
| `meaning-transformations-30.md` | 30 paired-meaning comparisons | One comparison, 0–4 | 120 |

The questions are newly authored and self-contained wherever practicable. Conversation context is local to each session. No external calls, retrieved facts, hidden labels, or pre-inserted action results are needed. The population is public in this repository from its creation; it is a fixed diagnostic/regression set and **cannot establish future first-seen performance or uncontaminated holdout quality**.

[`manifest.json`](manifest.json) fixes the counts, execution status, and SHA-256 identities of the authored files and any recorded run. Run `python build_assets.py` from this directory to regenerate the manifest and score sheets after an intentional edit.

To capture an unscored local alpha diagnostic through the ordinary CLI, run:

```bash
python run_diagnostic.py --output diagnostic-alpha1-observations.jsonl
```

The runner uses a temporary database, separate actors for independent items, and one actor/case per five-turn conversation. It grants no external access, records exact request/output bytes and hashes, and leaves human scores blank. A run record is an observation set, not acceptance evidence.

The alpha 1 run artifact is [`diagnostic-alpha1-observations.jsonl`](diagnostic-alpha1-observations.jsonl), with an unscored row sheet at [`scoring-sheet-alpha1.tsv`](scoring-sheet-alpha1.tsv). All 116 requests ended in `SUSPEND` with the same unsupported-grammar response. In alpha 2, the labelled meaning-comparison route reached `COMMIT` on all 30 transformations; the 24 questions, 12 writing tasks and 50 conversation turns still ended in `SUSPEND`. Output inspection found false feature labels for some obligation, condition and participant phrases. Alpha 3 refines those bounded rules and again reaches `COMMIT` on all 30 transformations; the other 86 response units remain `SUSPEND`. All three runs are unscored. They diagnose input coverage and do not claim a quality score or acceptance result. See the [alpha 2 observations](diagnostic-alpha2-observations.jsonl), [alpha 2 score sheet](scoring-sheet-alpha2.tsv), [alpha 3 observations](diagnostic-alpha3-observations.jsonl), and [alpha 3 score sheet](scoring-sheet-alpha3.tsv).

## Scoring

For questions, conversations, and meaning transformations, use the same response-level anchors:

| Score | Anchor |
|---:|---|
| 4 | Correct, directly answers the request, covers the case-specific required points, preserves scope, and states uncertainty where needed. |
| 3 | Core answer is correct and useful; one minor omission or imprecision does not change the conclusion. |
| 2 | Partly useful, but materially incomplete, vague, or weakly supported; no central reversal. |
| 1 | Mostly misses the request or includes a material error that undermines the answer. |
| 0 | No usable answer, central contradiction, fabricated support, or a response unrelated to the task. |

Each writing artifact receives five dimension scores, each 0–2: (1) requested purpose/content, (2) fidelity to supplied facts and no invention, (3) requested voice and audience, (4) format and length constraints, and (5) clarity and practical usefulness. A score of 2 meets the item-specific checks, 1 partly meets them, and 0 misses them. The five dimensions are equally weighted. A polished artifact cannot compensate for a changed date, amount, name, or other required fact. Count Japanese length constraints in Unicode scalar values after NFC normalization, excluding line breaks and Markdown-only syntax; include punctuation and spaces.

Each item also has a separate critical-error flag. Mark it when the output reverses a central fact or relation, invents source support, violates a safety-relevant item constraint, discloses information the prompt says to remove, or claims an external action occurred without evidence. Report flags separately; do not silently turn them into extra points or hide them in an average.

## Collection and reporting protocol

Freeze the exact code revision and executable hash first. Run every question, writing item, and transformation in a fresh state. Run each conversation in one fresh state across its five turns and record the complete transcript. Use the ordinary public CLI route, preserve the exact input/output bytes, terminal state, elapsed time, permissions, budgets, and dependency/environment identity. Disable external effects and retrieval for this diagnostic population. If an item suspends or rejects, record that state and score the user-visible response; do not inject a successful intermediate result. Do not retry selectively.

Score after capturing outputs. Use the case-specific required points as the answer key, not lexical matching. When possible, have two human scorers rate independently, reconcile disagreements with a written note, and retain both original ratings. A score assigned by the implementation author is self-evaluation, not independent evaluation.

Report each category separately: raw points, possible points, per-item scores, critical-error flags, terminal states, and missing observations. A normalized category percentage may be shown as `earned / possible × 100`; do not combine unlike categories into one headline score. No aggregate pass threshold is set here because the historical threshold is missing and the owner has not approved a replacement. A later decision to adopt this population as an acceptance gate needs an explicit owner decision and a separately fixed threshold. This suite alone does not close the R2 first-seen-quality, full-language, source-grounding, or product-acceptance requirements.

## Item-specific prompt and key files

- [24 questions](questions-24.md)
- [12 writing tasks](writing-12.md)
- [10 five-turn conversations](conversations-10.md)
- [30 meaning transformations](meaning-transformations-30.md)
- [Scoring sheet template](scoring-sheet-template.tsv)
- [Population manifest](manifest.json)

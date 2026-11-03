# MINIDORA Engine Beta 1 — Scoped Acceptance Record (2026-11-03)

## Decision and scope

**Accepted as a beta distribution of a general-purpose execution-engine architecture**, within the finite mechanism, test and environment boundaries below. This is **not** acceptance of every ability, every source module, production readiness or universal reasoning performance.

- Package: `minidora-notnn==0.6.0b1`
- Upstream source: `gatchimuchio/NOTNN-LLM-MINIDORA`, `main` at `9307d7e2b1799750ecef1957d85128fcbe403d02` (same at initial and final observation)
- Machines: input `HDS入力系`, core `HDS駆動コア`, output `HDS出力系`, public beta facade `ミニドラβ実行機`
- Upstream public source: **414 files present**; 413 byte-identical Git blobs; 1 deliberately modified file; 2 new entry-point files; **0 missing original source files**.

## Observed evidence

1. **313 selected regression cases passed, 0 failures, 0 errors** — 2026-11-03, Linux, Python 3.12.
2. Wheel `minidora_notnn-0.6.0b1-py3-none-any.whl` built successfully from the **original beta distribution**.
3. That wheel installed in an isolated temporary target; package version verified as `0.6.0b1`.
4. A controlled installed-wheel input → core → output test resulted in terminal `COMMIT`, the output state `成立`, successful rendering and **zero implicit external deliveries**.
5. Git blob comparison confirmed all original 414 source paths: the sole deliberate change and two new sources are recorded in `RELEASE_MANIFEST.json`.

### Regression modules included

These are Python module names, deliberately kept in their exact Japanese source spelling:

```text
test_HDS学習機械循環_v2
test_HDS内包統合_v2
test_HDS内包統合_v2追加
test_HDS原子的更新非退行
test_HDS非退行包絡
test_HDS実行主体_v1
test_HDS目的保持_v1
test_HDS形成隔離復帰_v1
test_MINIDORAコア閉包_v3
test_MINIDORAコア完成_v2
test_HDSコア入力優先_v1
test_HDS正本継承循環_v1
test_HDS配線境界_v3
test_HDS自律接続_v3
test_実行系_HDS_v1
test_β三機駆動
目的関係修正_v1.test_実行契約
目的関係修正_v1.test_循環回帰
目的関係修正_v1.test_有限探索
目的関係修正_v1.test_目的証拠
目的関係修正_v1.test_計画契約
```

The beta bundle includes **147 test files** (including the additional beta test). This is **not** a claim that all **359 upstream public test files** were restored or passed. Reproduce the selected suite with `python tools/beta_acceptance.py`.

## Deliberate changes in the frozen engine beta

- When resuming after an explicit external-input update, bounded re-reading and final revalidation can use the existing initial action budget plus its policy-defined increment. The goal-progress decision is unchanged.
- A single extra finite stage of an action may be considered **within the existing normal budget** only when the action newly forms a declared intermediate state and declares production of a required state. Irrelevant internal-counter changes do not cause unbounded repetition.
- A legacy test's reference-identity assertion was aligned to the current `主体辞書()` defensive-copy contract: equal value, distinct object identity.
- The beta facade rejects a repeated `(案件ID, 入力ID, 入力版)` before input acceptance.

## Exclusions and unverified areas

- Unincluded tests, old saved fixtures, a Cloud Run deployment, full-duration evaluations and entire upstream acceptance are **outside this acceptance**.
- No new numerical measurement of general inference, Japanese language quality or GPQA was made for this beta.
- The beta facade accepts general text; specialized selection/relation input continues through inherited core APIs.
- Result lookup and duplicate suppression are **per-process, per-instance in-memory contracts**, not durable persistence across restarts.
- External delivery, deployed cloud/search access, and autonomous success on arbitrary tasks are **not guaranteed**.
- Successful execution of a **supplied test action** is not evidence of autonomous arithmetic or general intelligence.
- Multi-OS acceptance via the repository's manually dispatched GitHub Actions workflow has **not been claimed**.

## Release discipline

The original `0.6.0b1` engine distribution is immutable; future **implementation changes** require new versioning, diffs and hashes. The upstream repository was not modified. The publication boundary in `PUBLICATION_BOUNDARY.md` excludes internal/private theory and large derived semantic datasets. English documentation localization is a **separately tracked, documentation-only revision**, explained in `DOCS_REVISION.md`; it does not revise this dated acceptance evidence.

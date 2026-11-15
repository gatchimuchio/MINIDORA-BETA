# BUILD_BATCH_48 — Cross-actor prior-result context rejection

**main基線:** `7d791544815b30d97aa7086106aed7ed995e3a19`（B47）。

**主語・目的:** Coreがcontext selectorとして渡されたresult IDを、現在のactor/caseで実際に利用可能な前回resultと照合し、別actorのIDから内容を読ませないことを確かめる。対象はR2 `I02` / `I06` / `CAP-D11`。B47は一actorのrule失効が他actorのmemoryへ波及しないことを確認した。

**施工範囲:** 隔離SQLite DB上の`minidora-engine` integration test一件と局所記録。通常CLI/HTTP、production code、保存契約、R4三文書は変更しない。`CAP-D11=UNVERIFIED`、`CORE-G03=PARTIAL`を維持する。

## 実producer→consumer

二actorが同一scopeに別々の直接factとrelation-question ResultRecordを保存した。Engineを再起動後、actor Aは自分のcaseでactor Bのcommitted result IDを`RequestEnvelope.context_ref`に渡した。Aのcaseに紐づく実前回resultは別IDなので、入力封印は`CONTEXT_MISMATCH`となりCoreは`FAIL / INPUT_FAILURE`を記録した。失敗resultにはContentPlan/renderもBの関係文面もなく、AのcaseにはA自身の先行result historyが保持された。

AとBのmemory projectionは値比較で前後一致した。その後、AはA自身、BはB自身のprior result IDを選び、それぞれ`COMMIT / RENDERED`した。これは二actor・二case・一scope・明示context mismatch一件の局所Core fixtureであり、CLI/HTTPからのcontext指定、通常利用経路、並行writer、他scope、outside-world truth、CAP-D11全受入は確認していない。

## 限定確認

- `cargo test --locked -p minidora-engine --test core_actor_context_boundary_48 -- --nocapture`: **PASS**（1 test、exit 0）。結果は[focused test log](b48-focused-tests.log)。
- `cargo fmt --all -- --check`: **PASS**。
- `cargo metadata --locked --no-deps --format-version 1`: **PASS**（3 workspace members）。
- R4 manifest、三文書、ZIP members/hash/size再照合: **PASS**。
- 初回test compileはfixture内の`EvidencePacket`を文字列として扱った型誤りで**FAIL**し、test本体は未実行。その後JSON化したfixture assertionへ修正し同一focused testを再実行してPASS。compile失敗は[初回build log](b48-initial-compile-failure.log)に保存。
- workspace全test、Clippy、全workspace check/build、normal CLI/HTTP、parallel writer、CAP-D11全受入、独立QA、release/deployment: **NOT_RUN**。

環境はLinux x86_64、Rust/Cargo 1.99.1。build targetは`/workspace/.minidora-cloud/rust-target`。終了確認時の空き容量は43G。test専用temp stateは削除済み。Windows storage routineはLinuxのためN/A。OneDrive操作、外部保管、ゴミ箱移動なし。

## 資産閉包

| 状態 | 個数 | 対象 |
|---|---:|---|
| keep | 8 | B48 integration test、CAP-D11台帳差分、status、choices、本記録、観測JSON、focused pass log、初回compile failure log |
| store | 0 | 外部保管へ移動した資料なし |
| in-use | 3 | Rust target（7.8G）、Cargo cache（252M）、Rust toolchain（617M） |
| trash | 0 | 削除・ゴミ箱移動なし |
| hold | 0 | 保留資産なし |
| **合計** | **11** | keep 8 + store 0 + in-use 3 + trash 0 + hold 0 |

## 復旧・次consumer

編集前にCASでlocal recovery branch/tagとremote recovery tagを`backup-main=7d79154`（B47）、`backup-main-prev=3108c73`（B46）へ進めた。旧値はleaseで照合し、legacy remote backup branchesは変更していない。次consumerは通常入口からのactor/case ownership境界と継続会話の統合確認。B48から通常CLI/HTTP全経路やCAP-D11全受入へ昇格しない。

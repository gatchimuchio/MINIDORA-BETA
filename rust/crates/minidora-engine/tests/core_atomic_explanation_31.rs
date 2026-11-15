use minidora_domain::{Intent, Lifecycle, Proposition, RequestEnvelope};
use minidora_engine::{Engine, store::Store};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let serial = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "minidora-core-atomic-explanation-31-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create isolated atomic explanation state");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn proposition(object: &str) -> Proposition {
    Proposition {
        subject: "Momo".into(),
        predicate: "is".into(),
        object: object.into(),
        negative: false,
        scope: "atomic-explanation-lab".into(),
    }
}

fn request(case: &str, input: &str, actor: &str) -> RequestEnvelope {
    RequestEnvelope::new(case, input, actor, "")
}

#[test]
fn typed_atomic_explanation_reaches_k7_and_stops_without_evidence_or_write_authority() {
    let directory = TestDirectory::new();
    let actor = "b31-atomic-explanation-observer";
    let mut engine = Engine::open(directory.path()).expect("open isolated Core state");

    let seed = engine
        .execute(
            actor,
            RequestEnvelope::new(
                "atomic-explanation-seed",
                "momo-bird-premise",
                actor,
                "[scope=atomic-explanation-lab]Momo is a bird",
            ),
        )
        .expect("seed actor memory with a bounded relation premise");
    assert_eq!(seed.lifecycle, Lifecycle::Commit, "{seed:?}");
    let premise_id = seed
        .dependencies
        .first()
        .expect("seed result exposes its adopted premise ID")
        .clone();

    let supported_proposition = proposition("bird");
    let supported = engine
        .compose_atomic_explanation(
            actor,
            request("atomic-explanation-supported", "explain-momo", actor),
            supported_proposition.clone(),
        )
        .expect("execute typed atomic explanation");
    assert_eq!(supported.lifecycle, Lifecycle::Commit, "{supported:?}");
    assert!(supported.dependencies.contains(&premise_id));
    let plan = supported
        .content_plan
        .as_ref()
        .expect("Core adopted a generated content plan");
    assert!(!plan.text.is_empty());
    assert!(!plan.required_units.is_empty());
    assert!(plan.dependencies.contains(&premise_id));
    let render = supported
        .render
        .as_ref()
        .expect("K7 produced an actual render observation");
    assert_eq!(render.status, "RENDERED");
    assert_eq!(render.plan_id, plan.id);
    assert_eq!(render.bytes, plan.text.as_bytes());
    assert!(!render.actual_sha256.is_empty());
    assert!(supported.execution_graph_ref.is_some());

    let supported_case = engine
        .latest(actor, "atomic-explanation-supported")
        .expect("read supported case")
        .expect("supported case exists");
    let supported_bundle = supported_case
        .bundle
        .as_ref()
        .expect("typed request was sealed in Core");
    assert_eq!(
        supported_bundle.meaning.source_kind,
        "CORE_TYPED_ATOMIC_MEANING_V1"
    );
    assert_eq!(
        supported_bundle.meaning.parser_version,
        "core-typed-atomic-meaning-v1"
    );
    assert_eq!(
        supported_bundle.meaning.intent,
        Intent::AskWhy {
            proposition: supported_proposition
        }
    );
    let direct_input: Value =
        serde_json::from_str(&supported_bundle.request.text).expect("sealed typed input JSON");
    assert_eq!(direct_input["purpose"], "explain");
    assert_eq!(direct_input["source_ids"][0], premise_id);

    let missing = engine
        .compose_atomic_explanation(
            actor,
            request("atomic-explanation-missing", "explain-unsupported", actor),
            proposition("mammal"),
        )
        .expect("unsupported typed explanation returns a terminal result");
    assert_eq!(missing.lifecycle, Lifecycle::Suspend, "{missing:?}");
    assert!(
        missing
            .reason
            .as_deref()
            .unwrap_or_default()
            .contains("MISSING_PREMISES")
    );
    assert!(missing.content_plan.is_none());
    assert!(missing.render.is_none());

    let forbidden_path = directory.path().join("unauthorized-output.txt");
    let mut denied_request = request("atomic-explanation-denied-write", "explain-write", actor);
    denied_request.output_path = Some(forbidden_path.clone());
    let denied = engine
        .compose_atomic_explanation(actor, denied_request, proposition("bird"))
        .expect("unauthorized write returns a terminal result");
    assert_eq!(denied.lifecycle, Lifecycle::Suspend, "{denied:?}");
    assert!(
        denied
            .reason
            .as_deref()
            .unwrap_or_default()
            .contains("WRITE_PERMISSION_MISSING")
    );
    assert!(denied.dependencies.contains(&premise_id));
    assert!(denied.content_plan.is_some());
    assert_eq!(
        denied.render.as_ref().map(|render| render.status.as_str()),
        Some("RENDERED")
    );
    assert!(denied.receipts.is_empty());
    assert!(!forbidden_path.exists());
    assert!(
        Store::open(directory.path())
            .expect("open isolated action ledger")
            .actions(actor, "atomic-explanation-denied-write")
            .expect("read denied write action ledger")
            .is_empty()
    );
}

use minidora_domain::{Guarantee, Lifecycle, Proposition, RequestEnvelope};
use minidora_engine::{Engine, store::Store};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let serial = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "minidora-scope-classification-58-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create isolated scope-classification state");
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

fn request(case: &str, input: &str, actor: &str) -> RequestEnvelope {
    RequestEnvelope::new(case, input, actor, "")
}

fn relation(subject: &str, object: &str, scope: &str) -> Proposition {
    Proposition {
        subject: subject.into(),
        predicate: "is".into(),
        object: object.into(),
        negative: false,
        scope: scope.into(),
        temporal_scope: None,
        condition_refs: Default::default(),
    }
}

#[test]
fn exact_scope_miss_distinguishes_out_of_scope_from_missing_premises() {
    let directory = TestDirectory::new();
    let actor = "w06-scope-classification-actor";
    let case = "w06-scope-classification-case";
    let mut engine = Engine::open(directory.path()).expect("open one Core state");

    let premise = engine
        .assert_atomic_relation(
            actor,
            request(case, "archive-ore", actor),
            relation("Nera", "ore", "archive"),
        )
        .expect("retain a direct premise in its declared scope");
    assert_eq!(premise.lifecycle, Lifecycle::Commit, "{premise:?}");
    let premise_id = premise.dependencies[0].clone();
    let memory_before_queries = Store::open(directory.path())
        .expect("open memory before scope queries")
        .load_memory(actor)
        .expect("read actor memory before scope queries");

    let out_of_scope = engine
        .compose_atomic_explanation(
            actor,
            request(case, "ask-ore-in-lab", actor),
            relation("Nera", "ore", "lab"),
        )
        .expect("report the matching direct premise from a different scope");
    assert_eq!(
        out_of_scope.lifecycle,
        Lifecycle::Suspend,
        "{out_of_scope:?}"
    );
    assert_eq!(out_of_scope.guarantee, Guarantee::Unknown);
    assert!(
        out_of_scope
            .reason
            .as_deref()
            .is_some_and(|reason| reason.starts_with("OUT_OF_SCOPE"))
    );
    assert!(out_of_scope.dependencies.contains(&premise_id));
    assert!(out_of_scope.content_plan.is_none());
    assert!(out_of_scope.render.is_none());

    let missing = engine
        .compose_atomic_explanation(
            actor,
            request(case, "ask-blue-in-lab", actor),
            relation("Nera", "blue", "lab"),
        )
        .expect("report a relation with no matching scoped premise");
    assert_eq!(missing.lifecycle, Lifecycle::Suspend, "{missing:?}");
    assert_eq!(missing.guarantee, Guarantee::Unknown);
    assert!(
        missing
            .reason
            .as_deref()
            .is_some_and(|reason| reason.starts_with("MISSING_PREMISES"))
    );
    assert!(missing.dependencies.is_empty());
    assert!(missing.content_plan.is_none());
    assert!(missing.render.is_none());

    let memory_after_queries = Store::open(directory.path())
        .expect("open memory after scope queries")
        .load_memory(actor)
        .expect("read actor memory after scope queries");
    assert_eq!(memory_after_queries, memory_before_queries);
}

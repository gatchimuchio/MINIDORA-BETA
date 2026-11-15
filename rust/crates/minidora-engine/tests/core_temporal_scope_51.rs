use minidora_domain::{Guarantee, Intent, Lifecycle, Proposition, RequestEnvelope};
use minidora_engine::{Engine, store::Store};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let serial = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "minidora-core-temporal-scope-51-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create isolated temporal-scope state");
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

fn relation(subject: &str, object: &str, scope: &str, temporal_scope: Option<&str>) -> Proposition {
    Proposition {
        subject: subject.into(),
        predicate: "is".into(),
        object: object.into(),
        negative: false,
        scope: scope.into(),
        temporal_scope: temporal_scope.map(str::to_owned),
        condition_refs: Default::default(),
    }
}

fn request(case: &str, input: &str, actor: &str) -> RequestEnvelope {
    RequestEnvelope::new(case, input, actor, "")
}

#[test]
fn core_temporal_identity_requires_exact_time_and_keeps_comparison_sides_distinct() {
    let directory = TestDirectory::new();
    let actor = "b51-temporal-scope";
    let scope = "b51-materials-lab";
    let morning = "2026-11-15T10:00Z";
    let evening = "2026-11-15T18:00Z";
    let mut engine = Engine::open(directory.path()).expect("open isolated Core state");

    let rule = engine
        .add_atomic_relation_rule(
            actor,
            request("b51-temporal-seeds", "ore-to-refined", actor),
            vec![relation("$x", "ore", scope, Some(morning))],
            relation("$x", "refined", scope, Some(morning)),
        )
        .expect("persist typed time-scoped rule through Core");
    assert_eq!(rule.lifecycle, Lifecycle::Commit, "{rule:?}");
    assert_eq!(rule.guarantee, Guarantee::UserPremise);

    let ore = engine
        .assert_atomic_relation(
            actor,
            request("b51-temporal-seeds", "nera-is-ore", actor),
            relation("Nera", "ore", scope, Some(morning)),
        )
        .expect("persist typed time-scoped premise through Core");
    assert_eq!(ore.lifecycle, Lifecycle::Commit, "{ore:?}");
    assert_eq!(ore.guarantee, Guarantee::UserPremise);
    let rule_id = rule.dependencies[0].clone();
    let ore_id = ore.dependencies[0].clone();

    let target = relation("Nera", "refined", scope, Some(morning));
    let result = engine
        .compose_atomic_explanation(
            actor,
            request("b51-time-match", "explain-refined", actor),
            target.clone(),
        )
        .expect("resolve and render the same-time proof through Core");
    assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
    assert_eq!(result.guarantee, Guarantee::Derived);
    for proof_id in [&rule_id, &ore_id] {
        assert!(result.dependencies.contains(proof_id), "lost {proof_id}");
    }
    let plan = result.content_plan.as_ref().expect("adopted ContentPlan");
    assert_eq!(plan.guarantee, Guarantee::Derived);
    assert!(plan.text.contains(morning));
    assert!(plan.dependencies.contains(&rule_id));
    assert!(plan.dependencies.contains(&ore_id));
    let render = result.render.as_ref().expect("K7 render");
    assert_eq!(render.status, "RENDERED");
    assert_eq!(render.bytes, plan.text.as_bytes());

    let saved = engine
        .latest(actor, "b51-time-match")
        .expect("read sealed typed query")
        .expect("query revision is present");
    let bundle = saved.bundle.expect("query bundle is sealed by Core");
    assert_eq!(
        bundle.meaning.intent,
        Intent::AskWhy {
            proposition: target
        }
    );
    assert_eq!(bundle.meaning.source_kind, "CORE_TYPED_ATOMIC_MEANING_V1");

    let other_time = engine
        .compose_atomic_explanation(
            actor,
            request("b51-time-mismatch", "explain-refined-evening", actor),
            relation("Nera", "refined", scope, Some(evening)),
        )
        .expect("a query at another time reaches Core's ordinary stop path");
    assert_eq!(other_time.lifecycle, Lifecycle::Suspend, "{other_time:?}");
    assert!(
        other_time
            .reason
            .as_deref()
            .is_some_and(|reason| reason.starts_with("MISSING_PREMISES:"))
    );
    assert!(other_time.content_plan.is_none());
    assert!(other_time.render.is_none());
    assert_eq!(
        result.lifecycle,
        Lifecycle::Commit,
        "prior result is immutable"
    );

    let aster = engine
        .assert_atomic_relation(
            actor,
            request("b51-comparison-seeds", "aster-rare-at-morning", actor),
            relation("Aster", "rare ore", scope, Some(morning)),
        )
        .expect("persist Aster's timed relation");
    let beryl = engine
        .assert_atomic_relation(
            actor,
            request("b51-comparison-seeds", "beryl-rare-at-evening", actor),
            relation("Beryl", "rare ore", scope, Some(evening)),
        )
        .expect("persist Beryl's relation at another time");
    assert_eq!(aster.lifecycle, Lifecycle::Commit, "{aster:?}");
    assert_eq!(beryl.lifecycle, Lifecycle::Commit, "{beryl:?}");

    let mut comparison_request = request("b51-temporal-comparison", "compare", actor);
    comparison_request.budget.max_actions = 128;
    let comparison = engine
        .compose_atomic_relation_comparison(actor, comparison_request, "Aster", "Beryl", scope)
        .expect("compare Core inventories with distinct temporal identities");
    assert_eq!(comparison.lifecycle, Lifecycle::Commit, "{comparison:?}");
    assert!(comparison.text.contains(morning));
    assert!(comparison.text.contains(evening));
    assert!(comparison.text.contains("では同じ関係の支持を確認できず"));
    assert!(!comparison.text.contains("両側で確認した同じ関係"));

    let store = Store::open(directory.path()).expect("inspect persisted temporal relations");
    let memory = store.load_memory(actor).expect("load actor memory");
    assert!(memory.facts.iter().any(|fact| {
        fact.id == ore_id && fact.proposition.temporal_scope.as_deref() == Some(morning)
    }));
    assert!(memory.rules.iter().any(|stored_rule| {
        stored_rule.id == rule_id
            && stored_rule.premise.temporal_scope.as_deref() == Some(morning)
            && stored_rule.conclusion.temporal_scope.as_deref() == Some(morning)
    }));

    println!(
        "B51_TEMPORAL_SCOPE_OBSERVATION={}",
        serde_json::to_string(&json!({
            "work_item": "B51 exact temporal scope in Core relation identity",
            "actor": actor,
            "scope": scope,
            "same_time": morning,
            "different_time": evening,
            "proof_ids": [rule_id, ore_id],
            "same_time_result": {
                "lifecycle": result.lifecycle,
                "guarantee": result.guarantee,
                "dependencies": result.dependencies,
                "text": result.text,
                "content_plan_id": plan.id,
                "render_status": render.status,
            },
            "different_time_result": {
                "lifecycle": other_time.lifecycle,
                "reason": other_time.reason,
                "content_plan": other_time.content_plan.is_some(),
                "render": other_time.render.is_some(),
            },
            "comparison": {
                "lifecycle": comparison.lifecycle,
                "common_label_present": comparison.text.contains("両側で確認した同じ関係"),
                "different_time_labels_present": comparison.text.contains(morning) && comparison.text.contains(evening),
            },
            "gate_assessment": {
                "CORE-G01": "PARTIAL; exact time labels now participate in typed relation identity, rules, proof queries, explanation content and comparison grouping; natural-language time parsing and chronology remain open",
            },
            "limits": ["typed Core API", "exact label equality only", "no chronology or interval reasoning"]
        }))
        .expect("serialize B51 runtime observation")
    );
}

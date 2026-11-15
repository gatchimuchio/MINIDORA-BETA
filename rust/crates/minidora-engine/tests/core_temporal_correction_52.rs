use minidora_domain::{
    Guarantee, Intent, Lifecycle, Proposition, RequestEnvelope, SCHEMA_VERSION, digest,
};
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
            "minidora-core-temporal-correction-52-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create isolated temporal-correction state");
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

fn relation(subject: &str, object: &str, scope: &str, time: &str) -> Proposition {
    Proposition {
        subject: subject.into(),
        predicate: "is".into(),
        object: object.into(),
        negative: false,
        scope: scope.into(),
        temporal_scope: Some(time.into()),
    }
}

fn request(case: &str, input: &str, actor: &str) -> RequestEnvelope {
    RequestEnvelope::new(case, input, actor, "")
}

#[test]
fn typed_temporal_correction_invalidates_only_the_exact_dependent_proof() {
    let directory = TestDirectory::new();
    let actor = "b52-temporal-correction";
    let case = "b52-temporal-correction-case";
    let scope = "b52-materials-lab";
    let morning = "2026-11-15T08:00Z";
    let midday = "2026-11-15T12:00Z";
    let evening = "2026-11-15T18:00Z";
    let mut engine = Engine::open(directory.path()).expect("open isolated Core state");

    let rule = engine
        .add_atomic_relation_rule(
            actor,
            request(case, "b52-rule", actor),
            vec![relation("$x", "ore", scope, morning)],
            relation("$x", "refined ore", scope, morning),
        )
        .expect("persist exact-time rule through Core");
    assert_eq!(rule.lifecycle, Lifecycle::Commit, "{rule:?}");
    assert_eq!(rule.guarantee, Guarantee::UserPremise);
    let rule_id = rule.dependencies[0].clone();

    let original_premise = relation("Nera", "ore", scope, morning);
    let original = engine
        .assert_atomic_relation(
            actor,
            request(case, "b52-original-morning-premise", actor),
            original_premise.clone(),
        )
        .expect("persist the morning premise");
    let original_fact_id = original.dependencies[0].clone();
    let evening_control = engine
        .assert_atomic_relation(
            actor,
            request(case, "b52-evening-control", actor),
            relation("Nera", "ore", scope, evening),
        )
        .expect("persist same-subject, different-time control");
    let evening_fact_id = evening_control.dependencies[0].clone();
    let other_subject_control = engine
        .assert_atomic_relation(
            actor,
            request(case, "b52-other-subject-control", actor),
            relation("Mira", "ore", scope, morning),
        )
        .expect("persist same-time, different-subject control");
    let other_subject_fact_id = other_subject_control.dependencies[0].clone();

    let original_proof = engine
        .compose_atomic_explanation(
            actor,
            request(case, "b52-nera-morning-derived", actor),
            relation("Nera", "refined ore", scope, morning),
        )
        .expect("derive the morning conclusion through Core");
    assert_eq!(
        original_proof.lifecycle,
        Lifecycle::Commit,
        "{original_proof:?}"
    );
    assert_eq!(original_proof.guarantee, Guarantee::Derived);
    assert!(original_proof.dependencies.contains(&rule_id));
    assert!(original_proof.dependencies.contains(&original_fact_id));
    let original_plan = original_proof
        .content_plan
        .as_ref()
        .expect("derived result has adopted content");
    assert!(original_plan.dependencies.contains(&original_fact_id));
    assert_eq!(
        original_proof
            .render
            .as_ref()
            .map(|render| render.status.as_str()),
        Some("RENDERED")
    );

    let before_correction = Store::open(directory.path())
        .expect("open Core store for pre-correction revision")
        .load_memory(actor)
        .expect("read pre-correction memory");
    let replacement = relation("Nera", "ore", scope, midday);
    let correction = engine
        .correct_atomic_relation(
            actor,
            request(case, "b52-correct-time", actor),
            original_premise.clone(),
            replacement.clone(),
        )
        .expect("correct the premise's exact temporal scope through Core");
    assert_eq!(correction.lifecycle, Lifecycle::Commit, "{correction:?}");
    assert_eq!(correction.guarantee, Guarantee::UserPremise);
    assert!(correction.dependencies.contains(&original_fact_id));

    let corrected_case = engine
        .latest(actor, case)
        .expect("read sealed correction case")
        .expect("correction revision exists");
    let correction_bundle = corrected_case.bundle.expect("correction input is sealed");
    assert_eq!(
        correction_bundle.meaning.source_kind,
        "CORE_TYPED_RELATION_CORRECTION_V1"
    );
    assert!(matches!(
        correction_bundle.meaning.intent,
        Intent::CorrectPremise { superseded, replacement: corrected }
            if superseded == original_premise && corrected == replacement
    ));
    assert_eq!(
        correction_bundle.raw_sha256,
        digest(correction_bundle.request.text.as_bytes())
    );

    drop(engine);
    let store = Store::open(directory.path()).expect("reopen durable Core store");
    let memory = store
        .load_memory(actor)
        .expect("read corrected actor memory");
    assert_eq!(memory.revision, before_correction.revision + 1);
    let fact = |id: &str| {
        memory
            .facts
            .iter()
            .find(|record| record.id == id)
            .unwrap_or_else(|| panic!("fact {id} was not retained"))
    };
    assert!(
        !fact(&original_fact_id).valid,
        "corrected fact stayed active"
    );
    assert!(
        fact(&evening_fact_id).valid,
        "different time was invalidated"
    );
    assert!(
        fact(&other_subject_fact_id).valid,
        "different subject was invalidated"
    );
    let old_negative = Proposition {
        negative: true,
        ..original_premise.clone()
    };
    assert!(memory.facts.iter().any(|record| {
        record.valid && record.proposition == old_negative && record.dependencies.is_empty()
    }));
    assert!(memory.facts.iter().any(|record| {
        record.valid && record.proposition == replacement && record.dependencies.is_empty()
    }));
    let replacement_fact_id = memory
        .facts
        .iter()
        .find(|record| {
            record.valid && record.proposition == replacement && record.dependencies.is_empty()
        })
        .expect("corrected-time premise is retained")
        .id
        .clone();
    assert!(correction.dependencies.contains(&replacement_fact_id));
    drop(store);

    let mut reopened = Engine::open(directory.path()).expect("reopen Core after correction");
    let corrected_old_query = reopened
        .compose_atomic_explanation(
            actor,
            request(case, "b52-nera-morning-after-correction", actor),
            relation("Nera", "refined ore", scope, morning),
        )
        .expect("re-evaluate the same morning conclusion after correction");
    assert_eq!(
        corrected_old_query.lifecycle,
        Lifecycle::Suspend,
        "{corrected_old_query:?}"
    );
    assert!(
        corrected_old_query
            .reason
            .as_deref()
            .is_some_and(|reason| reason.starts_with("MISSING_PREMISES:"))
    );
    assert!(corrected_old_query.content_plan.is_none());
    assert!(corrected_old_query.render.is_none());

    let corrected_time_query = reopened
        .compose_atomic_explanation(
            actor,
            request(case, "b52-nera-at-corrected-time", actor),
            replacement.clone(),
        )
        .expect("consume the corrected direct premise at noon");
    assert_eq!(corrected_time_query.lifecycle, Lifecycle::Commit);
    assert_eq!(corrected_time_query.guarantee, Guarantee::UserPremise);
    assert!(
        corrected_time_query
            .dependencies
            .contains(&replacement_fact_id)
    );

    let evening_query = reopened
        .compose_atomic_explanation(
            actor,
            request(case, "b52-nera-evening-control-query", actor),
            relation("Nera", "ore", scope, evening),
        )
        .expect("consume the unaffected same-subject evening premise");
    assert_eq!(evening_query.lifecycle, Lifecycle::Commit);
    assert!(evening_query.dependencies.contains(&evening_fact_id));

    let other_subject_query = reopened
        .compose_atomic_explanation(
            actor,
            request(case, "b52-mira-morning-control-query", actor),
            relation("Mira", "refined ore", scope, morning),
        )
        .expect("derive the unaffected same-time other-subject conclusion");
    assert_eq!(other_subject_query.lifecycle, Lifecycle::Commit);
    assert_eq!(other_subject_query.guarantee, Guarantee::Derived);
    assert!(
        other_subject_query
            .dependencies
            .contains(&other_subject_fact_id)
    );

    assert_eq!(original_proof.lifecycle, Lifecycle::Commit);
    assert!(original_proof.content_plan.is_some());
    println!(
        "MINIDORA_B52_TRACE={}",
        json!({
            "schema": "MINIDORA_CORE_TEMPORAL_CORRECTION_TRACE_V1",
            "software_schema": SCHEMA_VERSION,
            "actor": actor,
            "case": case,
            "world_scope": scope,
            "original_temporal_scope": morning,
            "replacement_temporal_scope": midday,
            "same_subject_control_temporal_scope": evening,
            "old_fact_id": original_fact_id,
            "rule_id": rule_id,
            "reopened_memory_revision_before": before_correction.revision,
            "reopened_memory_revision_after": memory.revision,
            "old_fact_valid_after": fact(&original_fact_id).valid,
            "evening_control_valid_after": fact(&evening_fact_id).valid,
            "other_subject_control_valid_after": fact(&other_subject_fact_id).valid,
            "original_proof": {
                "result_id": original_proof.id.clone(),
                "lifecycle": original_proof.lifecycle.clone(),
                "guarantee": original_proof.guarantee.clone(),
                "dependencies": original_proof.dependencies.clone(),
                "render": original_proof.render.as_ref().map(|render| render.status.clone()),
            },
            "correction": {
                "result_id": correction.id.clone(),
                "lifecycle": correction.lifecycle.clone(),
                "dependencies": correction.dependencies.clone(),
                "source_kind": correction_bundle.meaning.source_kind.clone(),
                "raw_sha256": correction_bundle.raw_sha256.clone(),
            },
            "after_correction": {
                "same_time_derived_query": {
                    "lifecycle": corrected_old_query.lifecycle.clone(),
                    "reason": corrected_old_query.reason.clone(),
                    "content_plan": corrected_old_query.content_plan.is_some(),
                    "render": corrected_old_query.render.is_some(),
                },
                "replacement_time_query": {
                    "lifecycle": corrected_time_query.lifecycle.clone(),
                    "guarantee": corrected_time_query.guarantee.clone(),
                    "dependencies": corrected_time_query.dependencies.clone(),
                },
                "other_time_query": {
                    "lifecycle": evening_query.lifecycle.clone(),
                    "dependencies": evening_query.dependencies.clone(),
                },
                "other_subject_query": {
                    "lifecycle": other_subject_query.lifecycle.clone(),
                    "guarantee": other_subject_query.guarantee.clone(),
                    "dependencies": other_subject_query.dependencies.clone(),
                },
                "prior_result_retained_in_memory": original_proof.lifecycle == Lifecycle::Commit,
            },
            "r4_scope": "direct typed Core correction only; no natural-language timestamp parser or chronology solver",
            "r4_documents_modified": false,
        })
    );
}

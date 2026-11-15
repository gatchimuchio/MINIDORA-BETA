use minidora_domain::{Guarantee, Intent, Lifecycle, Proposition, RequestEnvelope};
use minidora_engine::{
    Engine,
    language::{MeaningCompositionReport, MeaningCompositionRequest, MeaningRole},
    store::Store,
};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let serial = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "minidora-core-atomic-explanation-50-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create isolated multi-hop explanation state");
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

fn proposition(subject: &str, object: &str) -> Proposition {
    Proposition {
        subject: subject.into(),
        predicate: "is".into(),
        object: object.into(),
        negative: false,
        scope: "multi-hop-explanation-lab".into(),
    }
}

fn execute(
    engine: &mut Engine,
    actor: &str,
    case: &str,
    input_id: &str,
    text: &str,
) -> minidora_domain::ResultRecord {
    engine
        .execute(actor, RequestEnvelope::new(case, input_id, actor, text))
        .unwrap_or_else(|error| panic!("Engine::execute failed for {input_id}: {error}"))
}

#[test]
fn core_builds_and_renders_a_new_explanation_from_a_two_rule_proof() {
    let directory = TestDirectory::new();
    let actor = "b50-multi-hop-explanation";
    let seed_case = "multi-hop-proof-seed";
    let mut engine = Engine::open(directory.path()).expect("open isolated Core state");

    let first_rule = execute(
        &mut engine,
        actor,
        seed_case,
        "ore-to-refined",
        "If [scope=multi-hop-explanation-lab]X is ore then [scope=multi-hop-explanation-lab]X is refined",
    );
    let second_rule = execute(
        &mut engine,
        actor,
        seed_case,
        "refined-and-inspected-to-ready",
        "If [scope=multi-hop-explanation-lab]X is refined and inspected then [scope=multi-hop-explanation-lab]X is ready",
    );
    let ore = execute(
        &mut engine,
        actor,
        seed_case,
        "nera-is-ore",
        "[scope=multi-hop-explanation-lab]Nera is ore",
    );
    let inspected = execute(
        &mut engine,
        actor,
        seed_case,
        "nera-is-inspected",
        "[scope=multi-hop-explanation-lab]Nera is inspected",
    );
    for seed in [&first_rule, &second_rule, &ore, &inspected] {
        assert_eq!(seed.lifecycle, Lifecycle::Commit, "{seed:?}");
    }
    assert_eq!(first_rule.guarantee, Guarantee::UserPremise);
    assert_eq!(second_rule.guarantee, Guarantee::UserPremise);

    // Only a typed target proposition and the Explain purpose enter Core. No
    // finished explanation sentence or expected answer is supplied.
    let target = proposition("Nera", "ready");
    let result = engine
        .compose_atomic_explanation(
            actor,
            RequestEnvelope::new("multi-hop-explanation-result", "explain-ready", actor, ""),
            target.clone(),
        )
        .expect("execute typed multi-hop explanation through Core");
    assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
    assert_eq!(result.guarantee, Guarantee::Derived);

    let first_rule_id = first_rule.dependencies[0].clone();
    let second_rule_id = second_rule.dependencies[0].clone();
    let ore_id = ore.dependencies[0].clone();
    let inspected_id = inspected.dependencies[0].clone();
    for proof_id in [&first_rule_id, &second_rule_id, &ore_id, &inspected_id] {
        assert!(
            result.dependencies.contains(proof_id),
            "missing proof ID {proof_id}"
        );
    }

    let plan = result
        .content_plan
        .as_ref()
        .expect("Core adopted its proof-derived ContentPlan");
    assert_eq!(plan.guarantee, Guarantee::Derived);
    for proof_id in [&first_rule_id, &second_rule_id, &ore_id, &inspected_id] {
        assert!(
            plan.dependencies.contains(proof_id),
            "plan lost proof ID {proof_id}"
        );
    }
    assert!(plan.text.contains("Neraはreadyである"));
    assert!(plan.text.contains("ore"));
    assert!(plan.text.contains("refined"));
    assert!(plan.text.contains("inspected"));
    assert!(plan.text.contains(&first_rule_id));
    assert!(plan.text.contains(&second_rule_id));
    assert!(plan.text.contains("Coreが親規則から合成した短縮規則"));
    assert!(plan.text.contains("現実世界の真偽"));

    let render = result.render.as_ref().expect("K7 render observation");
    assert_eq!(render.status, "RENDERED");
    assert_eq!(render.plan_id, plan.id);
    assert_eq!(render.bytes, plan.text.as_bytes());
    assert!(result.execution_graph_ref.is_some());

    let state = engine
        .latest(actor, "multi-hop-explanation-result")
        .expect("read saved explanation case")
        .expect("explanation case exists");
    let bundle = state.bundle.as_ref().expect("typed input sealed by Core");
    assert_eq!(
        bundle.meaning.intent,
        Intent::AskWhy {
            proposition: target
        }
    );
    assert_eq!(bundle.meaning.source_kind, "CORE_TYPED_ATOMIC_MEANING_V1");

    let store = Store::open(directory.path()).expect("open scoped artifact store");
    let compute_ref = plan
        .dependencies
        .last()
        .expect("plan retains the computation artifact");
    let compute_bytes = store
        .get_bytes(actor, "multi-hop-explanation-result", compute_ref)
        .expect("read computation artifact");
    assert_eq!(minidora_domain::digest(&compute_bytes), *compute_ref);
    let compute: Value =
        serde_json::from_slice(&compute_bytes).expect("decode computation artifact");
    assert_eq!(
        compute["kind"],
        "minidora.core-computation.relation-meaning.v3"
    );
    let request: MeaningCompositionRequest =
        serde_json::from_value(compute["content_request"].clone())
            .expect("Core-authored content request");
    let composition: MeaningCompositionReport =
        serde_json::from_value(compute["content_composition"].clone())
            .expect("retained composition result");
    assert_eq!(engine.compose_typed_meaning(&request).unwrap(), composition);
    assert_eq!(composition.text, plan.text);
    assert_eq!(
        composition
            .evidence_ids
            .iter()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>(),
        result
            .dependencies
            .iter()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>()
    );
    assert!(
        composition
            .steps
            .iter()
            .any(|step| step.role == MeaningRole::Rule)
    );
    assert!(
        composition
            .steps
            .iter()
            .any(|step| step.role == MeaningRole::Evidence)
    );
    assert!(
        composition
            .steps
            .iter()
            .filter(|step| step.role == MeaningRole::Rule)
            .count()
            == 2,
        "the two declared parent rules must be shown as rules"
    );
    let memory = store.load_memory(actor).expect("read actor rule lineage");
    let composed_rule = memory
        .rules
        .iter()
        .find(|rule| rule.source_input == "derived:proof-composition-v1")
        .expect("Core proof-composition rule is retained in memory");
    assert!(composed_rule.dependencies.contains(&first_rule_id));
    assert!(composed_rule.dependencies.contains(&second_rule_id));
    let composed_detail = request
        .units
        .iter()
        .find(|unit| {
            unit.role == MeaningRole::Detail && unit.evidence_ids.contains(&composed_rule.id)
        })
        .expect("composed rule is labeled separately as Core-derived detail");
    assert!(
        composed_detail
            .text
            .contains("Coreが親規則から合成した短縮規則")
    );
    assert!(composed_detail.text.contains(&composed_rule.id));
    assert!(composed_detail.text.contains(&first_rule_id));
    assert!(composed_detail.text.contains(&second_rule_id));
    let parent_rule_units = request
        .units
        .iter()
        .filter(|unit| {
            unit.role == MeaningRole::Rule
                && (unit.evidence_ids.contains(&first_rule_id)
                    || unit.evidence_ids.contains(&second_rule_id))
        })
        .map(|unit| unit.id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(parent_rule_units.len(), 2);
    assert_eq!(
        composed_detail
            .dependencies
            .iter()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>(),
        parent_rule_units
    );

    let follow_up = engine
        .execute(
            actor,
            RequestEnvelope::new(
                "multi-hop-explanation-result",
                "explain-the-saved-explanation",
                actor,
                "Why?",
            ),
        )
        .expect("consume the retained V3 typed explanation through Core");
    assert_eq!(follow_up.lifecycle, Lifecycle::Commit, "{follow_up:?}");
    assert_eq!(follow_up.guarantee, Guarantee::Derived);
    assert!(follow_up.dependencies.contains(&result.id));
    assert_eq!(
        follow_up
            .render
            .as_ref()
            .map(|render| render.status.as_str()),
        Some("RENDERED")
    );
    assert_eq!(
        composition.steps.first().map(|step| step.role),
        Some(MeaningRole::Claim)
    );
    assert_eq!(
        composition.steps.last().map(|step| step.role),
        Some(MeaningRole::Limitation)
    );

    let observation = json!({
        "work_item": "B50 Core multi-hop atomic explanation",
        "base_route": "typed Proposition + Explain purpose -> relation proof -> Core-authored typed units -> K4 composition -> ContentPlan -> K7 render",
        "actor": actor,
        "seed_case": seed_case,
        "result_case": "multi-hop-explanation-result",
        "target": {"subject": "Nera", "predicate": "is", "object": "ready", "scope": "multi-hop-explanation-lab"},
        "proof_ids": result.dependencies,
        "root_ids": {"first_rule": first_rule_id, "second_rule": second_rule_id, "ore_fact": ore_id, "inspected_fact": inspected_id},
        "result": {
            "id": result.id,
            "lifecycle": result.lifecycle,
            "guarantee": result.guarantee,
            "text": result.text,
            "dependencies": result.dependencies,
            "execution_graph_ref": result.execution_graph_ref,
            "content_plan": {"id": plan.id, "dependencies": plan.dependencies, "required_units": plan.required_units},
            "render": {"status": render.status, "actual_sha256": render.actual_sha256, "byte_length": render.bytes.len()}
        },
        "content_request": {"purpose": request.purpose, "units": request.units},
        "composition": {
            "text": composition.text,
            "evidence_ids": composition.evidence_ids,
            "steps": composition.steps,
            "model_revision": composition.model_revision,
            "context_state_sha256": composition.context_state_sha256,
            "decoder_version": composition.decoder_version,
            "objective": composition.objective
        },
        "computation_artifact_sha256": compute_ref,
        "v3_artifact_follow_up": {
            "id": follow_up.id,
            "lifecycle": follow_up.lifecycle,
            "guarantee": follow_up.guarantee,
            "dependencies": follow_up.dependencies,
            "text": follow_up.text,
            "render_status": follow_up.render.as_ref().map(|render| render.status.as_str())
        },
        "gate_assessment": {"CORE-G03": "PARTIAL; one multi-hop typed explanation now reaches the ordinary Core content-adoption and K7 render path; general semantic adequacy, ordinary parser/CLI/HTTP, other purposes, and full R4 population remain unverified"},
        "limits": ["single declared two-rule chain", "typed Engine API rather than ordinary CLI/HTTP", "stored premises and rules are not verified against outside-world truth"]
    });
    println!(
        "B50_CORE_MULTI_HOP_EXPLANATION={}",
        serde_json::to_string(&observation).expect("serialize runtime observation")
    );
}

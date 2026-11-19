use minidora_domain::{
    ConditionDefinition, ConditionExpression, EffectPhase, GoalContract, Lifecycle, Proposition,
    RequestEnvelope, SCHEMA_VERSION,
};
use minidora_engine::{Engine, store::Store};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let serial = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "minidora-condition-evaluation-57-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create isolated condition state");
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

fn relation(subject: &str, predicate: &str, object: &str, scope: &str) -> Proposition {
    Proposition {
        subject: subject.into(),
        predicate: predicate.into(),
        object: object.into(),
        negative: false,
        scope: scope.into(),
        temporal_scope: None,
        condition_refs: BTreeSet::new(),
    }
}

fn condition_fact(id: &str, object: &str, scope: &str) -> ConditionDefinition {
    ConditionDefinition {
        condition_id: id.into(),
        subject: "$subject".into(),
        scope: scope.into(),
        expression: ConditionExpression::Fact {
            predicate: "is".into(),
            object: object.into(),
        },
        valid_from: None,
        valid_until: None,
        dependencies: BTreeSet::new(),
    }
}

fn condition_and(id: &str, children: &[&str], scope: &str) -> ConditionDefinition {
    ConditionDefinition {
        condition_id: id.into(),
        subject: "$subject".into(),
        scope: scope.into(),
        expression: ConditionExpression::And {
            operands: children
                .iter()
                .map(|condition_id| ConditionExpression::ConditionRef {
                    condition_id: (*condition_id).into(),
                })
                .collect(),
        },
        valid_from: None,
        valid_until: None,
        dependencies: children.iter().map(|id| (*id).to_owned()).collect(),
    }
}

fn condition_or(id: &str, children: &[&str], scope: &str) -> ConditionDefinition {
    ConditionDefinition {
        condition_id: id.into(),
        subject: "$subject".into(),
        scope: scope.into(),
        expression: ConditionExpression::Or {
            operands: children
                .iter()
                .map(|condition_id| ConditionExpression::ConditionRef {
                    condition_id: (*condition_id).into(),
                })
                .collect(),
        },
        valid_from: None,
        valid_until: None,
        dependencies: children.iter().map(|id| (*id).to_owned()).collect(),
    }
}

fn condition_not(id: &str, child: &str, scope: &str) -> ConditionDefinition {
    ConditionDefinition {
        condition_id: id.into(),
        subject: "$subject".into(),
        scope: scope.into(),
        expression: ConditionExpression::Not {
            operand: Box::new(ConditionExpression::ConditionRef {
                condition_id: child.into(),
            }),
        },
        valid_from: None,
        valid_until: None,
        dependencies: BTreeSet::from([child.into()]),
    }
}

fn define(engine: &mut Engine, actor: &str, case: &str, id: &str, definition: ConditionDefinition) {
    let result = engine
        .define_condition(actor, request(case, id, actor), definition)
        .expect("define a condition through Core");
    assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
}

fn assert_relation(
    engine: &mut Engine,
    actor: &str,
    case: &str,
    input: &str,
    proposition: Proposition,
) -> String {
    let result = engine
        .assert_atomic_relation(actor, request(case, input, actor), proposition)
        .expect("assert a typed relation through Core");
    assert_eq!(result.lifecycle, Lifecycle::Commit, "{result:?}");
    result.dependencies[0].clone()
}

fn explain(
    engine: &mut Engine,
    actor: &str,
    case: &str,
    input: &str,
    condition_id: &str,
    object: &str,
    scope: &str,
) -> minidora_domain::ResultRecord {
    let mut target = relation("Nera", "is", object, scope);
    target.condition_refs.insert(condition_id.into());
    engine
        .compose_atomic_explanation(actor, request(case, input, actor), target)
        .expect("explain a relation under its typed condition")
}

fn assert_stop_before_content(result: &minidora_domain::ResultRecord, code: &str) {
    assert_eq!(result.lifecycle, Lifecycle::Suspend, "{result:?}");
    assert!(
        result
            .reason
            .as_deref()
            .is_some_and(|reason| reason.starts_with(code)),
        "expected {code}, got {:?}",
        result.reason
    );
    assert!(result.content_plan.is_none());
    assert!(result.render.is_none());
}

#[test]
fn core_evaluates_condition_truth_composition_time_and_dependency_invalidation() {
    let directory = TestDirectory::new();
    let actor = "w04-condition-actor";
    let case = "w04-condition-case";
    let scope = "w04-lab";
    let dry = "condition:dry:v1";
    let wet = "condition:wet:v1";
    let absent = "condition:absent:v1";
    let disputed = "condition:disputed:v1";
    let mut engine = Engine::open(directory.path()).expect("open one Core state");

    define(
        &mut engine,
        actor,
        case,
        dry,
        condition_fact(dry, "dry", scope),
    );
    define(
        &mut engine,
        actor,
        case,
        wet,
        condition_fact(wet, "wet", scope),
    );
    define(
        &mut engine,
        actor,
        case,
        absent,
        condition_fact(absent, "raining", scope),
    );
    define(
        &mut engine,
        actor,
        case,
        disputed,
        condition_fact(disputed, "leaking", scope),
    );

    let dry_fact = assert_relation(
        &mut engine,
        actor,
        case,
        "evidence-dry",
        relation("Nera", "is", "dry", scope),
    );
    let mut wet_evidence = relation("Nera", "is", "wet", scope);
    wet_evidence.negative = true;
    assert_relation(&mut engine, actor, case, "evidence-not-wet", wet_evidence);
    assert_relation(
        &mut engine,
        actor,
        case,
        "evidence-leaking",
        relation("Nera", "is", "leaking", scope),
    );
    let mut not_leaking = relation("Nera", "is", "leaking", scope);
    not_leaking.negative = true;
    assert_relation(
        &mut engine,
        actor,
        case,
        "evidence-not-leaking",
        not_leaking,
    );

    let dry_claim = relation("Nera", "is", "ready", scope);
    let mut conditional_dry_claim = dry_claim.clone();
    conditional_dry_claim.condition_refs.insert(dry.into());
    let claim_id = assert_relation(
        &mut engine,
        actor,
        case,
        "claim-under-dry",
        conditional_dry_claim,
    );
    let dry_result = explain(&mut engine, actor, case, "explain-dry", dry, "ready", scope);
    assert_eq!(dry_result.lifecycle, Lifecycle::Commit, "{dry_result:?}");
    assert!(dry_result.dependencies.contains(&claim_id));
    assert!(dry_result.dependencies.contains(&dry_fact));
    assert!(dry_result.text.contains("TRUE"));
    assert!(dry_result.text.contains(dry));

    for (id, status, lifecycle) in [
        (wet, "CONDITION_FALSE", Lifecycle::Reject),
        (absent, "CONDITION_UNKNOWN", Lifecycle::Suspend),
        (disputed, "CONDITION_CONFLICT", Lifecycle::Suspend),
        (
            "condition:legacy-without-definition",
            "CONDITION_UNKNOWN",
            Lifecycle::Suspend,
        ),
    ] {
        let mut claim = relation("Nera", "is", "ready", scope);
        claim.condition_refs.insert(id.into());
        assert_relation(&mut engine, actor, case, &format!("claim-{id}"), claim);
        let result = explain(
            &mut engine,
            actor,
            case,
            &format!("explain-{id}"),
            id,
            "ready",
            scope,
        );
        assert_eq!(result.lifecycle, lifecycle, "{result:?}");
        assert!(
            result
                .reason
                .as_deref()
                .is_some_and(|reason| reason.starts_with(status))
        );
        assert!(result.content_plan.is_none());
        assert!(result.render.is_none());
    }

    let and_id = "condition:false-and-unknown:v1";
    let or_id = "condition:dry-or-absent:v1";
    let not_id = "condition:not-absent:v1";
    define(
        &mut engine,
        actor,
        case,
        and_id,
        condition_and(and_id, &[wet, absent], scope),
    );
    define(
        &mut engine,
        actor,
        case,
        or_id,
        condition_or(or_id, &[dry, absent], scope),
    );
    define(
        &mut engine,
        actor,
        case,
        not_id,
        condition_not(not_id, absent, scope),
    );
    for (id, expected, lifecycle) in [
        (and_id, "CONDITION_FALSE", Lifecycle::Reject),
        (or_id, "TRUE", Lifecycle::Commit),
        (not_id, "CONDITION_UNKNOWN", Lifecycle::Suspend),
    ] {
        let mut claim = relation("Nera", "is", "composite-ready", scope);
        claim.condition_refs.insert(id.into());
        assert_relation(&mut engine, actor, case, &format!("claim-{id}"), claim);
        let result = explain(
            &mut engine,
            actor,
            case,
            &format!("explain-{id}"),
            id,
            "composite-ready",
            scope,
        );
        assert_eq!(result.lifecycle, lifecycle, "{result:?}");
        if expected == "TRUE" {
            assert!(result.text.contains(expected), "{result:?}");
        } else {
            assert!(
                result
                    .reason
                    .as_deref()
                    .is_some_and(|reason| reason.starts_with(expected)),
                "{result:?}"
            );
        }
    }

    let period_id = "condition:valid-window:v1";
    let in_window = "2026-11-17T12:00:00Z";
    let mut timed_evidence = relation("Nera", "is", "available", scope);
    timed_evidence.temporal_scope = Some(in_window.into());
    assert_relation(&mut engine, actor, case, "timed-evidence", timed_evidence);
    define(
        &mut engine,
        actor,
        case,
        period_id,
        ConditionDefinition {
            condition_id: period_id.into(),
            subject: "$subject".into(),
            scope: scope.into(),
            expression: ConditionExpression::Fact {
                predicate: "is".into(),
                object: "available".into(),
            },
            valid_from: Some("2026-11-17T00:00:00Z".into()),
            valid_until: Some("2026-11-18T00:00:00Z".into()),
            dependencies: BTreeSet::new(),
        },
    );
    let mut timed_claim = relation("Nera", "is", "timed-ready", scope);
    timed_claim.temporal_scope = Some(in_window.into());
    timed_claim.condition_refs.insert(period_id.into());
    assert_relation(&mut engine, actor, case, "timed-claim", timed_claim);
    let mut in_period_target = relation("Nera", "is", "timed-ready", scope);
    in_period_target.temporal_scope = Some(in_window.into());
    in_period_target.condition_refs.insert(period_id.into());
    let in_period = engine
        .compose_atomic_explanation(
            actor,
            request(case, "explain-in-period", actor),
            in_period_target,
        )
        .expect("explain within the declared condition interval");
    assert_eq!(in_period.lifecycle, Lifecycle::Commit, "{in_period:?}");
    let mut end_boundary_target = relation("Nera", "is", "timed-ready", scope);
    end_boundary_target.temporal_scope = Some("2026-11-18T00:00:00Z".into());
    end_boundary_target.condition_refs.insert(period_id.into());
    let end_boundary = engine
        .compose_atomic_explanation(
            actor,
            request(case, "explain-at-end", actor),
            end_boundary_target,
        )
        .expect("stop at the exclusive end of the interval");
    assert_eq!(
        end_boundary.lifecycle,
        Lifecycle::Reject,
        "{end_boundary:?}"
    );
    assert!(
        end_boundary
            .reason
            .as_deref()
            .is_some_and(|reason| reason.starts_with("CONDITION_FALSE"))
    );

    // W05: the ordinary condition evaluator must also guard an isolated
    // counterfactual. A permitted hypothetical Fact can satisfy a condition,
    // but an unresolved or false condition cannot reach the relation rule.
    let raining = "condition:scenario-raining:v1";
    define(
        &mut engine,
        actor,
        case,
        raining,
        condition_fact(raining, "raining", scope),
    );
    let damp = relation("Nera", "is", "damp", scope);
    let mut conditional_damp = damp.clone();
    conditional_damp.condition_refs.insert(raining.into());
    assert_relation(
        &mut engine,
        actor,
        case,
        "conditional-damp-premise",
        conditional_damp.clone(),
    );
    let mut soaked = relation("Nera", "is", "soaked", scope);
    soaked.condition_refs.insert(raining.into());
    let rule = engine
        .add_atomic_relation_rule(
            actor,
            request(case, "conditional-soaking-rule", actor),
            vec![conditional_damp],
            soaked.clone(),
        )
        .expect("define a condition-preserving relation rule");
    assert_eq!(rule.lifecycle, Lifecycle::Commit, "{rule:?}");

    let before_counterfactual = Store::open(directory.path())
        .expect("open the state before isolated scenarios")
        .load_memory(actor)
        .expect("read actor memory before isolated scenarios");
    let rainy_condition_record = before_counterfactual
        .conditions
        .iter()
        .find(|record| record.definition.condition_id == raining)
        .expect("persist the scenario condition");
    let mut raining_assumption = relation("Nera", "is", "raining", scope);
    let scenario = engine
        .evaluate_counterfactual_relation(
            actor,
            case,
            std::slice::from_ref(&raining_assumption),
            &[],
            &soaked,
            256,
        )
        .expect("evaluate a condition-satisfying isolated scenario");
    assert_eq!(
        scenario.status,
        minidora_engine::core::CounterfactualRelationStatus::Supported,
        "{scenario:?}"
    );
    assert_eq!(
        scenario.condition_status,
        Some(minidora_domain::ConditionStatus::True)
    );
    assert!(scenario.dependencies.contains(&rainy_condition_record.id));
    assert!(scenario.dependencies.contains(&scenario.hypothesis_ids[0]));
    assert!(
        scenario
            .hypothesis_ids_used
            .contains(&scenario.hypothesis_ids[0])
    );

    raining_assumption.object = "cloudy".into();
    let unknown_scenario = engine
        .evaluate_counterfactual_relation(
            actor,
            case,
            std::slice::from_ref(&raining_assumption),
            &[],
            &soaked,
            256,
        )
        .expect("stop an isolated scenario at an unknown condition");
    assert_eq!(
        unknown_scenario.condition_status,
        Some(minidora_domain::ConditionStatus::Unknown)
    );
    assert_eq!(
        unknown_scenario.status,
        minidora_engine::core::CounterfactualRelationStatus::Unknown
    );
    assert!(unknown_scenario.facts.is_empty());
    assert!(unknown_scenario.hypothesis_ids_used.is_empty());

    let mut conflicting_condition_target = relation("Nera", "is", "contested-outcome", scope);
    conflicting_condition_target
        .condition_refs
        .insert(disputed.into());
    let conflict_scenario = engine
        .evaluate_counterfactual_relation(
            actor,
            case,
            std::slice::from_ref(&raining_assumption),
            &[],
            &conflicting_condition_target,
            256,
        )
        .expect("retain CONFLICT at the isolated scenario boundary");
    assert_eq!(
        conflict_scenario.condition_status,
        Some(minidora_domain::ConditionStatus::Conflict)
    );
    assert_eq!(
        conflict_scenario.status,
        minidora_engine::core::CounterfactualRelationStatus::Unknown
    );
    assert!(conflict_scenario.facts.is_empty());

    let mut false_condition_target = relation("Nera", "is", "wet-outcome", scope);
    false_condition_target.condition_refs.insert(wet.into());
    let false_scenario = engine
        .evaluate_counterfactual_relation(
            actor,
            case,
            std::slice::from_ref(&raining_assumption),
            &[],
            &false_condition_target,
            256,
        )
        .expect("preserve FALSE when the scenario condition blocks inference");
    assert_eq!(
        false_scenario.condition_status,
        Some(minidora_domain::ConditionStatus::False)
    );
    assert_eq!(
        false_scenario.status,
        minidora_engine::core::CounterfactualRelationStatus::Unknown
    );
    assert!(false_scenario.facts.is_empty());
    assert_eq!(
        Store::open(directory.path())
            .expect("reopen after isolated scenarios")
            .load_memory(actor)
            .expect("read actor memory after isolated scenarios"),
        before_counterfactual,
        "counterfactual condition evaluation must not persist hypotheses or invalidation"
    );

    let corrected = engine
        .correct_atomic_relation(
            actor,
            request(case, "correct-dry-evidence", actor),
            relation("Nera", "is", "dry", scope),
            relation("Nera", "is", "wet", scope),
        )
        .expect("correct the direct evidence used by the condition");
    assert_eq!(corrected.lifecycle, Lifecycle::Commit, "{corrected:?}");
    let after_correction = explain(
        &mut engine,
        actor,
        case,
        "explain-dry-after-correction",
        dry,
        "ready",
        scope,
    );
    assert_eq!(
        after_correction.lifecycle,
        Lifecycle::Reject,
        "{after_correction:?}"
    );
    assert!(
        after_correction
            .reason
            .as_deref()
            .is_some_and(|reason| reason.starts_with("CONDITION_FALSE"))
    );

    let memory = Store::open(directory.path())
        .expect("reopen the durable Core store")
        .load_memory(actor)
        .expect("load condition and Fact records");
    assert_eq!(SCHEMA_VERSION, "minidora-rust/0.7.0-alpha.11");
    assert!(
        memory
            .conditions
            .iter()
            .any(|record| record.definition.condition_id == dry)
    );
    assert!(
        memory
            .facts
            .iter()
            .any(|fact| fact.id == dry_fact && !fact.valid)
    );

    // W05: invalidating one condition definition returns the complete
    // condition-record impact set through the ordinary Core operation.
    let impact_parent = "condition:impact-parent:v1";
    define(
        &mut engine,
        actor,
        case,
        impact_parent,
        condition_and(impact_parent, &[dry, absent], scope),
    );
    let before_impact = Store::open(directory.path())
        .expect("open memory before condition impact enumeration")
        .load_memory(actor)
        .expect("load condition records before impact enumeration");
    let dry_record_id = before_impact
        .conditions
        .iter()
        .find(|record| record.definition.condition_id == dry)
        .expect("find the invalidation root condition record")
        .id
        .clone();
    let parent_record_id = before_impact
        .conditions
        .iter()
        .find(|record| record.definition.condition_id == impact_parent)
        .expect("find the dependent composite condition record")
        .id
        .clone();
    let invalidation = engine
        .execute(
            actor,
            RequestEnvelope::new(
                case,
                "invalidate-condition-definition",
                actor,
                &format!("invalidate {dry_record_id}"),
            ),
        )
        .expect("invalidate a condition definition through the normal Core intent");
    assert_eq!(
        invalidation.lifecycle,
        Lifecycle::Commit,
        "{invalidation:?}"
    );
    assert!(invalidation.dependencies.contains(&dry_record_id));
    assert!(invalidation.dependencies.contains(&parent_record_id));

    let parent_query = explain(
        &mut engine,
        actor,
        case,
        "explain-invalidated-condition-parent",
        impact_parent,
        "after-invalidation",
        scope,
    );
    assert_eq!(
        parent_query.lifecycle,
        Lifecycle::Suspend,
        "{parent_query:?}"
    );
    assert!(
        parent_query
            .reason
            .as_deref()
            .is_some_and(|reason| reason.starts_with("CONDITION_UNKNOWN"))
    );
    assert!(parent_query.content_plan.is_none());
    assert!(parent_query.render.is_none());
}

#[test]
fn conditioned_conjunctive_proofs_intersect_scope_and_keep_condition_dependencies() {
    use minidora_domain::Guarantee;

    let directory = TestDirectory::new();
    let actor = "w05-conditioned-join-actor";
    let case = "w05-conditioned-join-case";
    let scope = "w05-calibration-world";
    let subject = "Nera";
    let condition_id = "condition:calibration-authorized:v1";
    let mut engine = Engine::open(directory.path()).expect("open one Core state");

    define(
        &mut engine,
        actor,
        case,
        condition_id,
        condition_fact(condition_id, "authorized", scope),
    );
    let authorization_fact_id = assert_relation(
        &mut engine,
        actor,
        case,
        "calibration-authorization",
        relation(subject, "is", "authorized", scope),
    );
    let condition_record_id = Store::open(directory.path())
        .expect("open the condition-definition store")
        .load_memory(actor)
        .expect("load the condition definition")
        .conditions[0]
        .id
        .clone();

    let mut calibrated = relation("$x", "is", "calibrated", scope);
    calibrated.condition_refs.insert(condition_id.into());
    let mut powered = relation("$x", "is", "powered", scope);
    powered.condition_refs.insert(condition_id.into());
    let mut ready = relation("$x", "is", "ready", scope);
    ready.condition_refs.insert(condition_id.into());
    let rule = engine
        .add_atomic_relation_rule(
            actor,
            request(case, "conditioned-two-premise-rule", actor),
            vec![calibrated, powered.clone()],
            ready.clone(),
        )
        .expect("save a condition-preserving two-premise rule");
    assert_eq!(rule.lifecycle, Lifecycle::Commit, "{rule:?}");
    let rule_id = rule.dependencies[0].clone();

    let mut nera_calibrated = relation(subject, "is", "calibrated", scope);
    nera_calibrated.condition_refs.insert(condition_id.into());
    let calibrated_fact_id =
        assert_relation(&mut engine, actor, case, "nera-calibrated", nera_calibrated);
    let mut wrong_scope_powered = relation(subject, "is", "powered", "w05-other-world");
    wrong_scope_powered
        .condition_refs
        .insert(condition_id.into());
    let wrong_scope_fact_id = assert_relation(
        &mut engine,
        actor,
        case,
        "nera-powered-outside-calibration-world",
        wrong_scope_powered,
    );

    let mut target = relation(subject, "is", "ready", scope);
    target.condition_refs.insert(condition_id.into());
    let scope_disjoint = engine
        .compose_atomic_explanation(
            actor,
            request(case, "ready-with-disjoint-premise-scopes", actor),
            target.clone(),
        )
        .expect("a scope-disjoint second premise cannot complete the join");
    assert_stop_before_content(&scope_disjoint, "MISSING_PREMISES");
    assert!(
        scope_disjoint.dependencies.contains(&condition_record_id),
        "{scope_disjoint:?}"
    );
    assert!(
        scope_disjoint.dependencies.contains(&authorization_fact_id),
        "{scope_disjoint:?}"
    );

    let mut nera_powered = relation(subject, "is", "powered", scope);
    nera_powered.condition_refs.insert(condition_id.into());
    let powered_fact_id = assert_relation(&mut engine, actor, case, "nera-powered", nera_powered);
    let target_path = directory.path().join("conditioned-explanation.txt");
    let mut output_request = request(case, "ready-with-both-conditioned-premises", actor);
    output_request.output_path = Some(target_path.clone());
    output_request
        .permissions
        .write_paths
        .insert(target_path.clone());
    let supported = engine
        .compose_atomic_explanation(actor, output_request, target.clone())
        .expect("the exact-scope conjunctive proof reaches Core adoption");
    assert_eq!(supported.lifecycle, Lifecycle::Commit, "{supported:?}");
    assert_eq!(supported.guarantee, Guarantee::Derived);
    for dependency in [
        &calibrated_fact_id,
        &powered_fact_id,
        &rule_id,
        &condition_record_id,
        &authorization_fact_id,
    ] {
        assert!(
            supported.dependencies.contains(dependency),
            "the conditioned proof omitted dependency {dependency}"
        );
    }
    assert!(!supported.dependencies.contains(&wrong_scope_fact_id));
    let plan = supported
        .content_plan
        .as_ref()
        .expect("adopt the supported conditioned proof");
    for dependency in [
        &calibrated_fact_id,
        &powered_fact_id,
        &rule_id,
        &condition_record_id,
        &authorization_fact_id,
    ] {
        assert!(plan.dependencies.contains(dependency));
    }
    let state = engine.latest(actor, case).unwrap().unwrap();
    let store = Store::open(directory.path()).unwrap();
    let goal: GoalContract = serde_json::from_slice(
        &store
            .get_bytes(actor, case, state.goal_sha256.as_ref().unwrap())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        goal.required_predicates,
        [
            "RELATION_PROOF_EXPLANATION_V1",
            "SUPPORTED_CONTENT",
            "EXACT_RENDER",
            "CONFIRMED_FILE"
        ]
    );
    let compute_ref = plan.dependencies.last().unwrap();
    let compute: serde_json::Value =
        serde_json::from_slice(&store.get_bytes(actor, case, compute_ref).unwrap()).unwrap();
    let condition_unit = compute["relation_meaning_plan"]["units"]
        .as_array()
        .unwrap()
        .iter()
        .find(|unit| unit["meaning"]["kind"] == "CONDITION")
        .unwrap();
    assert_eq!(condition_unit["meaning"]["evaluation"]["status"], "TRUE");
    assert_eq!(
        condition_unit["meaning"]["evaluation"]["condition_id"],
        condition_id
    );
    let generation = &compute["content_generation"];
    assert!(
        generation["required_unit_ids"]
            .as_array()
            .unwrap()
            .contains(&condition_unit["id"])
    );
    assert!(
        generation["token_steps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|step| {
                step["unit_id"] == condition_unit["id"]
                    && !step["distribution"].as_array().unwrap().is_empty()
            })
    );
    assert_eq!(generation["text"], plan.text);
    let render = supported.render.as_ref().unwrap();
    assert_eq!(fs::read(&target_path).unwrap(), render.bytes);
    assert_eq!(supported.receipts[0].phase, EffectPhase::Confirmed);
    assert_eq!(supported.receipts[0].content_sha256, render.actual_sha256);
    let graph: serde_json::Value = serde_json::from_slice(
        &store
            .get_bytes(actor, case, supported.execution_graph_ref.as_ref().unwrap())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(graph[2]["compute"], *compute_ref);
    assert_eq!(graph[4]["render"], graph[1]["render"]);

    let mut negative_powered = relation(subject, "is", "powered", scope);
    negative_powered.negative = true;
    negative_powered.condition_refs.insert(condition_id.into());
    let negative_powered_fact_id = assert_relation(
        &mut engine,
        actor,
        case,
        "nera-not-powered",
        negative_powered,
    );
    let blocked_conclusion = engine
        .compose_atomic_explanation(
            actor,
            request(case, "ready-after-conditioned-premise-conflict", actor),
            target,
        )
        .expect("a conflicted conditioned premise cannot support the derived conclusion");
    assert_stop_before_content(&blocked_conclusion, "MISSING_PREMISES");

    let mut contested_premise = relation(subject, "is", "powered", scope);
    contested_premise.condition_refs.insert(condition_id.into());
    let contested = engine
        .compose_atomic_explanation(
            actor,
            request(case, "ask-conditioned-powered-conflict", actor),
            contested_premise,
        )
        .expect("report the contradictory conditioned premise pair");
    assert_stop_before_content(&contested, "CONFLICTING_PREMISES");
    for dependency in [
        &powered_fact_id,
        &negative_powered_fact_id,
        &condition_record_id,
        &authorization_fact_id,
    ] {
        assert!(
            contested.dependencies.contains(dependency),
            "the conflict report omitted dependency {dependency}"
        );
    }
}

#[test]
fn counterfactual_rejects_an_assumption_outside_the_target_scope_without_mutation() {
    let directory = TestDirectory::new();
    let actor = "w05-counterfactual-scope-actor";
    let case = "w05-counterfactual-scope-case";
    let scope = "w05-target-world";
    let condition_id = "condition:scope-bound:v1";
    let mut engine = Engine::open(directory.path()).expect("open one Core state");
    define(
        &mut engine,
        actor,
        case,
        condition_id,
        condition_fact(condition_id, "raining", scope),
    );

    let before = Store::open(directory.path())
        .expect("open memory before scope rejection")
        .load_memory(actor)
        .expect("read memory before scope rejection");
    let mut target = relation("Nera", "is", "soaked", scope);
    target.condition_refs.insert(condition_id.into());
    let other_scope_assumption = relation("Nera", "is", "raining", "w05-other-world");
    let rejected = engine.evaluate_counterfactual_relation(
        actor,
        case,
        std::slice::from_ref(&other_scope_assumption),
        &[],
        &target,
        128,
    );
    assert!(matches!(
        rejected,
        Err(minidora_domain::Error::Contract { code, .. })
            if code == "COUNTERFACTUAL_SCOPE_MISMATCH"
    ));
    let after = Store::open(directory.path())
        .expect("open memory after scope rejection")
        .load_memory(actor)
        .expect("read memory after scope rejection");
    assert_eq!(
        after, before,
        "a rejected scenario must not mutate actor memory"
    );
}

#[test]
fn core_adoption_stops_preserve_condition_status_evidence_and_missing_reason() {
    use minidora_domain::Guarantee;

    let directory = TestDirectory::new();
    let actor = "w06-condition-adoption-actor";
    let case = "w06-condition-adoption-case";
    let scope = "w06-adoption-world";
    let false_id = "condition:adoption-false:v1";
    let unknown_id = "condition:adoption-unknown:v1";
    let conflict_id = "condition:adoption-conflict:v1";
    let mut engine = Engine::open(directory.path()).expect("open one Core state");
    for (condition_id, object) in [
        (false_id, "dry"),
        (unknown_id, "ready"),
        (conflict_id, "leaking"),
    ] {
        define(
            &mut engine,
            actor,
            case,
            condition_id,
            condition_fact(condition_id, object, scope),
        );
    }

    let mut not_dry = relation("Nera", "is", "dry", scope);
    not_dry.negative = true;
    let false_evidence_id = assert_relation(&mut engine, actor, case, "not-dry", not_dry);
    let conflict_positive_id = assert_relation(
        &mut engine,
        actor,
        case,
        "leaking",
        relation("Nera", "is", "leaking", scope),
    );
    let mut not_leaking = relation("Nera", "is", "leaking", scope);
    not_leaking.negative = true;
    let conflict_negative_id =
        assert_relation(&mut engine, actor, case, "not-leaking", not_leaking);

    let memory = Store::open(directory.path())
        .expect("open the condition decision store")
        .load_memory(actor)
        .expect("load condition records for adoption checks");
    let record_id = |condition_id: &str| {
        memory
            .conditions
            .iter()
            .find(|record| record.definition.condition_id == condition_id)
            .expect("condition definition is retained")
            .id
            .clone()
    };
    let false_record_id = record_id(false_id);
    let unknown_record_id = record_id(unknown_id);
    let conflict_record_id = record_id(conflict_id);

    let false_result = explain(
        &mut engine,
        actor,
        case,
        "adopt-false-condition",
        false_id,
        "outcome-false",
        scope,
    );
    assert_eq!(
        false_result.lifecycle,
        Lifecycle::Reject,
        "{false_result:?}"
    );
    assert_eq!(false_result.guarantee, Guarantee::Unknown);
    assert!(
        false_result
            .reason
            .as_deref()
            .is_some_and(|reason| reason.starts_with("CONDITION_FALSE"))
    );
    assert!(false_result.dependencies.contains(&false_record_id));
    assert!(false_result.dependencies.contains(&false_evidence_id));
    assert!(false_result.content_plan.is_none());
    assert!(false_result.render.is_none());

    let unknown_result = explain(
        &mut engine,
        actor,
        case,
        "adopt-unknown-condition",
        unknown_id,
        "outcome-unknown",
        scope,
    );
    assert_eq!(
        unknown_result.lifecycle,
        Lifecycle::Suspend,
        "{unknown_result:?}"
    );
    assert_eq!(unknown_result.guarantee, Guarantee::Unknown);
    assert!(
        unknown_result
            .reason
            .as_deref()
            .is_some_and(|reason| reason.starts_with("CONDITION_UNKNOWN"))
    );
    assert!(unknown_result.dependencies.contains(&unknown_record_id));
    assert!(unknown_result.content_plan.is_none());
    assert!(unknown_result.render.is_none());

    let conflict_result = explain(
        &mut engine,
        actor,
        case,
        "adopt-conflicting-condition",
        conflict_id,
        "outcome-conflict",
        scope,
    );
    assert_eq!(
        conflict_result.lifecycle,
        Lifecycle::Suspend,
        "{conflict_result:?}"
    );
    assert_eq!(conflict_result.guarantee, Guarantee::Unknown);
    assert!(
        conflict_result
            .reason
            .as_deref()
            .is_some_and(|reason| reason.starts_with("CONDITION_CONFLICT"))
    );
    for dependency in [
        &conflict_record_id,
        &conflict_positive_id,
        &conflict_negative_id,
    ] {
        assert!(
            conflict_result.dependencies.contains(dependency),
            "the conflict adoption decision omitted {dependency}"
        );
    }
    assert!(conflict_result.content_plan.is_none());
    assert!(conflict_result.render.is_none());
}

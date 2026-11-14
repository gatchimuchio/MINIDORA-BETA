use minidora_domain::Guarantee;
use minidora_engine::{
    Engine,
    language::{
        MeaningCompositionFormat as Format, MeaningCompositionPurpose as Purpose,
        MeaningCompositionReport, MeaningCompositionRequest, MeaningCompositionStyle as Style,
        MeaningRole as Role, TypedMeaningUnit,
    },
};
use serde_json::json;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

fn unit(
    id: &str,
    role: Role,
    text: &str,
    evidence_ids: &[&str],
    dependencies: &[&str],
) -> TypedMeaningUnit {
    TypedMeaningUnit {
        id: id.into(),
        role,
        text: text.into(),
        evidence_ids: evidence_ids.iter().map(|id| (*id).into()).collect(),
        dependencies: dependencies.iter().map(|id| (*id).into()).collect(),
    }
}

fn request(
    purpose: Purpose,
    format: Format,
    units: Vec<TypedMeaningUnit>,
    prior_sequence: &[&str],
) -> MeaningCompositionRequest {
    MeaningCompositionRequest {
        purpose,
        format,
        style: Style::Neutral,
        units,
        prior_sequence: prior_sequence.iter().map(|id| (*id).into()).collect(),
        max_output_bytes: 4096,
        max_tokens: 4096,
    }
}

fn assert_units_once(request: &MeaningCompositionRequest, report: &MeaningCompositionReport) {
    for unit in &request.units {
        assert_eq!(
            report.text.matches(&unit.text).count(),
            1,
            "unit {}",
            unit.id
        );
        assert_eq!(
            report
                .steps
                .iter()
                .filter(|step| step.unit_id == unit.id)
                .count(),
            1,
            "unit {} must be represented once in the trace",
            unit.id
        );
    }
}

#[test]
fn typed_core_meaning_composer_builds_explanation_comparison_and_short_composition() {
    let serial = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "minidora-core-meaning-composition-three-{}-{serial}",
        std::process::id()
    ));
    fs::create_dir(&directory).expect("unique isolated construction state");
    let engine = Engine::open(&directory).expect("open isolated Core state");
    let actor = "b12-typed-composition";
    let case = "construction-only-no-adoption";
    let before = engine.latest(actor, case).expect("read absent case");
    assert!(before.is_none());

    let explanation_request = request(
        Purpose::Explain,
        Format::Paragraph,
        vec![
            unit(
                "weather-record",
                Role::Evidence,
                "観測票には、北側地点の降水量が十二ミリと記録されている。",
                &["packet:weather-7"],
                &["rain-claim"],
            ),
            unit(
                "closing-note",
                Role::Limitation,
                "この説明は、提示された観測票の範囲に限られる。",
                &[],
                &["weather-record", "application-rule"],
            ),
            unit(
                "rain-claim",
                Role::Claim,
                "北側地点では、対象日の降水量が確認できる。",
                &["packet:weather-7"],
                &[],
            ),
            unit(
                "application-rule",
                Role::Rule,
                "分類処理では、観測票に記載された対象日を使う。",
                &["procedure:classification"],
                &["rain-claim"],
            ),
        ],
        &[],
    );
    let explanation = engine
        .compose_typed_meaning(&explanation_request)
        .expect("compose unseen typed explanation");
    assert_units_once(&explanation_request, &explanation);
    assert_eq!(explanation.steps.len(), 4);
    assert_eq!(explanation.steps[0].role, Role::Claim);
    assert_eq!(explanation.steps[3].role, Role::Limitation);
    assert!(
        explanation.steps[1..3]
            .iter()
            .any(|step| step.role == Role::Evidence)
    );
    assert!(
        explanation.steps[1..3]
            .iter()
            .any(|step| step.role == Role::Rule)
    );
    assert!(
        explanation
            .text
            .contains("北側地点では、対象日の降水量が確認できる。")
    );
    assert!(
        explanation
            .text
            .contains("観測票には、北側地点の降水量が十二ミリと記録されている。")
    );
    assert!(
        explanation
            .text
            .contains("分類処理では、観測票に記載された対象日を使う。")
    );
    assert!(
        explanation
            .text
            .contains("この説明は、提示された観測票の範囲に限られる。")
    );
    assert!(explanation.text.contains("根拠として、") || explanation.text.contains("資料からは、"));
    assert_eq!(explanation.factual_status, "TYPED_INPUT_UNVERIFIED");
    assert_eq!(explanation.guarantee, Guarantee::Unknown);
    assert!(
        explanation
            .evidence_ids
            .contains(&"packet:weather-7".into())
    );
    assert!(
        explanation
            .evidence_ids
            .contains(&"procedure:classification".into())
    );
    assert!(
        explanation
            .steps
            .iter()
            .all(|step| !step.selected_probability.is_empty())
    );
    assert!(
        explanation
            .steps
            .iter()
            .any(|step| { step.selected_probability != "1" && step.selected_probability != "1/2" })
    );

    let mut concise_request = explanation_request.clone();
    concise_request.style = Style::Concise;
    let concise_explanation = engine
        .compose_typed_meaning(&concise_request)
        .expect("compose with explicit concise style");
    assert_units_once(&concise_request, &concise_explanation);
    assert_eq!(concise_explanation.style, Style::Concise);
    assert_ne!(
        concise_explanation.context_state_sha256,
        explanation.context_state_sha256
    );
    assert_eq!(
        concise_explanation.steps[1].admissible_continuations.len(),
        2
    );

    let comparison_request = request(
        Purpose::Compare,
        Format::Paragraph,
        vec![
            unit(
                "delta-difference",
                Role::Difference,
                "質量の変化方向は、試料Aと試料Bで逆である。",
                &["lab:delta"],
                &["sample-a", "sample-b"],
            ),
            unit(
                "sample-b",
                Role::SubjectB,
                "試料Bは測定前より三グラム軽くなった。",
                &["lab:delta"],
                &[],
            ),
            unit(
                "sample-a",
                Role::SubjectA,
                "試料Aは測定前より一グラム重くなった。",
                &["lab:delta"],
                &[],
            ),
        ],
        &[],
    );
    let comparison = engine
        .compose_typed_meaning(&comparison_request)
        .expect("compose unseen typed comparison");
    let repeated_comparison = engine
        .compose_typed_meaning(&comparison_request)
        .expect("repeat exact typed comparison");
    assert_eq!(comparison, repeated_comparison);
    assert_units_once(&comparison_request, &comparison);
    assert_eq!(
        comparison
            .steps
            .iter()
            .map(|step| step.role)
            .collect::<Vec<_>>(),
        vec![Role::SubjectA, Role::SubjectB, Role::Difference]
    );
    for text in [
        "試料Aは測定前より一グラム重くなった。",
        "試料Bは測定前より三グラム軽くなった。",
        "質量の変化方向は、試料Aと試料Bで逆である。",
    ] {
        assert!(comparison.text.contains(text), "missing typed unit {text}");
    }
    assert!(comparison.text.contains("一方、") || comparison.text.contains("これに対して、"));
    assert!(
        comparison.text.contains("この比較では、")
            || comparison.text.contains("比較に目を向けると、")
    );
    let mut changed_comparison_request = comparison_request.clone();
    changed_comparison_request.units[1].text = "試料Bは測定前より五グラム軽くなった。".into();
    let changed_comparison = engine
        .compose_typed_meaning(&changed_comparison_request)
        .expect("compose a changed semantic input");
    assert_units_once(&changed_comparison_request, &changed_comparison);
    assert!(
        changed_comparison
            .text
            .contains("試料Bは測定前より五グラム軽くなった。")
    );
    assert_ne!(
        changed_comparison.context_state_sha256,
        comparison.context_state_sha256
    );
    assert_ne!(changed_comparison.model_revision, comparison.model_revision);

    let composition_request = request(
        Purpose::ShortComposition,
        Format::Bullets,
        vec![
            unit(
                "email-field",
                Role::Body,
                "連絡先には、返信用メールアドレスを記載する。",
                &["application:field-guide"],
                &[],
            ),
            unit(
                "name-field",
                Role::Body,
                "申請書には、担当者名を記入する。",
                &["application:field-guide"],
                &[],
            ),
            unit(
                "date-field",
                Role::Body,
                "日付欄には、申請日を記入する。",
                &["application:field-guide"],
                &[],
            ),
            unit(
                "submission-check",
                Role::Conclusion,
                "記入した三項目を提出前に確認する。",
                &[],
                &["name-field", "email-field", "date-field"],
            ),
        ],
        &["name-field"],
    );
    let composition = engine
        .compose_typed_meaning(&composition_request)
        .expect("compose unseen short text");
    assert_units_once(&composition_request, &composition);
    assert_eq!(composition.steps.len(), composition_request.units.len());
    assert_eq!(composition.steps[0].unit_id, "name-field");
    assert_eq!(composition.steps[3].unit_id, "submission-check");
    assert!(
        [
            composition.steps[1].unit_id.as_str(),
            composition.steps[2].unit_id.as_str()
        ]
        .contains(&"email-field")
    );
    assert!(
        [
            composition.steps[1].unit_id.as_str(),
            composition.steps[2].unit_id.as_str()
        ]
        .contains(&"date-field")
    );
    assert_eq!(composition.steps[1].admissible_continuations.len(), 2);
    assert!(
        composition
            .text
            .starts_with("・申請書には、担当者名を記入する。")
    );
    assert!(
        composition
            .text
            .contains("連絡先には、返信用メールアドレスを記載する。")
    );
    assert!(composition.text.contains("日付欄には、申請日を記入する。"));
    assert!(
        composition
            .text
            .contains("記入した三項目を提出前に確認する。")
    );
    assert_eq!(composition.decoder_version, "finite-role-trie-exact-v1");
    assert_eq!(
        composition.conditional_distribution_version,
        "exact-rational-support-backoff-char-trie-v2"
    );
    assert_eq!(composition.transform_version, "inherited-char-ngram-v1");
    assert_eq!(
        composition.literal_fallback_version,
        "literal-raw-byte-fallback-v1"
    );
    assert_eq!(
        composition.composition_grammar_version,
        "typed-role-single-sentence-boundary-v1"
    );
    assert_eq!(composition.guarantee, Guarantee::Unknown);
    assert_eq!(composition.transform_order, 3);
    assert_eq!(composition.transform_smoothing, 1);
    assert_ne!(composition.steps[1].selected_probability, "1");
    assert!(composition.sequence_token_count <= composition_request.max_tokens);

    assert_eq!(engine.latest(actor, case).expect("recheck case"), before);
    if let Ok(path) = std::env::var("MINIDORA_B12_OBSERVATION_PATH") {
        let record = json!({
            "record_type": "MINIDORA_BB_CORE_MEANING_COMPOSITION_OBSERVATION_03_V1",
            "entry": "in-process Engine::compose_typed_meaning",
            "construction_boundary": "direct typed semantic injection; no natural-language parser, no evidence verification, no persistent adoption, and no external model",
            "case_state_before": before,
            "observations": [
                { "request": explanation_request, "report": explanation },
                { "request": concise_request, "report": concise_explanation },
                { "request": comparison_request, "report": comparison },
                { "request": changed_comparison_request, "report": changed_comparison },
                { "request": composition_request, "report": composition },
            ],
            "repeat_comparison_identical": true,
            "case_state_after": engine.latest(actor, case).expect("case remains absent"),
        });
        fs::write(
            path,
            serde_json::to_vec_pretty(&record).expect("serialize observation"),
        )
        .expect("write designated observation evidence");
    }
    drop(engine);
    fs::remove_dir_all(directory).expect("remove isolated construction state");
}

#[test]
fn typed_core_meaning_composer_rejects_missing_semantic_roles() {
    let directory = std::env::temp_dir().join(format!(
        "minidora-core-meaning-composition-invalid-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir(&directory).expect("create isolated state");
    let engine = Engine::open(&directory).expect("open isolated Core state");
    let invalid = request(
        Purpose::Explain,
        Format::Paragraph,
        vec![unit(
            "one-claim",
            Role::Claim,
            "分類の説明は作業記録を参照する。",
            &[],
            &[],
        )],
        &[],
    );
    let error = engine
        .compose_typed_meaning(&invalid)
        .expect_err("an explanation without typed evidence or a rule must stop");
    assert!(error.to_string().contains("MEANING_ROLE_COVERAGE"));

    let mut over_budget = request(
        Purpose::ShortComposition,
        Format::Paragraph,
        vec![unit(
            "brief-body",
            Role::Body,
            "短文の本文を作成する。",
            &[],
            &[],
        )],
        &[],
    );
    over_budget.max_output_bytes = 1;
    let error = engine
        .compose_typed_meaning(&over_budget)
        .expect_err("output budget must be enforced before returning generated text");
    assert!(error.to_string().contains("MEANING_BUDGET"));
    drop(engine);
    fs::remove_dir_all(directory).expect("remove isolated state");
}

//! Versioned public execution records. These records describe observable
//! contracts, not private HDS theory or a complete legacy compatibility claim.
#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

pub const SCHEMA_VERSION: &str = "minidora-rust/0.7.0-alpha.8";
pub const LEGACY_SCHEMA_VERSION: &str = "minidora-rust/0.7.0-alpha.7";
pub const CANONICALIZER: &str = "utf8-sorted-json-v1";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{code}: {message}")]
    Contract { code: String, message: String },
    #[error("IO: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON: {0}")]
    Json(#[from] serde_json::Error),
}
pub type Result<T> = std::result::Result<T, Error>;
pub fn failure(code: &str, message: impl Into<String>) -> Error {
    Error::Contract {
        code: code.into(),
        message: message.into(),
    }
}
pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn canonical<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    fn exact(value: &serde_value::Value) -> bool {
        use serde_value::Value;
        match value {
            Value::F32(_) | Value::F64(_) => false,
            Value::Seq(values) => values.iter().all(exact),
            Value::Map(values) => values.iter().all(|(key, value)| exact(key) && exact(value)),
            Value::Option(Some(value)) | Value::Newtype(value) => exact(value),
            _ => true,
        }
    }
    let projection = serde_value::to_value(value)
        .map_err(|e| failure("CANONICALIZATION_ERROR", e.to_string()))?;
    if !exact(&projection) {
        return Err(failure(
            "NONEXACT_NUMBER",
            "binary floats, NaN and infinity are forbidden; use exact strings",
        ));
    }
    // serde_json's default map uses lexical ordering. Conversion through Value
    // also orders struct/map fields; vectors remain semantically ordered.
    Ok(serde_json::to_vec(&serde_json::to_value(value)?)?)
}
pub fn identity<T: Serialize>(value: &T) -> Result<String> {
    let bytes = canonical(&(CANONICALIZER, value))?;
    Ok(digest(&bytes))
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Permissions {
    pub read_hosts: BTreeSet<String>,
    pub write_paths: BTreeSet<PathBuf>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceBudget {
    pub max_input_bytes: usize,
    pub max_evidence_bytes: usize,
    pub max_actions: u32,
    pub deadline_ms: u64,
}
impl ResourceBudget {
    /// An explicit development admission envelope, not a measured universal
    /// resource or Windows/8GB guarantee. Callers may supply tighter budgets.
    pub fn development() -> Self {
        Self {
            max_input_bytes: 256_000,
            max_evidence_bytes: 2_000_000,
            max_actions: 32,
            deadline_ms: 30_000,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestEnvelope {
    pub case_id: String,
    pub input_id: String,
    pub input_version: u64,
    pub actor_id: String,
    pub text: String,
    pub locale: String,
    pub permissions: Permissions,
    pub output_path: Option<PathBuf>,
    pub context_ref: Option<String>,
    pub budget: ResourceBudget,
}
impl RequestEnvelope {
    pub fn new(case: &str, input: &str, actor: &str, text: &str) -> Self {
        Self {
            case_id: case.into(),
            input_id: input.into(),
            input_version: 1,
            actor_id: actor.into(),
            text: text.into(),
            locale: "ja".into(),
            permissions: Permissions::default(),
            output_path: None,
            context_ref: None,
            budget: ResourceBudget::development(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Guarantee {
    Computed,
    UserPremise,
    Derived,
    SourceReport,
    Hypothesis,
    Unknown,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Proposition {
    pub subject: String,
    pub predicate: String,
    pub object: String,
    pub negative: bool,
    pub scope: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelationRule {
    pub id: String,
    pub premise: Proposition,
    pub conclusion: Proposition,
    pub source_input: String,
    pub dependencies: Vec<String>,
    pub valid: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fact {
    pub id: String,
    pub proposition: Proposition,
    pub guarantee: Guarantee,
    pub source_input: String,
    pub dependencies: Vec<String>,
    pub valid: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryState {
    pub revision: u64,
    pub facts: Vec<Fact>,
    pub rules: Vec<RelationRule>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Intent {
    Calculate {
        expression: String,
    },
    CalculateSequence {
        steps: Vec<CalculationSequenceStep>,
    },
    CorrectCalculation {
        operand_index: usize,
        replacement: String,
    },
    ExplainPrevious,
    AskPreviousGroupMean,
    AskPreviousSourceQuestion {
        query: String,
    },
    SummarizePreviousSource,
    GenerateCode {
        document: String,
    },
    ReadCode {
        document: String,
    },
    EvaluateCode {
        document: String,
    },
    VerifyCode {
        document: String,
    },
    VerifyPreviousCode {
        document: String,
    },
    FetchPreviousSearchCandidate {
        rank: u32,
    },
    CsvSelect {
        document: String,
        column: String,
    },
    CsvFilter {
        document: String,
        column: String,
        equals: String,
    },
    CsvSort {
        document: String,
        column: String,
        descending: bool,
    },
    CsvGroupCount {
        document: String,
        column: String,
    },
    CsvGroupStatistics {
        document: String,
        group_column: String,
        value_column: String,
    },
    CsvCompare {
        document: String,
        key_column: String,
    },
    CsvInnerJoin {
        document: String,
        left_key_column: String,
        right_key_column: String,
    },
    JsonCompare {
        document: String,
    },
    JsonTableCompare {
        document: String,
        key_column: String,
    },
    JsonTableInnerJoin {
        document: String,
        left_key_column: String,
        right_key_column: String,
    },
    JsonTableFilter {
        document: String,
        column: String,
        equals: String,
    },
    JsonTableSort {
        document: String,
        column: String,
        descending: bool,
    },
    JsonTableGroupCount {
        document: String,
        column: String,
    },
    JsonTableGroupStatistics {
        document: String,
        group_column: String,
        value_column: String,
    },
    JsonTableSelect {
        document: String,
        column: String,
    },
    JsonTableProject {
        document: String,
        columns: Vec<String>,
    },
    EvidenceSynthesize {
        document: String,
    },
    CsvToJson {
        document: String,
        has_header: bool,
    },
    JsonToCsv {
        document: String,
    },
    CsvStatistics {
        document: String,
        column: String,
    },
    TextReplace {
        document: String,
        old: String,
        new: String,
        count: Option<usize>,
    },
    ComposeText {
        document: String,
    },
    TextSummaryCandidates {
        document: String,
    },
    ImportText {
        document: String,
    },
    EditText {
        document: String,
    },
    LocateTextReplacement {
        document: String,
    },
    Search {
        provider: String,
        query: String,
    },
    LinearSystem {
        coefficients: Vec<Vec<String>>,
        rhs: Vec<String>,
    },
    LinearEquation {
        left: String,
        right: String,
    },
    QuadraticEquation {
        left: String,
        right: String,
    },
    Statistics {
        values: Vec<String>,
        units: Option<Vec<String>>,
    },
    NormalizeProbabilityWeights {
        weights: Vec<String>,
    },
    FiniteProbabilityStatistics {
        outcomes: Vec<String>,
        weights: Vec<String>,
    },
    FiniteEventProbability {
        outcomes: Vec<String>,
        weights: Vec<String>,
        event_values: Vec<String>,
    },
    EvaluatePolynomial {
        expression: String,
        bindings: BTreeMap<String, String>,
    },
    Symbolic {
        expression: String,
        operation: String,
        variable: String,
    },
    Assert {
        proposition: Proposition,
    },
    AddRule {
        premise: Proposition,
        conclusion: Proposition,
    },
    Ask {
        proposition: Proposition,
    },
    AskWhy {
        proposition: Proposition,
    },
    InspectPremises {
        subject: String,
    },
    Invalidate {
        id: String,
    },
    Retrieve {
        urls: Vec<String>,
        question: String,
    },
    JsonPointer {
        document: String,
        pointer: String,
    },
    JsonObjectProject {
        document: String,
        fields: Vec<String>,
    },
    JsonStatistics {
        document: String,
        pointer: String,
    },
    MeaningCompare {
        a: String,
        b: String,
    },
    Cancel,
    Unknown {
        reason: String,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSpan {
    pub start: usize,
    pub end: usize,
    pub text: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalculationSequenceStep {
    pub id: String,
    pub expression: SourceSpan,
    pub requires: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeaningIR {
    pub parser_version: String,
    pub intent: Intent,
    pub spans: Vec<SourceSpan>,
    pub unresolved: Vec<String>,
    pub source_kind: String,
    pub context_result_ref: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputBundle {
    pub request: RequestEnvelope,
    pub raw_sha256: String,
    pub context_sha256: String,
    pub fingerprint: String,
    pub meaning: MeaningIR,
    pub semantic_signature: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Lifecycle {
    Received,
    Sealed,
    Running,
    Adopted,
    Commit,
    Suspend,
    Reject,
    Fail,
    Cancel,
}
impl Lifecycle {
    pub fn permits(self, next: Self) -> bool {
        use Lifecycle::*;
        matches!(
            (self, next),
            (Received, Sealed | Reject | Fail | Cancel)
                | (Sealed, Running | Suspend | Reject | Fail | Cancel)
                | (
                    Running,
                    Running | Adopted | Suspend | Reject | Fail | Cancel
                )
                | (Adopted, Running | Commit | Suspend | Fail | Cancel)
                | (Suspend, Running | Received)
                | (Commit | Reject | Fail | Cancel, Received)
        )
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoalContract {
    pub input_fingerprint: String,
    pub intent: Intent,
    pub output_path: Option<PathBuf>,
    pub permissions: Permissions,
    pub budget: ResourceBudget,
    pub required_predicates: Vec<String>,
    pub required_graph_ref: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentPlan {
    pub id: String,
    pub text: String,
    pub locale: String,
    pub guarantee: Guarantee,
    pub required_units: Vec<String>,
    pub dependencies: Vec<String>,
    pub model_revision: String,
    pub model_artifact_ref: String,
    pub model_observation_ref: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderObservation {
    pub plan_id: String,
    pub renderer_version: String,
    pub bytes: Vec<u8>,
    pub actual_sha256: String,
    pub status: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EffectPhase {
    Proposed,
    Authorized,
    Prepared,
    Attempted,
    Confirmed,
    Failed,
    Unknown,
    Cancelled,
}
impl EffectPhase {
    pub fn permits(self, next: Self) -> bool {
        use EffectPhase::*;
        matches!(
            (self, next),
            (Proposed, Authorized | Cancelled)
                | (Authorized, Prepared | Cancelled)
                | (Prepared, Attempted | Cancelled)
                | (Attempted, Confirmed | Failed | Unknown)
                | (Unknown, Confirmed | Failed)
        )
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryReceipt {
    pub action_id: String,
    pub target: String,
    pub content_sha256: String,
    pub phase: EffectPhase,
    pub observed_bytes: Option<u64>,
    pub reason: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionRecord {
    pub case_id: String,
    pub actor_id: String,
    pub action_id: String,
    pub input_fingerprint: String,
    pub idempotency_key: String,
    pub effect_class: String,
    pub target: String,
    pub authorization_sha256: String,
    pub exact_request_sha256: String,
    pub phase: EffectPhase,
    pub receipt: Option<DeliveryReceipt>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidencePacket {
    pub id: String,
    pub mode: String,
    pub packet_kind: String,
    pub request_uri: String,
    pub final_uri: String,
    pub redirect_chain: Vec<String>,
    pub observed_at_unix_ms: u64,
    pub provider_id: String,
    pub http_status: u16,
    pub raw_body: Vec<u8>,
    pub raw_sha256: String,
    pub extracted_text: Option<String>,
    pub parser_version: String,
    pub independence_group: String,
    pub failure: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Calculation {
    pub expression: String,
    pub exact_value: String,
    pub steps: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResultRecord {
    pub id: String,
    pub input_id: String,
    pub input_version: u64,
    pub lifecycle: Lifecycle,
    pub text: String,
    pub reason: Option<String>,
    pub guarantee: Guarantee,
    pub calculation: Option<Calculation>,
    pub content_plan: Option<ContentPlan>,
    pub render: Option<RenderObservation>,
    pub receipts: Vec<DeliveryReceipt>,
    pub evidence: Vec<EvidencePacket>,
    pub dependencies: Vec<String>,
    pub execution_graph_ref: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaseRevision {
    pub schema_version: String,
    pub case_id: String,
    pub actor_id: String,
    pub revision: u64,
    pub parent_revision: Option<u64>,
    pub lifecycle: Lifecycle,
    pub goal_sha256: Option<String>,
    pub input_fingerprint: Option<String>,
    pub bundle: Option<InputBundle>,
    pub result: Option<ResultRecord>,
    pub reason: Option<String>,
    pub prior_results: BTreeMap<String, ResultRecord>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_records_preserve_sources_and_use_stable_key_order() {
        let record = BTreeMap::from([("z", "雨"), ("a", "1/3")]);
        assert_eq!(
            canonical(&record).unwrap(),
            "{\"a\":\"1/3\",\"z\":\"雨\"}".as_bytes()
        );
        assert_ne!(identity(&"Ａ").unwrap(), identity(&"A").unwrap());
    }
    #[test]
    fn nonexact_numbers_cannot_silently_become_null_or_facts() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.1] {
            assert!(canonical(&vec![Some(value)]).is_err());
        }
        assert!(canonical(&Option::<u64>::None).is_ok());
    }
    #[test]
    fn completion_and_effect_transitions_require_intermediate_observations() {
        assert!(!Lifecycle::Received.permits(Lifecycle::Commit));
        assert!(!Lifecycle::Running.permits(Lifecycle::Commit));
        assert!(Lifecycle::Adopted.permits(Lifecycle::Commit));
        assert!(!EffectPhase::Prepared.permits(EffectPhase::Confirmed));
        assert!(EffectPhase::Unknown.permits(EffectPhase::Confirmed));
        assert!(!EffectPhase::Unknown.permits(EffectPhase::Attempted));
    }
}

#[cfg(test)]
mod envelope_tests {
    use super::*;
    #[test]
    fn unrepresented_requested_effects_are_not_ignored_by_frontends() {
        let request = RequestEnvelope::new("c", "i", "a", "2+3");
        let mut wire = serde_json::to_value(&request).unwrap();
        wire["send_email"] = serde_json::json!({"to":"unknown"});
        assert!(serde_json::from_value::<RequestEnvelope>(wire).is_err());
        let mut wire = serde_json::to_value(&request).unwrap();
        wire["permissions"]["all_effects"] = serde_json::json!(true);
        assert!(serde_json::from_value::<RequestEnvelope>(wire).is_err());
    }
}

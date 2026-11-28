//! Read and select evidence within a bounded composed Goal.
use crate::{language, math, ports, store::Store};
use minidora_domain::{
    Error, EvidencePacket, IndependentRequest, InputBundle, Intent, ResourceBudget, Result,
    canonical, failure,
};
use serde::Serialize;
use std::time::Duration;

pub(crate) fn read_url(item: &IndependentRequest) -> Option<&str> {
    let Intent::Retrieve { urls, question } = item.intent.as_ref() else {
        return None;
    };
    let [url] = urls.as_slice() else {
        return None;
    };
    let text = question.trim();
    let (verb, authored_url) = text.split_once(char::is_whitespace)?;
    (authored_url.trim() == url
        && (verb == "取得"
            || verb.eq_ignore_ascii_case("fetch")
            || verb.eq_ignore_ascii_case("retrieve")))
    .then_some(url)
}

pub(crate) struct SourceReads<'a> {
    budget: std::result::Result<Option<ports::RetrievalBudget>, (String, String)>,
    bytes_remaining: usize,
    remaining_sources: usize,
    admit: &'a dyn Fn() -> Result<()>,
}

impl<'a> SourceReads<'a> {
    pub(crate) fn new(
        retrieval: Option<Result<ports::RetrievalBudget>>,
        budget: &ResourceBudget,
        remaining_sources: usize,
        admit: &'a dyn Fn() -> Result<()>,
    ) -> Result<Self> {
        let bytes_remaining = budget.max_evidence_bytes / 2;
        let budget = match retrieval.transpose() {
            Ok(value) => Ok(value.map(|mut retrieval| {
                retrieval.reserve_output(
                    Duration::from_millis((budget.deadline_ms / 4).min(1000)),
                    budget.max_evidence_bytes - bytes_remaining,
                );
                retrieval
            })),
            Err(Error::Contract { code, message }) => Err((code, message)),
            Err(error) => return Err(error),
        };
        Ok(Self {
            budget,
            bytes_remaining,
            remaining_sources,
            admit,
        })
    }

    pub(crate) fn observe(
        &mut self,
        store: &Store,
        bundle: &InputBundle,
        uri: &str,
    ) -> Result<(String, EvidencePacket)> {
        let budget = match &mut self.budget {
            Ok(Some(budget)) => budget,
            Ok(None) => {
                return Err(failure(
                    "COMPOUND_SOURCE_NOT_PLANNED",
                    "No read was declared in this Goal.",
                ));
            }
            Err((code, message)) => return Err(failure(code, message.clone())),
        };
        let allowance = self.bytes_remaining / self.remaining_sources.max(1);
        self.remaining_sources = self.remaining_sources.saturating_sub(1);
        // A failure has at most four hops plus the requested/final/group URIs.
        // Each URI may be JSON-escaped, and the retry packet is retained twice.
        const FIXED_METADATA_BYTES: usize = 2048;
        const ENCODED_ROUTE_BYTES_PER_URI_BYTE: usize = 28;
        let uri_limit = (allowance.saturating_sub(FIXED_METADATA_BYTES)
            / ENCODED_ROUTE_BYTES_PER_URI_BYTE)
            .min(2048);
        budget.bound_failure_metadata(uri_limit);
        let request = &bundle.request;
        let packet =
            ports::fetch_with_budget(uri, &request.permissions, budget, self.admit)?.observation;
        if packet.request_uri != uri {
            return Err(failure(
                "COMPOUND_SOURCE_MISMATCH",
                "The retrieved URI differs from the sealed item.",
            ));
        }
        if packet.failure.is_none() {
            ports::validate_successful_packet(&packet, &request.permissions)?;
        }
        let bytes = canonical(&packet)?;
        // Retry evidence is also carried in the result to restore the case's gate.
        let size = bytes
            .len()
            .saturating_mul(if packet.retry_after.is_some() { 2 } else { 1 });
        if size > allowance {
            return Err(failure(
                "RESOURCE_LIMIT",
                "The shared evidence budget cannot retain this HTTP observation.",
            ));
        }
        self.bytes_remaining -= size;
        let reference = store.put_bytes(&request.actor_id, &request.case_id, &bytes)?;
        Ok((reference, packet))
    }
}

#[derive(Serialize)]
pub(crate) struct SelectedJsonNumber<'a> {
    packet_ref: &'a str,
    packet_id: String,
    final_uri: String,
    raw_sha256: String,
    pointer: &'a str,
    source_start: usize,
    source_end: usize,
    source_text: String,
    pub(crate) exact_value: String,
}

pub(crate) fn select_json_number<'a>(
    store: &Store,
    bundle: &InputBundle,
    packet_ref: &'a str,
    pointer: &'a str,
) -> Result<SelectedJsonNumber<'a>> {
    let request = &bundle.request;
    let bytes = store.get_bytes(&request.actor_id, &request.case_id, packet_ref)?;
    let packet: EvidencePacket = serde_json::from_slice(&bytes)?;
    ports::validate_successful_packet(&packet, &request.permissions)?;
    let text = packet.extracted_text.as_deref().ok_or_else(|| {
        failure(
            "COMPOUND_SOURCE_MISMATCH",
            "The retained evidence has no verified UTF-8 body.",
        )
    })?;
    let document = ports::strict_json_document(text)?;
    let field = ports::json_node_at_pointer(&document, pointer)?;
    if !matches!(field.kind, ports::JsonKind::Number) {
        return Err(failure(
            "JSON_POINTER_NOT_NUMERIC",
            "The selected field is not a JSON number literal.",
        ));
    }
    let source_text = &text[field.span.clone()];
    let exact_value = language::rational_text(&math::scalar(source_text)?);
    Ok(SelectedJsonNumber {
        packet_ref,
        packet_id: packet.id,
        final_uri: packet.final_uri,
        raw_sha256: packet.raw_sha256,
        pointer,
        source_start: field.span.start,
        source_end: field.span.end,
        source_text: source_text.to_owned(),
        exact_value,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{composition, core::base_result, input};
    use minidora_domain::{
        BranchOutcome, BranchState, RequestEnvelope, RetryAfterDirective, RetryAfterObservation,
        digest, identity,
    };
    use std::collections::BTreeMap;

    const URL: &str = "https://source.example/values.json";

    fn bundle(store: &Store, case: &str) -> InputBundle {
        let mut request = RequestEnvelope::new(
            case,
            "read",
            "owner",
            &format!("- 取得 {URL}\n- json-number @{{request-1}} /a~1b/0"),
        );
        request
            .permissions
            .read_hosts
            .insert("source.example".into());
        assert!(
            store
                .register_input("owner", &request, &identity(&request).unwrap())
                .unwrap()
        );
        input::seal(&request, None).unwrap()
    }

    // This fixture represents bytes already retained by a source port. It
    // exercises scoped revalidation and selection without a network request.
    fn fixture(body: &str) -> EvidencePacket {
        let mut packet = EvidencePacket {
            id: String::new(),
            mode: "LIVE".into(),
            packet_kind: "FETCHED_BODY".into(),
            request_uri: URL.into(),
            final_uri: URL.into(),
            redirect_chain: vec![URL.into()],
            observed_at_unix_ms: 1_000,
            provider_id: "https-direct-url-v1".into(),
            http_status: 200,
            raw_body: body.as_bytes().to_vec(),
            raw_sha256: digest(body.as_bytes()),
            extracted_text: Some(body.into()),
            parser_version: "strict-utf8-identity-v1".into(),
            independence_group: URL.into(),
            failure: None,
            retry_after: None,
        };
        packet.id = ports::fetch_packet_identity(&packet).unwrap();
        packet
    }

    #[test]
    fn composed_retained_source_selects_exact_json_and_binds_calculation() {
        let temporary = tempfile::tempdir().unwrap();
        let store = Store::open(temporary.path()).unwrap();
        let prepared = bundle(&store, "original");
        let body = r#"{"a/b":[9.007199254740993e+15,"91"],"ラベル":"数値"}"#;
        let packet = fixture(body);
        let reference = store.put_artifact("owner", "original", &packet).unwrap();
        let selected = select_json_number(&store, &prepared, &reference, "/a~1b/0").unwrap();
        assert_eq!(
            &body[selected.source_start..selected.source_end],
            selected.source_text
        );
        assert_eq!(selected.source_text, "9.007199254740993e+15");
        assert_eq!(selected.raw_sha256, digest(body.as_bytes()));
        assert_eq!(selected.exact_value, "9007199254740993");

        let selected_ref = store.put_artifact("owner", "original", &selected).unwrap();
        let resolved = language::calculate_sequence_expression(
            "@{request-2}+7",
            &BTreeMap::from([(
                "request-2".into(),
                language::SequenceValue {
                    exact_value: selected.exact_value,
                    artifact_sha256: selected_ref.clone(),
                },
            )]),
        )
        .unwrap();
        assert_eq!(resolved.calculation.exact_value, "9007199254741000");
        assert_eq!(resolved.consumed[0].artifact_sha256, selected_ref);
        assert!(matches!(
            select_json_number(&store, &prepared, &reference, "/a~1b/1"),
            Err(Error::Contract { code, .. }) if code == "JSON_POINTER_NOT_NUMERIC"
        ));
        let elsewhere = bundle(&store, "elsewhere");
        assert!(select_json_number(&store, &elsewhere, &reference, "/a~1b/0").is_err());
    }

    #[test]
    fn composed_retained_source_retry_requires_its_original_observation() {
        let temporary = tempfile::tempdir().unwrap();
        let store = Store::open(temporary.path()).unwrap();
        let prepared = bundle(&store, "rate-limited");
        let Intent::IndependentRequests { items } = &prepared.meaning.intent else {
            panic!("expected sealed source item");
        };
        let mut packet = fixture("");
        packet.packet_kind = "FETCH_FAILURE".into();
        packet.http_status = 429;
        packet.extracted_text = None;
        packet.failure = Some("HTTP_RATE_LIMITED".into());
        packet.retry_after = Some(RetryAfterObservation {
            directive: RetryAfterDirective::DelaySeconds { seconds: 60 },
            received_at_unix_ms: 1000,
            not_before_unix_ms: Some(61000),
        });
        packet.id = ports::fetch_packet_identity(&packet).unwrap();
        let reference = store
            .put_artifact("owner", "rate-limited", &packet)
            .unwrap();
        let item = &items[0];
        let observation = store
            .put_artifact(
                "owner",
                "rate-limited",
                &serde_json::json!({
                    "node_id": "request-1",
                    "input_fingerprint": prepared.fingerprint,
                    "source_range": [item.source.start, item.source.end],
                    "evidence_ref": reference,
                    "state": "FAILED",
                    "reason_code": "HTTP_RATE_LIMITED",
                    "value": null,
                }),
            )
            .unwrap();
        let mut result = base_result(&prepared.request);
        result.dependencies.push(observation.clone());
        result.evidence.push(packet.clone());
        result.branch_outcomes.push(BranchOutcome {
            node_id: "request-1".into(),
            state: BranchState::Failed,
            reason_code: Some("HTTP_RATE_LIMITED".into()),
            artifact_ref: observation,
        });
        composition::verify_retry_source(&store, &prepared, &result, &packet).unwrap();
        result.dependencies.clear();
        assert!(matches!(
            composition::verify_retry_source(&store, &prepared, &result, &packet),
            Err(Error::Contract { code, .. }) if code == "RETRY_AFTER_SOURCE_INVALID"
        ));
    }
}

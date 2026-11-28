//! Read and select evidence within a bounded composed Goal.
use crate::{
    core::{read_case_changed, supports_english_output_locale},
    language, math,
    news::{self, NewsAction, NewsFetch, NewsReview, NewsWindow},
    ports::{self, NewsWebFormat, ReadObservation},
    research_goal::{self, ResearchAction},
    store::Store,
};
use minidora_domain::{
    Error, EvidencePacket, IndependentRequest, InputBundle, Intent, Permissions, ResourceBudget,
    Result, canonical, digest, failure,
};
use serde::{Deserialize, Serialize};
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
        output_reserve: Duration,
        admit: &'a dyn Fn() -> Result<()>,
    ) -> Result<Self> {
        let bytes_remaining = budget.max_evidence_bytes / 2;
        let budget = match retrieval.transpose() {
            Ok(value) => Ok(value.map(|mut retrieval| {
                retrieval
                    .reserve_output(output_reserve, budget.max_evidence_bytes - bytes_remaining);
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
        source: SourceRequest<'_>,
    ) -> Result<SourceObservation> {
        let allowance = self.bytes_remaining / self.remaining_sources.max(1);
        self.remaining_sources = self.remaining_sources.saturating_sub(1);
        match source {
            SourceRequest::Url(uri) => {
                let (reference, read) = self.fetch(
                    store,
                    bundle,
                    uri,
                    None,
                    &bundle.request.permissions,
                    allowance,
                )?;
                Ok(SourceObservation::Url(
                    reference,
                    Box::new(read.observation),
                ))
            }
            SourceRequest::News { item, action } => {
                // Reserve enough space for the finite listing/article transcript
                // before any request can yield a Retry-After observation.
                const TRANSCRIPT_RESERVE: usize = 8192;
                let mut remaining = allowance
                    .checked_sub(TRANSCRIPT_RESERVE)
                    .filter(|n| *n >= 512)
                    .ok_or_else(|| {
                        failure("RESOURCE_LIMIT", "The news evidence budget is too small.")
                    })?;
                let observation = retain_news(
                    store,
                    bundle,
                    item,
                    &action,
                    NewsWindow::new(action.period)?,
                    |uri, format, permissions| {
                        let before = self.bytes_remaining;
                        let (reference, read) =
                            self.fetch(store, bundle, uri, Some(format), permissions, remaining)?;
                        remaining -= before - self.bytes_remaining;
                        Ok(NewsFetch {
                            packet: read.observation,
                            reference,
                            redirect_wait: read.redirect_wait,
                        })
                    },
                    TRANSCRIPT_RESERVE,
                )?;
                self.bytes_remaining -= observation.proof_size;
                Ok(SourceObservation::News(observation))
            }
            SourceRequest::Research { item, action } => {
                let (outcome, consumed) = match &mut self.budget {
                    Ok(Some(budget)) => budget.with_evidence_limit(allowance, |budget| {
                        research_goal::observe_composed(
                            store,
                            bundle,
                            item,
                            action,
                            Ok(budget),
                            self.admit,
                        )
                    }),
                    Ok(None) => {
                        return Err(failure(
                            "COMPOUND_SOURCE_NOT_PLANNED",
                            "No research read was declared in this Goal.",
                        ));
                    }
                    Err((code, message)) => (
                        research_goal::observe_composed(
                            store,
                            bundle,
                            item,
                            action,
                            Err(failure(code, message.clone())),
                            self.admit,
                        ),
                        0,
                    ),
                };
                self.bytes_remaining -= consumed;
                Ok(SourceObservation::Research(outcome?))
            }
        }
    }

    fn fetch(
        &mut self,
        store: &Store,
        bundle: &InputBundle,
        uri: &str,
        format: Option<NewsWebFormat>,
        permissions: &Permissions,
        allowance: usize,
    ) -> Result<(String, ReadObservation<EvidencePacket>)> {
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
        // A failure has at most four hops plus the requested/final/group URIs.
        // Each URI may be JSON-escaped, and the retry packet is retained twice.
        const FIXED_METADATA_BYTES: usize = 2048;
        const ENCODED_ROUTE_BYTES_PER_URI_BYTE: usize = 28;
        let uri_limit = (allowance.saturating_sub(FIXED_METADATA_BYTES)
            / ENCODED_ROUTE_BYTES_PER_URI_BYTE)
            .min(2048);
        budget.bound_failure_metadata(uri_limit);
        let read = match format {
            Some(format) => {
                ports::fetch_news_with_budget(uri, format, permissions, budget, self.admit)?
            }
            None => ports::fetch_with_budget(uri, permissions, budget, self.admit)?,
        };
        let packet = &read.observation;
        if packet.request_uri != uri {
            return Err(failure(
                "COMPOUND_SOURCE_MISMATCH",
                "The retrieved URI differs from the sealed item.",
            ));
        }
        let compact_body = format.is_some() && packet.failure.is_none();
        if packet.failure.is_none() {
            match format {
                Some(format) => ports::validate_news_packet(packet, format, permissions)?,
                None => ports::validate_successful_packet(packet, permissions)?,
            }
        }
        let bytes = if compact_body {
            let mut metadata = packet.clone();
            metadata.raw_body = Vec::new();
            metadata.extracted_text = None;
            canonical(&SavedNewsBody { metadata })?
        } else {
            canonical(packet)?
        };
        let size = bytes
            .len()
            .saturating_mul(if packet.retry_after.is_some() { 2 } else { 1 })
            .saturating_add(if compact_body {
                packet.raw_body.len()
            } else {
                0
            });
        if size > allowance {
            return Err(failure(
                "RESOURCE_LIMIT",
                "The shared evidence budget cannot retain this HTTP observation.",
            ));
        }
        self.bytes_remaining -= size;
        let request = &bundle.request;
        if compact_body {
            // The verified UTF-8 body is also the complete extracted text.
            store.put_bytes(&request.actor_id, &request.case_id, &packet.raw_body)?;
        }
        let reference = store.put_bytes(&request.actor_id, &request.case_id, &bytes)?;
        Ok((reference, read))
    }
}

pub(crate) enum SourceRequest<'a> {
    Url(&'a str),
    News {
        item: &'a IndependentRequest,
        action: NewsAction<'a>,
    },
    Research {
        item: &'a IndependentRequest,
        action: ResearchAction<'a>,
    },
}

pub(crate) enum SourceObservation {
    Url(String, Box<EvidencePacket>),
    News(NewsObservation),
    Research(research_goal::Outcome),
}

pub(crate) struct NewsObservation {
    pub(crate) reference: String,
    pub(crate) report_bytes: Vec<u8>,
    pub(crate) units: Vec<String>,
    pub(crate) complete: bool,
    pub(crate) confirmed: bool,
    pub(crate) retry_evidence: Vec<EvidencePacket>,
    pub(crate) reviews: Vec<NewsReview>,
    pub(crate) read_starts: usize,
    pub(crate) raw_bytes: usize,
    proof_size: usize,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedNewsBody {
    metadata: EvidencePacket,
}

fn read_news_packet(
    store: &Store,
    bundle: &InputBundle,
    reference: &str,
) -> Result<EvidencePacket> {
    let request = &bundle.request;
    let bytes = store.get_bytes(&request.actor_id, &request.case_id, reference)?;
    if let Ok(saved) = serde_json::from_slice::<SavedNewsBody>(&bytes) {
        if saved.metadata.failure.is_some()
            || !saved.metadata.raw_body.is_empty()
            || saved.metadata.extracted_text.is_some()
            || canonical(&saved)? != bytes
        {
            return Err(invalid_news());
        }
        let mut packet = saved.metadata;
        let body = store.get_bytes(&request.actor_id, &request.case_id, &packet.raw_sha256)?;
        let text = std::str::from_utf8(&body).map_err(|_| invalid_news())?;
        packet.extracted_text = Some(text.into());
        packet.raw_body = body;
        return Ok(packet);
    }
    let packet = serde_json::from_slice::<EvidencePacket>(&bytes)?;
    if canonical(&packet)? != bytes {
        return Err(invalid_news());
    }
    Ok(packet)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NewsProof {
    input_fingerprint: String,
    source_range: (usize, usize),
    captured_at_unix_ms: i64,
    reads: Vec<NewsRead>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
enum NewsRead {
    Retained {
        reference: String,
        redirect_wait: Option<String>,
    },
    Unavailable {
        uri: String,
        code: String,
    },
}

fn invalid_news() -> Error {
    failure(
        "COMPOUND_NEWS_PROOF_INVALID",
        "The saved news observation differs from the authored item or its read evidence.",
    )
}

pub(crate) fn ordinary_news_candidate_limit(
    max_actions: u32,
    required_actions: usize,
    publisher_count: usize,
) -> Option<usize> {
    let surplus = (max_actions as usize).checked_sub(required_actions)?;
    let extra_per_publisher = surplus.checked_div(publisher_count)?;
    Some((1 + extra_per_publisher).min(news::MAX_CANDIDATES_PER_PUBLISHER))
}

pub(crate) struct NewsTranscript {
    proof: NewsProof,
    invalid_read: bool,
}

impl NewsTranscript {
    pub(crate) fn new(
        bundle: &InputBundle,
        source_range: (usize, usize),
        window: &NewsWindow,
    ) -> Self {
        Self {
            proof: NewsProof {
                input_fingerprint: bundle.fingerprint.clone(),
                source_range,
                captured_at_unix_ms: window.captured_at_unix_ms(),
                reads: Vec::new(),
            },
            invalid_read: false,
        }
    }

    pub(crate) fn observe(
        &mut self,
        store: &Store,
        bundle: &InputBundle,
        uri: &str,
        format: NewsWebFormat,
        permissions: &Permissions,
        fetched: Result<NewsFetch>,
    ) -> Result<NewsFetch> {
        match fetched {
            Ok(read) => {
                let retained = read_news_packet(store, bundle, &read.reference);
                let validation = validate_news_read(
                    uri,
                    format,
                    permissions,
                    self.proof.captured_at_unix_ms,
                    &read,
                );
                if retained.ok().as_ref() != Some(&read.packet) || validation.is_err() {
                    self.invalid_read = true;
                    return Err(invalid_news());
                }
                self.proof.reads.push(NewsRead::Retained {
                    reference: read.reference.clone(),
                    redirect_wait: read.redirect_wait.clone(),
                });
                Ok(read)
            }
            Err(error) if read_case_changed(&error) => Err(error),
            Err(Error::Contract { code, message }) => {
                self.proof.reads.push(NewsRead::Unavailable {
                    uri: uri.into(),
                    code: code.clone(),
                });
                Err(failure(&code, message))
            }
            Err(error) => Err(error),
        }
    }

    pub(crate) fn retain(
        self,
        store: &Store,
        bundle: &InputBundle,
        max_proof_bytes: usize,
    ) -> Result<(String, usize)> {
        if self.invalid_read || self.proof.input_fingerprint != bundle.fingerprint {
            return Err(invalid_news());
        }
        let bytes = canonical(&self.proof)?;
        if bytes.len() > max_proof_bytes {
            return Err(failure(
                "RESOURCE_LIMIT",
                "The news transcript exceeds its evidence budget.",
            ));
        }
        let request = &bundle.request;
        let reference = store.put_bytes(&request.actor_id, &request.case_id, &bytes)?;
        Ok((reference, bytes.len()))
    }
}

pub(crate) fn retain_news(
    store: &Store,
    bundle: &InputBundle,
    item: &IndependentRequest,
    action: &NewsAction<'_>,
    window: NewsWindow,
    mut fetch: impl FnMut(&str, NewsWebFormat, &Permissions) -> Result<NewsFetch>,
    max_proof_bytes: usize,
) -> Result<NewsObservation> {
    let source_range = (item.source.start, item.source.end);
    let mut transcript = NewsTranscript::new(bundle, source_range, &window);
    let mut observation = inspect_news(
        bundle,
        action,
        &window,
        news::MAX_CANDIDATES_PER_PUBLISHER,
        |uri, format, permissions| {
            let fetched = fetch(uri, format, permissions);
            transcript.observe(store, bundle, uri, format, permissions, fetched)
        },
    )?;
    let (reference, proof_size) = transcript.retain(store, bundle, max_proof_bytes)?;
    observation.reference = reference;
    observation.proof_size = proof_size;
    Ok(observation)
}

pub(crate) fn replay_news(
    store: &Store,
    bundle: &InputBundle,
    item: &IndependentRequest,
    action: &NewsAction<'_>,
    reference: &str,
) -> Result<NewsObservation> {
    replay_news_scope(
        store,
        bundle,
        (item.source.start, item.source.end),
        action,
        reference,
        news::MAX_CANDIDATES_PER_PUBLISHER,
    )
}

pub(crate) fn replay_news_scope(
    store: &Store,
    bundle: &InputBundle,
    source_range: (usize, usize),
    action: &NewsAction<'_>,
    reference: &str,
    candidate_limit: usize,
) -> Result<NewsObservation> {
    let request = &bundle.request;
    let bytes = store.get_bytes(&request.actor_id, &request.case_id, reference)?;
    let proof: NewsProof = serde_json::from_slice(&bytes)?;
    if proof.input_fingerprint != bundle.fingerprint
        || proof.source_range != source_range
        || proof.captured_at_unix_ms < 0
        || proof.captured_at_unix_ms > i64::MAX - 86_400_000
        || canonical(&proof)? != bytes
    {
        return Err(invalid_news());
    }
    let window = NewsWindow::at(action.period, proof.captured_at_unix_ms)?;
    let mut reads = proof.reads.into_iter();
    let mut mismatch = false;
    let outcome = inspect_news(
        bundle,
        action,
        &window,
        candidate_limit,
        |uri, format, permissions| {
            let verified = match reads.next() {
                Some(NewsRead::Retained {
                    reference,
                    redirect_wait,
                }) => (|| {
                    let packet = read_news_packet(store, bundle, &reference)?;
                    let read = NewsFetch {
                        packet,
                        reference,
                        redirect_wait,
                    };
                    validate_news_read(uri, format, permissions, proof.captured_at_unix_ms, &read)
                        .map(|()| read)
                })(),
                Some(NewsRead::Unavailable {
                    uri: expected,
                    code,
                }) if expected == uri && !code.is_empty() => {
                    return Err(failure(
                        &code,
                        "This read was unavailable in the original request.",
                    ));
                }
                _ => Err(invalid_news()),
            };
            mismatch |= verified.is_err();
            verified
        },
    )?;
    if mismatch || reads.next().is_some() {
        return Err(invalid_news());
    }
    Ok(NewsObservation {
        reference: reference.into(),
        proof_size: bytes.len(),
        ..outcome
    })
}

fn inspect_news(
    bundle: &InputBundle,
    action: &NewsAction<'_>,
    window: &NewsWindow,
    candidate_limit: usize,
    mut fetch: impl FnMut(&str, NewsWebFormat, &Permissions) -> Result<NewsFetch>,
) -> Result<NewsObservation> {
    let request = &bundle.request;
    let publishers = news::admitted_publishers(&request.permissions);
    if publishers.is_empty() || !(1..=news::MAX_CANDIDATES_PER_PUBLISHER).contains(&candidate_limit)
    {
        return Err(invalid_news());
    }
    let mut retry_evidence = Vec::new();
    let mut read_starts = 0usize;
    let mut raw_bytes = 0usize;
    let mut observe = |uri: &str, format, permissions: &Permissions| {
        let read = fetch(uri, format, permissions)?;
        read_starts = read_starts.saturating_add(read.packet.redirect_chain.len());
        raw_bytes = raw_bytes.saturating_add(read.packet.raw_body.len());
        if read.packet.retry_after.is_some() {
            retry_evidence.push(read.packet.clone());
        }
        Ok(read)
    };
    let reviews = publishers
        .into_iter()
        .map(|publisher| {
            action.review_publisher(
                publisher,
                &request.permissions,
                window,
                candidate_limit,
                &mut observe,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let report = action.report(window, &reviews);
    let english = supports_english_output_locale(&request.locale);
    let confirmed = reviews.iter().any(|review| review.citation.is_some());
    Ok(NewsObservation {
        reference: String::new(),
        report_bytes: canonical(&report)?,
        units: if confirmed {
            report.localized_units(english)
        } else {
            report.localized_unavailable(english)
        },
        complete: reviews.iter().all(|review| review.citation.is_some()),
        confirmed,
        retry_evidence,
        reviews,
        read_starts,
        raw_bytes,
        proof_size: 0,
    })
}

fn validate_news_read(
    uri: &str,
    format: NewsWebFormat,
    permissions: &Permissions,
    capture: i64,
    read: &NewsFetch,
) -> Result<()> {
    let packet = &read.packet;
    if uri != packet.request_uri
        || packet.raw_sha256 != digest(&packet.raw_body)
        || packet.observed_at_unix_ms < capture as u64
    {
        return Err(invalid_news());
    }
    if packet.failure.is_none() && read.redirect_wait.is_none() {
        // A confirmed article must pass the existing news transport validator.
        return ports::validate_news_packet(packet, format, permissions);
    }
    ports::validate_observed_route(packet, permissions)?;
    let redirect_status = (300..400).contains(&packet.http_status);
    let redirect_wait = packet.failure.as_deref() == Some(ports::REDIRECT_RETRY_DEFERRED)
        && redirect_status
        && packet.raw_body.is_empty();
    let host_wait = packet.failure.as_deref() == Some(ports::RETRY_DEFERRED)
        && redirect_status
        && packet.raw_body.is_empty()
        && packet.retry_after.is_none();
    if !ports::is_news_packet(packet)
        || packet.mode != "LIVE"
        || packet.packet_kind != "FETCH_FAILURE"
        || packet.id != ports::fetch_packet_identity(packet)?
        || packet.failure.is_none()
        || packet.extracted_text.is_some()
        || read.redirect_wait.is_some() != host_wait
        || (packet.retry_after.is_some()
            && !redirect_wait
            && !matches!(packet.http_status, 429 | 503))
        || (redirect_wait && packet.retry_after.is_none())
    {
        return Err(invalid_news());
    }
    Ok(())
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
    use crate::{composition, core::base_result, input, scheduler::Scheduler};
    use minidora_domain::{
        BranchOutcome, BranchState, ContentPlan, Guarantee, Lifecycle, RequestEnvelope,
        RetryAfterDirective, RetryAfterObservation, digest, identity,
    };
    use std::{collections::BTreeMap, path::Path};

    const URL: &str = "https://source.example/values.json";

    fn bundle(store: &Store, case: &str, destination: Option<&Path>) -> InputBundle {
        let mut request = RequestEnvelope::new(
            case,
            "read",
            "owner",
            &format!(
                "- 取得 {URL}\n- json-number @{{request-1}} /a~1b/0\n- Calculate @{{request-2}} + 7"
            ),
        );
        request
            .permissions
            .read_hosts
            .insert("source.example".into());
        if let Some(path) = destination {
            request.output_path = Some(path.to_owned());
            request.permissions.write_paths.insert(path.to_owned());
        }
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
        let prepared = bundle(&store, "original", None);
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
        let elsewhere = bundle(&store, "elsewhere", None);
        assert!(select_json_number(&store, &elsewhere, &reference, "/a~1b/0").is_err());
    }

    #[test]
    fn composed_file_delivery_replays_retained_https_number_and_calculation() {
        let temporary = tempfile::tempdir().unwrap();
        let target = temporary.path().join("result.txt");
        let store = Store::open(temporary.path()).unwrap();
        let prepared = bundle(&store, "file-replay", Some(&target));
        let request = &prepared.request;
        let Intent::IndependentRequests { items } = &prepared.meaning.intent else {
            panic!("expected sealed compound input");
        };
        let packet = fixture(r#"{"a/b":[9.007199254740993e+15]}"#);
        let packet_ref = store
            .put_artifact(&request.actor_id, &request.case_id, &packet)
            .unwrap();
        let mut graph = Scheduler::new(
            composition::nodes(items).collect(),
            request.budget.max_actions,
        )
        .unwrap();
        let mut outcome = composition::Outcome::new();
        // Return the retained fixture via the read seam. No port is invoked.
        composition::evaluate_items(
            &store,
            &prepared,
            items,
            &mut graph,
            None,
            |index, source| {
                let SourceRequest::Url(uri) = source else {
                    panic!("expected direct URL");
                };
                assert_eq!((index, uri), (0, URL));
                Ok(SourceObservation::Url(
                    packet_ref.clone(),
                    Box::new(packet.clone()),
                ))
            },
            |_, bytes| store.put_bytes(&request.actor_id, &request.case_id, bytes),
            &mut outcome,
        )
        .unwrap();
        assert!(outcome.complete());
        assert_eq!(outcome.guarantee, Guarantee::SourceReport);
        let mut result = base_result(request);
        result.lifecycle = Lifecycle::Adopted;
        result.text = outcome.units.join("\n");
        result.guarantee = outcome.guarantee;
        result.branch_outcomes = outcome.branches;
        result.dependencies = result
            .branch_outcomes
            .iter()
            .map(|branch| branch.artifact_ref.clone())
            .collect();
        assert!(result.text.ends_with(" = 9007199254741000"));
        assert_eq!(
            graph.consumed_by("request-2").unwrap()["request-1"],
            result.dependencies[0]
        );
        assert_eq!(
            graph.consumed_by("request-3").unwrap()["request-2"],
            result.dependencies[1]
        );
        let compute_ref = store
            .put_artifact(
                &request.actor_id,
                &request.case_id,
                &(
                    &result.calculation,
                    &outcome.units,
                    &result.evidence,
                    &result.dependencies,
                ),
            )
            .unwrap();
        let model = language::SequenceModel::train(&outcome.units, 2, 1).unwrap();
        let observation = model
            .realize_supported(
                std::slice::from_ref(&result.text),
                request.budget.max_evidence_bytes,
            )
            .unwrap();
        let mut dependencies = result.dependencies.clone();
        dependencies.push(compute_ref);
        let mut plan = ContentPlan {
            id: String::new(),
            text: observation.text.clone(),
            locale: request.locale.clone(),
            guarantee: result.guarantee.clone(),
            required_units: outcome.units,
            dependencies,
            model_revision: observation.model_revision.clone(),
            model_artifact_ref: store
                .put_artifact(&request.actor_id, &request.case_id, &model)
                .unwrap(),
            model_observation_ref: store
                .put_artifact(&request.actor_id, &request.case_id, &observation)
                .unwrap(),
        };
        plan.id = identity(&plan).unwrap();
        let plan_ref = store
            .put_artifact(&request.actor_id, &request.case_id, &plan)
            .unwrap();
        drop(store);
        let store = Store::open(temporary.path()).unwrap();
        let plan: ContentPlan = serde_json::from_slice(
            &store
                .get_bytes(&request.actor_id, &request.case_id, &plan_ref)
                .unwrap(),
        )
        .unwrap();
        composition::verify_delivery(&store, &prepared, &result, &plan).unwrap();
        assert!(!target.exists());

        let replacement = fixture(r#"{"a/b":[9007199254740994]}"#);
        let replacement_ref = store
            .put_artifact(&request.actor_id, &request.case_id, &replacement)
            .unwrap();
        let bytes = store
            .get_bytes(
                &request.actor_id,
                &request.case_id,
                &result.branch_outcomes[0].artifact_ref,
            )
            .unwrap();
        let mut altered_source: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        altered_source["evidence_ref"] = replacement_ref.into();
        let altered_ref = store
            .put_artifact(&request.actor_id, &request.case_id, &altered_source)
            .unwrap();
        let mut altered_result = result.clone();
        let mut altered_plan = plan.clone();
        altered_result.branch_outcomes[0].artifact_ref = altered_ref.clone();
        altered_result.dependencies[0] = altered_ref.clone();
        altered_plan.dependencies[0] = altered_ref;
        assert!(matches!(
            composition::verify_delivery(&store, &prepared, &altered_result, &altered_plan),
            Err(Error::Contract { code, .. }) if code == "RECOVERY_COMPOUND_MISMATCH"
        ));

        let mut altered_result = result;
        let mut altered_plan = plan;
        altered_result.text.push_str("\nUnverified addition.");
        altered_plan.text.clone_from(&altered_result.text);
        assert!(matches!(
            composition::verify_delivery(&store, &prepared, &altered_result, &altered_plan),
            Err(Error::Contract { code, .. }) if code == "RECOVERY_COMPOUND_MISMATCH"
        ));
        assert!(!target.exists());
    }

    #[test]
    fn composed_retained_source_retry_requires_its_original_observation() {
        let temporary = tempfile::tempdir().unwrap();
        let store = Store::open(temporary.path()).unwrap();
        let prepared = bundle(&store, "rate-limited", None);
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

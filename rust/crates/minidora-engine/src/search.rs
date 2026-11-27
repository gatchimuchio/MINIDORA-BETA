//! Public metadata search and separately authorized source-body acquisition.
//!
//! Search listings and article bodies are different evidence records. Candidate
//! snippets/abstracts are not promoted to article text. This module executes a
//! Core-selected provider; it neither chooses goals nor grants read authority.

use crate::{
    input::{PARTIAL_WEB_CANDIDATE_FOLLOWUP_PARSER_VERSION, WEB_QUESTION_PARSER_VERSION},
    ports,
    scheduler::GoalNode,
    store::{Store, VerifiedCaseHistory},
    web_answer,
};
use minidora_domain::{
    Calculation, CaseRevision, ContentPlan, EvidencePacket, GoalContract, Guarantee, InputBundle,
    Intent, Lifecycle, Permissions, ResourceBudget, Result, ResultRecord, canonical, digest,
    failure, identity,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};
use url::{Host, Url};

const SEARCH_ROWS: usize = 5;
const MAX_RETAINED_BODY_PACKETS: usize = crate::text_summary::MAX_QUESTION_SOURCE_PACKETS;
const PARTIAL_WEB_BODY_FAILURE: &str = "EVIDENCE_BYTE_LIMIT_PARTIAL_BODY";
const PARTIAL_WEB_STOP_MESSAGE: &str =
    "Web資料を使った回答を保留しました。EVIDENCE_BYTE_LIMIT_PARTIAL_BODY";
pub(crate) const SEARCH_LISTING_NODE: &str = "search-listing";
pub(crate) const SEARCH_SELECTION_KIND: &str = "search-listing-selection-v1";
pub(crate) const SEARCH_CANDIDATE_LISTING_NODE: &str = "search-candidate-listing";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SearchProvider {
    Wikipedia,
    Crossref,
    EuropePmc,
}

impl SearchProvider {
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        match name {
            "wikipedia" => Some(Self::Wikipedia),
            "crossref" => Some(Self::Crossref),
            "europepmc" => Some(Self::EuropePmc),
            _ => None,
        }
    }

    pub fn provider_id(self) -> &'static str {
        match self {
            Self::Wikipedia => "wikipedia-ja-rest-search-v1",
            Self::Crossref => "crossref-works-search-v1",
            Self::EuropePmc => "europe-pmc-rest-search-v1",
        }
    }

    pub fn host(self) -> &'static str {
        match self {
            Self::Wikipedia => "ja.wikipedia.org",
            Self::Crossref => "api.crossref.org",
            Self::EuropePmc => "www.ebi.ac.uk",
        }
    }

    fn request_uri(self, query: &str) -> Result<Url> {
        let endpoint = match self {
            Self::Wikipedia => "https://ja.wikipedia.org/w/rest.php/v1/search/page",
            Self::Crossref => "https://api.crossref.org/works",
            Self::EuropePmc => "https://www.ebi.ac.uk/europepmc/webservices/rest/search",
        };
        let mut uri = Url::parse(endpoint)
            .map_err(|_| failure("SEARCH_CONFIGURATION", "Invalid fixed provider endpoint"))?;
        let rows = SEARCH_ROWS.to_string();
        match self {
            Self::Wikipedia => {
                uri.query_pairs_mut()
                    .append_pair("q", query)
                    .append_pair("limit", &rows);
            }
            Self::Crossref => {
                uri.query_pairs_mut()
                    .append_pair("query", query)
                    .append_pair("rows", &rows);
            }
            Self::EuropePmc => {
                uri.query_pairs_mut()
                    .append_pair("query", query)
                    .append_pair("format", "json")
                    .append_pair("pageSize", &rows)
                    .append_pair("resultType", "core");
            }
        }
        Ok(uri)
    }

    pub fn search(
        self,
        query: &str,
        permissions: &Permissions,
        budget: &ResourceBudget,
    ) -> Result<SearchObservation> {
        search(self, query, permissions, budget)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchCandidate {
    pub id: String,
    /// Original provider listing position; deduplication does not renumber it.
    pub rank: u32,
    pub title: String,
    pub uri: String,
    pub provider: SearchProvider,
    pub provider_id: String,
    pub source_key: String,
    pub search_packet_id: String,
}

impl SearchCandidate {
    fn expected_identity(&self) -> Result<String> {
        identity(&(
            "search-candidate-v1",
            self.rank,
            &self.title,
            &self.uri,
            self.provider,
            &self.provider_id,
            &self.source_key,
            &self.search_packet_id,
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RejectedCandidate {
    pub rank: u32,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchObservation {
    pub provider: SearchProvider,
    pub query: String,
    pub source_fetch_packet_id: String,
    pub listing: EvidencePacket,
    pub candidates: Vec<SearchCandidate>,
    pub rejected: Vec<RejectedCandidate>,
    pub failure: Option<String>,
}

pub(crate) struct SearchPlan<'a> {
    pub primary: SearchProvider,
    pub fallback: Option<SearchProvider>,
    pub query: &'a str,
}

pub(crate) fn search_plan(intent: &Intent) -> Result<Option<SearchPlan<'_>>> {
    let (primary, fallback, query) = match intent {
        Intent::Search { provider, query } => (provider.as_str(), None, query.as_str()),
        Intent::SearchWithFallback {
            primary_provider,
            fallback_provider,
            query,
        } => (
            primary_provider.as_str(),
            Some(fallback_provider.as_str()),
            query.as_str(),
        ),
        _ => return Ok(None),
    };
    let resolve = |name| {
        SearchProvider::from_name(name).ok_or_else(|| {
            failure(
                "SEARCH_PROVIDER_UNAVAILABLE",
                "指定された検索接続がありません。",
            )
        })
    };
    let primary = resolve(primary)?;
    let fallback = fallback.map(resolve).transpose()?;
    if fallback == Some(primary) {
        return Err(failure(
            "SEARCH_FALLBACK_DUPLICATE",
            "代替検索先は最初の検索先と異なる必要があります。",
        ));
    }
    Ok(Some(SearchPlan {
        primary,
        fallback,
        query,
    }))
}

pub(crate) fn allows_fallback(observation: &SearchObservation) -> bool {
    observation.failure.as_deref() == Some("SEARCH_LIST_FETCH_FAILED")
        && matches!(
            (
                observation.listing.http_status,
                observation.listing.failure.as_deref()
            ),
            (429, Some("HTTP_RATE_LIMITED")) | (503, Some("HTTP_STATUS_FAILURE"))
        )
}

pub(crate) fn validate_failed_observation(
    observation: &SearchObservation,
    permissions: Option<&Permissions>,
) -> Result<()> {
    let listing = &observation.listing;
    let invalid = || {
        failure(
            "SEARCH_CONTEXT_INVALID",
            "The failed search differs from its observed request",
        )
    };
    if observation.failure.as_deref() != Some("SEARCH_LIST_FETCH_FAILED")
        || listing.packet_kind != "SEARCH_LIST"
        || listing.mode != "LIVE"
        || listing.failure.is_none()
        || listing.extracted_text.is_some()
        || listing.parser_version != "strict-utf8-identity-v1"
        || listing.independence_group != listing.final_uri
    {
        return Err(invalid());
    }
    if let Some(permissions) = permissions {
        ports::validate_observed_route(listing, permissions)?;
    }
    let mut fetched = listing.clone();
    fetched.packet_kind = "FETCH_FAILURE".into();
    fetched.provider_id = "https-direct-url-v1".into();
    fetched.id = ports::fetch_packet_identity(&fetched)?;
    if parse_listing(observation.provider, &observation.query, fetched)? != *observation {
        return Err(invalid());
    }
    Ok(())
}

pub fn search(
    provider: SearchProvider,
    query: &str,
    permissions: &Permissions,
    budget: &ResourceBudget,
) -> Result<SearchObservation> {
    search_with_budget(
        provider,
        query,
        permissions,
        budget,
        &mut ports::RetrievalBudget::standalone(budget)?,
        &|| Ok(()),
    )
    .map(|read| read.observation)
}

pub(crate) fn search_with_budget(
    provider: SearchProvider,
    query: &str,
    permissions: &Permissions,
    budget: &ResourceBudget,
    retrieval: &mut ports::RetrievalBudget,
    check_admission: &dyn Fn() -> Result<()>,
) -> Result<ports::ReadObservation<SearchObservation>> {
    if query.trim().is_empty()
        || query.chars().count() > 512
        || query.len() > budget.max_input_bytes
        || query.chars().any(char::is_control)
    {
        return Err(failure(
            "INVALID_SEARCH_QUERY",
            "The provider adapter requires a nonempty, bounded single-line query",
        ));
    }
    if budget.max_actions == 0 {
        return Err(failure(
            "ACTION_BUDGET_EXHAUSTED",
            "A search observation needs an authorized action budget",
        ));
    }
    let uri = provider.request_uri(query)?;
    let read = ports::fetch_with_budget(uri.as_str(), permissions, retrieval, check_admission)?;
    Ok(ports::ReadObservation {
        observation: parse_listing(provider, query, read.observation)?,
        redirect_wait: read.redirect_wait,
    })
}

fn source_uri(raw: &str) -> Result<String> {
    let uri = Url::parse(raw).map_err(|_| {
        failure(
            "INVALID_SOURCE_URI",
            "Provider returned a malformed source URI",
        )
    })?;
    let authority = raw
        .split_once("://")
        .map(|(_, tail)| tail.split(['/', '?', '#']).next().unwrap_or(""))
        .unwrap_or("");
    let host = match uri.host() {
        Some(Host::Domain(host)) => host,
        _ => {
            return Err(failure(
                "INVALID_SOURCE_URI",
                "A source URI must use a domain hostname",
            ));
        }
    };
    if uri.scheme() != "https"
        || uri.port_or_known_default() != Some(443)
        || authority.contains('@')
        || !uri.username().is_empty()
        || uri.password().is_some()
        || uri.fragment().is_some()
        || !host.contains('.')
        || host.ends_with('.')
        || [
            ".localhost",
            ".local",
            ".localdomain",
            ".internal",
            ".lan",
            ".home",
            ".onion",
        ]
        .iter()
        .any(|suffix| host.ends_with(suffix))
    {
        return Err(failure(
            "INVALID_SOURCE_URI",
            "Only unambiguous public HTTPS source URIs are supported",
        ));
    }
    Ok(uri.to_string())
}

fn field<'a>(row: &'a Value, name: &str) -> Result<&'a str> {
    row.get(name)
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .ok_or_else(|| {
            failure(
                "MALFORMED_SEARCH_CANDIDATE",
                format!("Missing textual provider field {name}"),
            )
        })
}

fn article_route(base: &str, segments: &[&str]) -> Result<String> {
    let mut uri = Url::parse(base)
        .map_err(|_| failure("SEARCH_CONFIGURATION", "Invalid fixed article route"))?;
    uri.path_segments_mut()
        .map_err(|_| failure("SEARCH_CONFIGURATION", "Invalid article route base"))?
        .extend(segments.iter().copied());
    source_uri(uri.as_str())
}

fn parse_candidate(provider: SearchProvider, row: &Value) -> Result<(String, String, String)> {
    match provider {
        SearchProvider::Wikipedia => {
            let title = field(row, "title")?;
            let key = field(row, "key")?;
            let uri = article_route(
                "https://ja.wikipedia.org/",
                &["wiki", &key.replace(' ', "_")],
            )?;
            Ok((title.into(), uri, key.into()))
        }
        SearchProvider::Crossref => {
            let title = row
                .get("title")
                .and_then(Value::as_array)
                .and_then(|values| values.first())
                .and_then(Value::as_str)
                .filter(|text| !text.trim().is_empty())
                .ok_or_else(|| {
                    failure("MALFORMED_SEARCH_CANDIDATE", "Missing first Crossref title")
                })?;
            let uri = source_uri(field(row, "URL")?)?;
            let key = field(row, "DOI")?;
            Ok((title.into(), uri, key.to_ascii_lowercase()))
        }
        SearchProvider::EuropePmc => {
            let title = field(row, "title")?;
            let source = field(row, "source")?;
            let id = field(row, "id")?;
            if !source.bytes().all(|b| b.is_ascii_alphanumeric())
                || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            {
                return Err(failure(
                    "MALFORMED_SEARCH_CANDIDATE",
                    "Invalid Europe PMC source identity",
                ));
            }
            let actual_url = row
                .pointer("/fullTextUrlList/fullTextUrl")
                .and_then(Value::as_array)
                .and_then(|links| {
                    links
                        .iter()
                        .filter_map(|link| link.get("url").and_then(Value::as_str))
                        .find_map(|uri| source_uri(uri).ok())
                });
            let uri = match actual_url {
                Some(uri) => uri,
                None => article_route("https://europepmc.org/", &["article", source, id])?,
            };
            Ok((title.into(), uri, format!("{source}:{id}")))
        }
    }
}

/// Parse only the actual JSON listing. FIXED_PACKET callers can use this same
/// parser for fixture/replay comparisons; its mode is retained without relabeling.
pub fn parse_listing(
    provider: SearchProvider,
    query: &str,
    mut listing: EvidencePacket,
) -> Result<SearchObservation> {
    let source_fetch_packet_id = listing.id.clone();
    listing.packet_kind = "SEARCH_LIST".into();
    listing.provider_id = provider.provider_id().into();
    listing.id = identity(&("search-list-v1", provider, query, &source_fetch_packet_id))?;
    // Listing text, including snippets and abstracts, cannot masquerade as a
    // fetched article body. Exact original JSON remains in raw_body unchanged.
    listing.extracted_text = None;
    let mut observation = SearchObservation {
        provider,
        query: query.into(),
        source_fetch_packet_id,
        listing,
        candidates: Vec::new(),
        rejected: Vec::new(),
        failure: None,
    };
    let expected_request = provider.request_uri(query)?;
    if observation.listing.raw_sha256 != digest(&observation.listing.raw_body)
        || observation.listing.request_uri != expected_request.as_str()
        || !matches!(observation.listing.mode.as_str(), "LIVE" | "FIXED_PACKET")
    {
        observation.failure = Some("SEARCH_LIST_IDENTITY_MISMATCH".into());
        return Ok(observation);
    }
    if observation.listing.failure.is_some()
        || observation.listing.retry_after.is_some()
        || observation.listing.http_status != 200
    {
        observation.failure = Some("SEARCH_LIST_FETCH_FAILED".into());
        return Ok(observation);
    }
    let parse = (|| -> Result<Vec<Value>> {
        let raw = std::str::from_utf8(&observation.listing.raw_body)
            .map_err(|_| failure("MALFORMED_SEARCH_LIST", "Listing is not UTF-8 JSON"))?;
        // The source-preserving parser rejects decoded duplicate keys and invalid
        // literals before serde_json projects the provider's metadata fields.
        ports::json_pointer(raw, "")?;
        let value: Value = serde_json::from_str(raw)?;
        let path = match provider {
            SearchProvider::Wikipedia => "/pages",
            SearchProvider::Crossref => "/message/items",
            SearchProvider::EuropePmc => "/resultList/result",
        };
        let rows = value
            .pointer(path)
            .and_then(Value::as_array)
            .ok_or_else(|| {
                failure(
                    "MALFORMED_SEARCH_LIST",
                    "Provider listing shape does not match its contract",
                )
            })?;
        if rows.len() > SEARCH_ROWS {
            return Err(failure(
                "MALFORMED_SEARCH_LIST",
                "Provider exceeded the explicitly requested row limit",
            ));
        }
        Ok(rows.clone())
    })();
    let rows = match parse {
        Ok(rows) => rows,
        Err(_) => {
            observation.failure = Some("MALFORMED_SEARCH_LIST".into());
            return Ok(observation);
        }
    };
    let mut seen_keys = BTreeSet::new();
    let mut seen_uris = BTreeSet::new();
    for (position, row) in rows.iter().enumerate() {
        let rank = (position + 1) as u32;
        let (title, uri, key) = match parse_candidate(provider, row) {
            Ok(value) => value,
            Err(_) => {
                observation.rejected.push(RejectedCandidate {
                    rank,
                    reason: "MALFORMED_OR_UNSUPPORTED_SOURCE_METADATA".into(),
                });
                continue;
            }
        };
        if title.chars().count() > 2048 || title.chars().any(char::is_control) {
            observation.rejected.push(RejectedCandidate {
                rank,
                reason: "TITLE_ADMISSION_LIMIT".into(),
            });
            continue;
        }
        if !seen_keys.insert(key.clone()) || !seen_uris.insert(uri.clone()) {
            observation.rejected.push(RejectedCandidate {
                rank,
                reason: "DUPLICATE_SOURCE".into(),
            });
            continue;
        }
        let mut candidate = SearchCandidate {
            id: String::new(),
            rank,
            title,
            uri,
            provider,
            provider_id: provider.provider_id().into(),
            source_key: key,
            search_packet_id: observation.listing.id.clone(),
        };
        candidate.id = candidate.expected_identity()?;
        observation.candidates.push(candidate);
    }
    Ok(observation)
}

pub(crate) fn listing_lines(observation: &SearchObservation) -> Vec<String> {
    let mut units =
        vec!["検索一覧（本文は未取得。題名・URLは本文の事実根拠ではありません）:".into()];
    units.extend(
        observation
            .candidates
            .iter()
            .map(|candidate| format!("{}: {}\n{}", candidate.rank, candidate.title, candidate.uri)),
    );
    if observation.candidates.is_empty() {
        units.push("検索候補がありません。".into());
    }
    units
}

pub(crate) fn selected_listing_lines(
    selected: &SearchObservation,
    failed: Option<&SearchObservation>,
    explicit_fallback: bool,
) -> Vec<String> {
    let mut units = Vec::new();
    if let Some(failed) = failed {
        units.push(format!(
            "最初の検索先 {} はHTTP {}で一覧を取得できませんでした。",
            failed.provider.provider_id(),
            failed.listing.http_status
        ));
    }
    if explicit_fallback {
        units.push(format!(
            "採用した検索先: {}",
            selected.provider.provider_id()
        ));
    }
    units.extend(listing_lines(selected));
    units
}

/// Permissions are supplied when replaying the original search. A later
/// candidate request consumes the already authorized listing as case evidence.
pub(crate) fn validate_stored_observation(
    observation: &SearchObservation,
    permissions: Option<&Permissions>,
) -> Result<()> {
    let listing = &observation.listing;
    if observation.failure.is_some()
        || listing.packet_kind != "SEARCH_LIST"
        || listing.provider_id != observation.provider.provider_id()
        || listing.mode != "LIVE"
        || listing.failure.is_some()
        || listing.retry_after.is_some()
        || listing.http_status != 200
        || listing.extracted_text.is_some()
        || listing.parser_version != "strict-utf8-identity-v1"
        || listing.raw_sha256 != digest(&listing.raw_body)
        || listing.request_uri
            != observation
                .provider
                .request_uri(&observation.query)?
                .as_str()
    {
        return Err(failure(
            "SEARCH_CONTEXT_INVALID",
            "The retained live search listing does not match its provider contract",
        ));
    }
    let source_packet_id = identity(&(
        &listing.mode,
        "FETCHED_BODY",
        &listing.request_uri,
        &listing.final_uri,
        listing.http_status,
        &listing.raw_sha256,
        &listing.parser_version,
        &listing.independence_group,
        &listing.failure,
        &listing.redirect_chain,
        "https-direct-url-v1",
    ))?;
    if source_packet_id != observation.source_fetch_packet_id {
        return Err(failure(
            "SEARCH_CONTEXT_INVALID",
            "The retained search listing no longer matches its original fetched packet identity",
        ));
    }
    let mut fetched_packet = listing.clone();
    fetched_packet.id = observation.source_fetch_packet_id.clone();
    fetched_packet.packet_kind = "FETCHED_BODY".into();
    fetched_packet.provider_id = "https-direct-url-v1".into();
    fetched_packet.extracted_text = Some(
        std::str::from_utf8(&listing.raw_body)
            .map_err(|_| failure("SEARCH_CONTEXT_INVALID", "The listing is not UTF-8"))?
            .into(),
    );
    if let Some(permissions) = permissions {
        ports::validate_successful_packet(&fetched_packet, permissions)?;
    }
    if parse_listing(observation.provider, &observation.query, fetched_packet)? != *observation {
        return Err(failure(
            "SEARCH_CONTEXT_INVALID",
            "The retained candidates differ from the exact saved search listing",
        ));
    }
    Ok(())
}

pub(crate) struct RetainedSearchListing {
    pub(crate) proof_ref: String,
    selected_ref: String,
    selected: SearchObservation,
    rejected_primary: Option<SearchObservation>,
}

/// Replay the original list selection against its sealed providers, permissions and budget.
pub(crate) fn verify_retained_search_listing(
    store: &Store,
    bundle: &InputBundle,
    result: &ResultRecord,
    plan: &ContentPlan,
) -> Result<Option<RetainedSearchListing>> {
    let Some(search_plan) = search_plan(&bundle.meaning.intent)? else {
        return Ok(None);
    };
    let invalid = || {
        failure(
            "RECOVERY_SEARCH_CONTEXT_MISMATCH",
            "the retained searches or adopted report differ from the original search order",
        )
    };
    let request = &bundle.request;
    let actor = &request.actor_id;
    let case = &request.case_id;
    let count = result.dependencies.len();
    if count == 0
        || count > 1 + usize::from(search_plan.fallback.is_some())
        || count != result.evidence.len()
        || result.id != identity(request)?
        || result.input_id != request.input_id
        || result.input_version != request.input_version
        || plan.guarantee != result.guarantee
        || plan.locale != request.locale
        || result.text != plan.text
        || result.text != plan.required_units.join("\n")
        || result.calculation.is_some()
        || result.guarantee != Guarantee::SourceReport
    {
        return Err(invalid());
    }
    let mut failed = None;
    let mut raw_bytes = 0usize;
    let mut starts = 0usize;
    for (index, (reference, packet)) in result.dependencies.iter().zip(&result.evidence).enumerate()
    {
        let bytes = store.get_bytes(actor, case, reference)?;
        let observation: SearchObservation = serde_json::from_slice(&bytes)?;
        let expected_provider = if index == 0 {
            Some(search_plan.primary)
        } else {
            search_plan.fallback
        };
        if expected_provider != Some(observation.provider)
            || search_plan.query != observation.query
            || observation.listing != *packet
            || canonical(&observation)? != bytes
        {
            return Err(invalid());
        }
        raw_bytes = raw_bytes
            .checked_add(packet.raw_body.len())
            .ok_or_else(invalid)?;
        starts = starts
            .checked_add(packet.redirect_chain.len())
            .ok_or_else(invalid)?;
        if raw_bytes > request.budget.max_evidence_bytes
            || (search_plan.fallback.is_some()
                && (packet.redirect_chain.len() > crate::ports::MAX_HTTP_STARTS_PER_SOURCE
                    || starts + 3 + usize::from(request.output_path.is_some())
                        > request.budget.max_actions as usize))
        {
            return Err(invalid());
        }
        if index + 1 < count {
            if !allows_fallback(&observation) {
                return Err(invalid());
            }
            validate_failed_observation(&observation, Some(&request.permissions))?;
            failed = Some(observation);
            continue;
        }
        validate_stored_observation(&observation, Some(&request.permissions))?;
        if plan.required_units
            != selected_listing_lines(
                &observation,
                failed.as_ref(),
                search_plan.fallback.is_some(),
            )
        {
            return Err(invalid());
        }
        let evidence = if search_plan.fallback.is_some() {
            let selection = canonical(&(SEARCH_SELECTION_KIND, &result.dependencies))?;
            let reference = digest(&selection);
            if store.get_bytes(actor, case, &reference)? != selection {
                return Err(invalid());
            }
            reference
        } else {
            reference.clone()
        };
        return Ok(Some(RetainedSearchListing {
            proof_ref: evidence,
            selected_ref: reference.clone(),
            selected: observation,
            rejected_primary: failed,
        }));
    }
    Err(invalid())
}

pub(crate) struct SearchExplanation {
    pub(crate) units: Vec<String>,
    pub(crate) dependencies: Vec<String>,
    pub(crate) selected_body_index: Option<usize>,
}

pub(crate) fn previous_search_explanation(
    store: &Store,
    bundle: &InputBundle,
    prior_results: &BTreeMap<String, ResultRecord>,
    previous: &ResultRecord,
) -> Result<Option<SearchExplanation>> {
    if !matches!(bundle.meaning.intent, Intent::ExplainPrevious)
        || previous.guarantee != Guarantee::SourceReport
    {
        return Ok(None);
    }
    let request = &bundle.request;
    store.with_verified_case_history(&request.actor_id, &request.case_id, |history| {
        previous_search_explanation_in_history(store, history, bundle, prior_results, previous)
    })
}

pub(crate) fn previous_search_explanation_in_history(
    store: &Store,
    history: &VerifiedCaseHistory<'_>,
    bundle: &InputBundle,
    prior_results: &BTreeMap<String, ResultRecord>,
    previous: &ResultRecord,
) -> Result<Option<SearchExplanation>> {
    if !matches!(bundle.meaning.intent, Intent::ExplainPrevious)
        || previous.guarantee != Guarantee::SourceReport
    {
        return Ok(None);
    }
    let request = &bundle.request;
    let actor = &request.actor_id;
    let case = &request.case_id;
    let invalid = || {
        failure(
            "SEARCH_LISTING_EXPLANATION_CONTEXT_INVALID",
            "the explanation does not match the retained original search listing",
        )
    };
    let Some((_, original)) = history.input_bundle(&previous.input_id, previous.input_version)?
    else {
        if previous
            .evidence
            .iter()
            .any(|packet| packet.packet_kind == "SEARCH_LIST")
            || previous.dependencies.first().is_some_and(|reference| {
                prior_results.contains_key(reference)
                    && previous
                        .evidence
                        .iter()
                        .any(|packet| packet.packet_kind == "FETCHED_BODY")
            })
        {
            return Err(invalid());
        }
        return Ok(None);
    };
    if bundle.meaning.parser_version == crate::input::WEB_ANSWER_EXPLANATION_PARSER_VERSION {
        return explain_web_answer(store, history, bundle, previous, &original).map(Some);
    }
    match original.meaning.intent {
        Intent::Search { .. } | Intent::SearchWithFallback { .. } => {}
        Intent::FetchPreviousSearchCandidate { rank } => {
            return explain_candidate_body(
                store,
                history,
                bundle,
                prior_results,
                previous,
                &original,
                rank,
            )
            .map(Some);
        }
        _ => return Ok(None),
    }
    validate_search_predecessor(
        bundle,
        previous,
        "SEARCH_LISTING_EXPLANATION_CONTEXT_INVALID",
    )?;
    let plan = previous.content_plan.as_ref().ok_or_else(invalid)?;
    let proof =
        verify_retained_search_listing(store, &original, previous, plan)?.ok_or_else(invalid)?;
    crate::document::revalidate_previous_document_render(
        store,
        actor,
        case,
        previous,
        None,
        "SEARCH_LISTING_EXPLANATION_CONTEXT_INVALID",
        "search listing",
    )?;

    let mut dependencies = vec![previous.id.clone()];
    if !matches!(
        bundle.meaning.parser_version.as_str(),
        crate::input::SEARCH_LISTING_EXPLANATION_PARSER_VERSION
            | crate::input::SEARCH_CANDIDATE_BODY_EXPLANATION_PARSER_VERSION
    ) {
        return Ok(Some(SearchExplanation {
            units: vec![previous.text.clone()],
            dependencies,
            selected_body_index: None,
        }));
    }

    let selected = proof.selected;
    let listing = &selected.listing;
    let mut units = vec![
        format!("前回の検索一覧の根拠（検索結果ID: {}）。", previous.id),
        format!("検索語: {:?}", selected.query),
    ];
    if let Some(failed) = proof.rejected_primary {
        units.push(format!(
            "最初の検索先: {} / HTTP {} / {} / 一覧packet ID: {}",
            failed.provider.provider_id(),
            failed.listing.http_status,
            failed
                .listing
                .failure
                .as_deref()
                .unwrap_or("SEARCH_LIST_FETCH_FAILED"),
            failed.listing.id
        ));
        units.push("元の依頼で指定された代替検索先の一覧を採用しました。".into());
    } else {
        units.push("元の依頼で最初に指定された検索先の一覧を採用しました。".into());
    }
    units.extend([
        format!(
            "採用した検索先: {} / 観測モード: {} / 観測時刻（Unix ms）: {}",
            selected.provider.provider_id(),
            listing.mode,
            listing.observed_at_unix_ms
        ),
        format!(
            "一覧取得URL: {} / 最終URL: {}",
            listing.request_uri, listing.final_uri
        ),
        format!(
            "一覧packet ID: {} / 原応答SHA-256: {} / 読み取り方式: {}",
            listing.id, listing.raw_sha256, listing.parser_version
        ),
        format!("選択記録SHA-256: {}", proof.proof_ref),
    ]);
    if selected.candidates.is_empty() {
        units.push("検索候補がありません。".into());
    }
    for candidate in selected.candidates {
        units.push(format!(
            "掲載順位 {} / 題名: {:?} / URL: {} / 候補ID: {}",
            candidate.rank, candidate.title, candidate.uri, candidate.id
        ));
    }
    for rejected in selected.rejected {
        units.push(format!(
            "掲載順位 {} は除外 / 理由: {}",
            rejected.rank, rejected.reason
        ));
    }
    units.push("掲載順位は検索先の元の位置です。候補本文は未取得です。一覧からは本文の内容や事実性、検索の網羅性を確認できません。".into());
    if units.join("\n").len() > request.budget.max_evidence_bytes {
        return Err(failure(
            "RESOURCE_LIMIT",
            "the search listing explanation exceeds its evidence budget",
        ));
    }
    dependencies.extend(previous.dependencies.iter().cloned());
    Ok(Some(SearchExplanation {
        units,
        dependencies,
        selected_body_index: None,
    }))
}

fn validate_search_predecessor(
    bundle: &InputBundle,
    previous: &ResultRecord,
    code: &str,
) -> Result<()> {
    let eligible = if bundle.meaning.parser_version == PARTIAL_WEB_CANDIDATE_FOLLOWUP_PARSER_VERSION
    {
        matches!(
            bundle.meaning.intent,
            Intent::FetchPreviousSearchCandidate { .. }
        ) && is_partial_web_candidate_context(previous)
            && bundle
                .request
                .context_ref
                .as_ref()
                .is_none_or(|id| id == &previous.id)
    } else {
        previous.lifecycle == Lifecycle::Commit
    };
    if !eligible
        || bundle.meaning.context_result_ref.as_deref() != Some(previous.id.as_str())
        || bundle.context_sha256 != identity(&Some(previous))?
    {
        return Err(failure(
            code,
            "the retained predecessor differs from the original input",
        ));
    }
    Ok(())
}

pub(crate) fn is_partial_web_candidate_context(result: &ResultRecord) -> bool {
    result.lifecycle == Lifecycle::Suspend
        && result
            .reason
            .as_deref()
            .and_then(|reason| reason.strip_prefix("WEB_BODY_UNAVAILABLE: "))
            == Some(PARTIAL_WEB_STOP_MESSAGE)
        && matches!(result.evidence.as_slice(), [_, packet]
            if packet.failure.as_deref() == Some(PARTIAL_WEB_BODY_FAILURE))
}

fn partial_web_candidate_observation(
    store: &Store,
    actor: &str,
    case: &str,
    previous: &ResultRecord,
    history: Option<&VerifiedCaseHistory<'_>>,
) -> Result<(String, SearchObservation)> {
    let invalid = || {
        failure(
            "SEARCH_CONTEXT_INVALID",
            "本文量の上限で保留した元の出典質問と検索一覧を照合できません。",
        )
    };
    if !is_partial_web_candidate_context(previous)
        || previous.text != PARTIAL_WEB_STOP_MESSAGE
        || previous.guarantee != Guarantee::Unknown
        || previous.calculation.is_some()
        || previous.content_plan.is_some()
        || previous.render.is_some()
        || previous.execution_graph_ref.is_some()
        || !previous.receipts.is_empty()
    {
        return Err(invalid());
    }
    let [listing_ref, body_ref] = previous.dependencies.as_slice() else {
        return Err(invalid());
    };
    let [listing, packet] = previous.evidence.as_slice() else {
        return Err(invalid());
    };
    let stopped = match history {
        Some(history) => history.suspended_result_revision(previous),
        None => store.load_suspended_result_revision(actor, case, previous),
    }?
    .ok_or_else(invalid)?;
    let (_, original) = match history {
        Some(history) => history.input_bundle(&previous.input_id, previous.input_version),
        None => store.get_input_bundle(actor, case, &previous.input_id, previous.input_version),
    }?
    .ok_or_else(invalid)?;
    let request = &original.request;
    let Intent::AskWeb { topic, .. } = &original.meaning.intent else {
        return Err(invalid());
    };
    if original.meaning.parser_version != WEB_QUESTION_PARSER_VERSION
        || request.actor_id != actor
        || request.case_id != case
        || identity(request)? != previous.id
        || stopped.actor_id != actor
        || stopped.case_id != case
        || stopped.input_fingerprint.as_ref() != Some(&previous.id)
        || stopped.bundle.as_ref() != Some(&original)
        || stopped.reason != previous.reason
    {
        return Err(invalid());
    }
    let goal_ref = stopped.goal_sha256.as_ref().ok_or_else(invalid)?;
    let goal_bytes = store.get_bytes(actor, case, goal_ref)?;
    let goal: GoalContract = serde_json::from_slice(&goal_bytes)?;
    let graph_bytes = store.get_bytes(actor, case, &goal.required_graph_ref)?;
    let nodes: Vec<GoalNode> = serde_json::from_slice(&graph_bytes)?;
    let allowed_starts = (goal.budget.max_actions as usize)
        .checked_sub(nodes.len().saturating_sub(2))
        .ok_or_else(invalid)?;
    if canonical(&goal)? != goal_bytes
        || !nodes.starts_with(&web_answer::nodes())
        || goal.input_fingerprint != previous.id
        || goal.intent != original.meaning.intent
        || goal.permissions != request.permissions
        || goal.budget != request.budget
        || goal.output_path != request.output_path
    {
        return Err(invalid());
    }
    let listing_bytes = store.get_bytes(actor, case, listing_ref)?;
    let observation: SearchObservation = serde_json::from_slice(&listing_bytes)?;
    if canonical(&observation)? != listing_bytes
        || &observation.listing != listing
        || &observation.query != topic
    {
        return Err(invalid());
    }
    let candidate = web_answer::select_candidate(&observation, &goal.permissions)?;
    ports::validate_observed_route(packet, &goal.permissions)?;
    if packet.mode != "LIVE"
        || packet.packet_kind != "FETCH_FAILURE"
        || !(200..300).contains(&packet.http_status)
        || packet.provider_id != "https-direct-url-v1"
        || packet.parser_version != "strict-utf8-identity-v1"
        || packet.independence_group != packet.final_uri
        || packet.request_uri != candidate_body_uri(candidate)?
        || packet.observed_at_unix_ms < listing.observed_at_unix_ms
        || packet.retry_after.is_some()
        || packet.extracted_text.is_some()
        || packet.raw_body.is_empty()
        || packet.raw_sha256 != digest(&packet.raw_body)
        || packet.id != ports::fetch_packet_identity(packet)?
        || canonical(packet)? != store.get_bytes(actor, case, body_ref)?
        || listing.raw_body.len().checked_add(packet.raw_body.len())
            != Some(goal.budget.max_evidence_bytes)
        || listing
            .redirect_chain
            .len()
            .saturating_add(packet.redirect_chain.len())
            > allowed_starts
    {
        return Err(invalid());
    }
    Ok((listing_ref.clone(), observation))
}

pub(crate) fn candidate_body_observation(
    store: &Store,
    bundle: &InputBundle,
    previous: &ResultRecord,
    history: Option<&VerifiedCaseHistory<'_>>,
    code: &str,
) -> Result<Option<(String, SearchObservation)>> {
    validate_search_predecessor(bundle, previous, code)?;
    let request = &bundle.request;
    let actor = &request.actor_id;
    let case = &request.case_id;
    if bundle.meaning.parser_version == PARTIAL_WEB_CANDIDATE_FOLLOWUP_PARSER_VERSION {
        return partial_web_candidate_observation(store, actor, case, previous, history).map(Some);
    }
    previous_observation_with_history(store, actor, case, previous, history)
}

const WEB_ANSWER_EXPLANATION_CONTEXT_INVALID: &str = "WEB_ANSWER_EXPLANATION_CONTEXT_INVALID";

fn invalid_web_answer_context() -> minidora_domain::Error {
    failure(
        WEB_ANSWER_EXPLANATION_CONTEXT_INVALID,
        "前回のWeb回答と保存された検索・取得・採用記録が一致しません。",
    )
}

pub(crate) fn verify_committed_web_answer<'a>(
    store: &Store,
    history: &VerifiedCaseHistory<'_>,
    previous: &'a ResultRecord,
    original: &InputBundle,
) -> Result<crate::recovery::RetainedWebAnswer<'a>> {
    use web_answer::{ANSWER_NODE, BODY_NODE, LISTING_NODE};

    let invalid = invalid_web_answer_context;
    if previous.lifecycle != Lifecycle::Commit
        || history
            .input_result(&previous.input_id, previous.input_version)?
            .as_ref()
            != Some(previous)
        || !history.contains_committed_result(previous)?
    {
        return Err(invalid());
    }
    let actor = &original.request.actor_id;
    let case = &original.request.case_id;
    let plan = previous.content_plan.as_ref().ok_or_else(invalid)?;
    let graph_ref = previous.execution_graph_ref.as_ref().ok_or_else(invalid)?;
    let graph: Value = serde_json::from_slice(&store.get_bytes(actor, case, graph_ref)?)?;
    let nodes: Vec<GoalNode> = serde_json::from_value(graph[0].clone())?;
    if !nodes.starts_with(&web_answer::nodes()) {
        return Err(invalid());
    }
    let proof = crate::recovery::verify_web_answer(store, original, previous, plan, nodes.len())?;
    let [listing_ref, body_ref, answer_ref] = proof.refs;
    if graph[1][LISTING_NODE].as_str() != Some(listing_ref)
        || graph[1][BODY_NODE].as_str() != Some(body_ref)
        || graph[1][ANSWER_NODE].as_str() != Some(answer_ref)
        || graph[5][LISTING_NODE] != serde_json::json!({})
        || graph[5][BODY_NODE] != serde_json::json!({ LISTING_NODE: listing_ref })
        || graph[5][ANSWER_NODE] != serde_json::json!({ BODY_NODE: body_ref })
        || graph[5]["compute"] != serde_json::json!({ ANSWER_NODE: answer_ref })
    {
        return Err(invalid());
    }
    crate::document::revalidate_previous_document_render(
        store,
        actor,
        case,
        previous,
        Some(answer_ref),
        WEB_ANSWER_EXPLANATION_CONTEXT_INVALID,
        "Web answer",
    )?;
    Ok(proof)
}

pub(crate) fn is_web_elevation_query(bundle: &InputBundle) -> bool {
    bundle.meaning.parser_version == WEB_QUESTION_PARSER_VERSION
        && matches!(
            bundle.meaning.intent,
            Intent::AskWeb {
                elevation_in_kilometres: true,
                ..
            }
        )
}

pub(crate) struct WebLengthConversion {
    pub(crate) calculation: Calculation,
    pub(crate) units: Vec<String>,
    pub(crate) dependencies: Vec<String>,
}

/// Reuse the typed quotient from an exact saved Web answer. Verification
/// here reads stored packets only and never requests a fresh observation.
pub(crate) fn previous_web_length_conversion(
    store: &Store,
    bundle: &InputBundle,
    previous: &ResultRecord,
    started: Instant,
) -> Result<WebLengthConversion> {
    let Intent::ConvertPreviousLength { unit } = &bundle.meaning.intent else {
        return Err(invalid_web_length_context());
    };
    if bundle.meaning.parser_version != crate::input::PREVIOUS_WEB_LENGTH_CONVERSION_PARSER_VERSION
    {
        return Err(invalid_web_length_context());
    }
    let mut origin = verified_web_length_origin(store, bundle, previous, started)?;
    origin.calculation = crate::language::convert_exact_length(&origin.calculation, unit)?;
    origin
        .units
        .insert(0, crate::core::exact_calculation_unit(&origin.calculation));
    if origin
        .source_bytes
        .saturating_add(origin.units.join("\n").len())
        > bundle.request.budget.max_evidence_bytes
    {
        return Err(web_length_limit());
    }
    if started.elapsed() >= Duration::from_millis(bundle.request.budget.deadline_ms) {
        return Err(web_length_deadline());
    }
    Ok(WebLengthConversion {
        calculation: origin.calculation,
        units: origin.units,
        dependencies: origin.dependencies,
    })
}

pub(crate) struct WebLengthOrigin {
    pub(crate) calculation: Calculation,
    pub(crate) units: Vec<String>,
    pub(crate) dependencies: Vec<String>,
    pub(crate) source_bytes: usize,
}

fn invalid_web_length_context() -> minidora_domain::Error {
    failure(
        "WEB_LENGTH_CONVERSION_CONTEXT_INVALID",
        "同じ会話の確定済み標高回答と採用した本文を照合できません。",
    )
}

fn web_length_limit() -> minidora_domain::Error {
    failure(
        "RESOURCE_LIMIT",
        "保存済みの出典と換算結果が要求の証拠予算を超えています。",
    )
}

fn web_length_deadline() -> minidora_domain::Error {
    failure(
        "DEADLINE",
        "標高の出典と換算結果の照合中に締切へ達しました。",
    )
}

/// Authenticate a saved Web length and its source without an external observation.
pub(crate) fn verified_web_length_origin(
    store: &Store,
    bundle: &InputBundle,
    previous: &ResultRecord,
    started: Instant,
) -> Result<WebLengthOrigin> {
    let check_deadline = || {
        if started.elapsed() >= Duration::from_millis(bundle.request.budget.deadline_ms) {
            return Err(web_length_deadline());
        }
        Ok(())
    };
    let request = &bundle.request;
    if previous.guarantee != Guarantee::SourceReport
        || request
            .context_ref
            .as_ref()
            .is_some_and(|id| id != &previous.id)
    {
        return Err(invalid_web_length_context());
    }
    validate_search_predecessor(bundle, previous, "WEB_LENGTH_CONVERSION_CONTEXT_INVALID")?;
    if request.budget.max_actions < 6 + u32::from(request.output_path.is_some()) {
        return Err(failure(
            "ACTION_BUDGET_EXHAUSTED",
            "検索一覧・本文・採用結果の照合と換算に必要な行動予算が不足しています。",
        ));
    }
    let [listing, body] = previous.evidence.as_slice() else {
        return Err(invalid_web_length_context());
    };
    let source_bytes = listing.raw_body.len().saturating_add(body.raw_body.len());
    if source_bytes > request.budget.max_evidence_bytes {
        return Err(web_length_limit());
    }
    check_deadline()?;
    let (input_ref, proof) =
        store.with_verified_case_history(&request.actor_id, &request.case_id, |history| {
            let (input_ref, original) = history
                .input_bundle(&previous.input_id, previous.input_version)?
                .ok_or_else(invalid_web_length_context)?;
            if !is_web_elevation_query(&original)
                || original.request.actor_id != request.actor_id
                || original.request.case_id != request.case_id
            {
                return Err(invalid_web_length_context());
            }
            let proof = verify_committed_web_answer(store, history, previous, &original)?;
            Ok((input_ref, proof))
        })?;
    let mut origin = proof
        .answer
        .elevation_calculation()
        .ok_or_else(invalid_web_length_context)?
        .clone();
    // The preserved Web artifact stores a unitless m / 1000 quotient. Its
    // sealed objective and replay above establish that the quotient is km.
    origin.exact_value.push_str(" km");
    let passage = &proof.answer.passage;
    let units = vec![
        format!(
            "前回採用した標高の出典: {}（取得時刻: Unix ms {}）",
            body.final_uri, body.observed_at_unix_ms
        ),
        format!(
            "採用本文 byte[{}..{}) SHA-256: {}",
            passage.source_start, passage.source_end, passage.source_span_sha256
        ),
        "保存した本文の記述を換算しました。再取得や現時点の真偽の確認はしていません。".into(),
    ];
    let source_bytes = source_bytes.saturating_add(canonical(&proof.answer)?.len());
    if source_bytes > request.budget.max_evidence_bytes {
        return Err(web_length_limit());
    }
    check_deadline()?;
    let mut dependencies = vec![previous.id.clone(), input_ref];
    dependencies.extend(proof.refs.into_iter().map(str::to_owned));
    Ok(WebLengthOrigin {
        calculation: origin,
        units,
        dependencies,
        source_bytes,
    })
}

fn explain_web_answer(
    store: &Store,
    history: &VerifiedCaseHistory<'_>,
    bundle: &InputBundle,
    previous: &ResultRecord,
    original: &InputBundle,
) -> Result<SearchExplanation> {
    validate_search_predecessor(bundle, previous, WEB_ANSWER_EXPLANATION_CONTEXT_INVALID)?;
    let proof = verify_committed_web_answer(store, history, previous, original)?;
    let [listing, body] = previous.evidence.as_slice() else {
        return Err(invalid_web_answer_context());
    };
    let units = proof
        .answer
        .explanation_units(listing, &proof.candidate, body);
    if units.join("\n").len() > bundle.request.budget.max_evidence_bytes {
        return Err(failure(
            "RESOURCE_LIMIT",
            "前回のWeb回答の根拠説明が出力予算を超えました。",
        ));
    }
    let mut dependencies = vec![previous.id.clone()];
    dependencies.extend(previous.dependencies.iter().cloned());
    Ok(SearchExplanation {
        units,
        dependencies,
        selected_body_index: None,
    })
}

pub(crate) struct RetainedSearchCandidateBody {
    search_ref: String,
    retains_listing: bool,
    pub(crate) body_id: String,
    pub(crate) body_ref: String,
    candidate: SearchCandidate,
    packet: EvidencePacket,
}

impl RetainedSearchCandidateBody {
    pub(crate) fn listing_ref(&self) -> Option<&String> {
        self.retains_listing.then_some(&self.search_ref)
    }
}

/// Verify the selected body by its dependency, even if an equal packet is
/// already among the carried bodies and was not appended to the evidence list.
pub(crate) fn verify_retained_candidate_body(
    store: &Store,
    bundle: &InputBundle,
    previous: &ResultRecord,
    result: &ResultRecord,
    plan: &ContentPlan,
    retains_listing: bool,
    history: Option<&VerifiedCaseHistory<'_>>,
) -> Result<RetainedSearchCandidateBody> {
    const CODE: &str = "RECOVERY_SEARCH_CONTEXT_MISMATCH";
    let invalid = || {
        failure(
            CODE,
            "the selected candidate, body or displayed source report differs",
        )
    };
    let Intent::FetchPreviousSearchCandidate { rank } = bundle.meaning.intent else {
        return Err(invalid());
    };
    let request = &bundle.request;
    let actor = &request.actor_id;
    let case = &request.case_id;
    if bundle.meaning.parser_version == PARTIAL_WEB_CANDIDATE_FOLLOWUP_PARSER_VERSION
        && !retains_listing
    {
        return Err(invalid());
    }
    let (search_ref, observation) =
        candidate_body_observation(store, bundle, previous, history, CODE)?.ok_or_else(invalid)?;
    let candidate = observation
        .candidates
        .iter()
        .find(|candidate| candidate.rank == rank)
        .ok_or_else(invalid)?;
    let body_ref = result.dependencies.last().ok_or_else(invalid)?;
    let packet_bytes = store.get_bytes(actor, case, body_ref)?;
    let packet: EvidencePacket = serde_json::from_slice(&packet_bytes)?;
    ports::validate_successful_packet(&packet, &request.permissions)?;
    if packet.request_uri != candidate_body_uri(candidate)?
        || packet.raw_body.len() > request.budget.max_evidence_bytes
        || canonical(&packet)? != packet_bytes
    {
        return Err(failure(
            "RECOVERY_EVIDENCE_MISMATCH",
            "the fetched packet differs from the selected candidate or original budget",
        ));
    }

    let mut expected_dependencies = vec![previous.id.clone(), search_ref.clone()];
    let mut expected_evidence = if retains_listing {
        let carried = carry_forward_candidate_evidence(previous, &observation)?;
        validate_next_candidate_body_capacity(&carried)?;
        for prior in carried
            .iter()
            .filter(|packet| packet.packet_kind == "FETCHED_BODY")
        {
            let bytes = canonical(prior)?;
            let reference = digest(&bytes);
            if store.get_bytes(actor, case, &reference)? != bytes {
                return Err(invalid());
            }
            expected_dependencies.push(reference);
        }
        carried
    } else {
        Vec::new()
    };
    expected_dependencies.push(body_ref.clone());
    if !expected_evidence.contains(&packet) {
        expected_evidence.push(packet.clone());
    }
    if result.evidence != expected_evidence {
        return Err(failure(
            "RECOVERY_EVIDENCE_MISMATCH",
            "the retained listing or body packets differ from the original result",
        ));
    }
    if result.dependencies != expected_dependencies
        || result.id != identity(request)?
        || result.input_id != request.input_id
        || result.input_version != request.input_version
        || result.calculation.is_some()
        || result.guarantee != Guarantee::SourceReport
        || plan.guarantee != result.guarantee
        || plan.locale != request.locale
        || plan.required_units != candidate_body_lines(candidate, &packet)
        || plan.text != result.text
        || plan.required_units.join("\n") != result.text
    {
        return Err(invalid());
    }
    Ok(RetainedSearchCandidateBody {
        search_ref,
        retains_listing,
        body_id: format!("search-candidate-body-{rank}"),
        body_ref: body_ref.clone(),
        candidate: candidate.clone(),
        packet,
    })
}

fn committed_search_result<'a>(
    store: &Store,
    history: &VerifiedCaseHistory<'_>,
    prior_results: &'a BTreeMap<String, ResultRecord>,
    mut current: &'a ResultRecord,
    selected_ref: &str,
    code: &str,
) -> Result<(&'a ResultRecord, RetainedSearchListing)> {
    let invalid = || {
        failure(
            code,
            "the selected observation cannot be traced to its original search",
        )
    };
    let mut visited = BTreeSet::new();
    loop {
        if !visited.insert(current.id.as_str()) {
            return Err(invalid());
        }
        let (_, bundle) = history
            .input_bundle(&current.input_id, current.input_version)?
            .ok_or_else(invalid)?;
        if let Intent::FetchPreviousSearchCandidate { .. } = bundle.meaning.intent {
            let parent = bundle
                .meaning
                .context_result_ref
                .as_ref()
                .and_then(|reference| prior_results.get(reference))
                .ok_or_else(invalid)?;
            if current.dependencies.first() != Some(&parent.id)
                || current.dependencies.get(1).map(String::as_str) != Some(selected_ref)
                || identity(&bundle.request)? != current.id
            {
                return Err(invalid());
            }
            validate_search_predecessor(&bundle, parent, code)?;
            current = parent;
            continue;
        }
        if is_partial_web_candidate_context(current) {
            let request = &bundle.request;
            let (reference, selected) = partial_web_candidate_observation(
                store,
                &request.actor_id,
                &request.case_id,
                current,
                Some(history),
            )?;
            if reference != selected_ref {
                return Err(invalid());
            }
            return Ok((
                current,
                RetainedSearchListing {
                    proof_ref: reference.clone(),
                    selected_ref: reference,
                    selected,
                    rejected_primary: None,
                },
            ));
        }
        let plan = current.content_plan.as_ref().ok_or_else(invalid)?;
        let root =
            verify_retained_search_listing(store, &bundle, current, plan)?.ok_or_else(invalid)?;
        if root.selected_ref != selected_ref {
            return Err(invalid());
        }
        return Ok((current, root));
    }
}

fn explain_candidate_body(
    store: &Store,
    history: &VerifiedCaseHistory<'_>,
    bundle: &InputBundle,
    prior_results: &BTreeMap<String, ResultRecord>,
    previous: &ResultRecord,
    original: &InputBundle,
    rank: u32,
) -> Result<SearchExplanation> {
    const CODE: &str = "SEARCH_CANDIDATE_BODY_EXPLANATION_CONTEXT_INVALID";
    let invalid = || {
        failure(
            CODE,
            "the original search candidate or saved body cannot be verified",
        )
    };
    validate_search_predecessor(bundle, previous, CODE)?;
    let request = &bundle.request;
    let actor = &request.actor_id;
    let case = &request.case_id;
    let preceding = original
        .meaning
        .context_result_ref
        .as_ref()
        .and_then(|reference| prior_results.get(reference))
        .ok_or_else(invalid)?;
    let plan = previous.content_plan.as_ref().ok_or_else(invalid)?;
    let graph_ref = previous.execution_graph_ref.as_ref().ok_or_else(invalid)?;
    let graph: Value = serde_json::from_slice(&store.get_bytes(actor, case, graph_ref)?)?;
    let nodes = graph[0].as_array().ok_or_else(invalid)?;
    let retains_listing = nodes
        .iter()
        .any(|node| node["id"] == SEARCH_CANDIDATE_LISTING_NODE);
    let proof = verify_retained_candidate_body(
        store,
        original,
        preceding,
        previous,
        plan,
        retains_listing,
        Some(history),
    )?;
    let allowed_starts = (original.request.budget.max_actions as usize)
        .checked_sub(nodes.len().saturating_sub(1))
        .ok_or_else(invalid)?;
    let listing_dependency = if retains_listing {
        serde_json::json!({ SEARCH_CANDIDATE_LISTING_NODE: proof.search_ref })
    } else {
        serde_json::json!({})
    };
    if proof.packet.redirect_chain.len() > allowed_starts
        || graph[1][&proof.body_id].as_str() != Some(&proof.body_ref)
        || graph[1][SEARCH_CANDIDATE_LISTING_NODE].as_str()
            != retains_listing.then_some(proof.search_ref.as_str())
        || graph[5][&proof.body_id] != listing_dependency
        || graph[5]["compute"] != serde_json::json!({ (&proof.body_id): proof.body_ref })
    {
        return Err(invalid());
    }
    crate::document::revalidate_previous_document_render(
        store,
        actor,
        case,
        previous,
        Some(&proof.body_ref),
        CODE,
        "search candidate body",
    )?;
    let (root_result, root) = committed_search_result(
        store,
        history,
        prior_results,
        preceding,
        &proof.search_ref,
        CODE,
    )?;
    if root_result.lifecycle == Lifecycle::Commit {
        crate::document::revalidate_previous_document_render(
            store,
            actor,
            case,
            root_result,
            Some(&root.selected_ref),
            CODE,
            "original search listing",
        )?;
    }
    let mut dependencies = vec![previous.id.clone()];
    if bundle.meaning.parser_version
        != crate::input::SEARCH_CANDIDATE_BODY_EXPLANATION_PARSER_VERSION
    {
        return Ok(SearchExplanation {
            units: vec![previous.text.clone()],
            dependencies,
            selected_body_index: None,
        });
    }

    let selected = root.selected;
    let candidate = proof.candidate;
    let body = proof.packet;
    let selected_body_index = previous
        .evidence
        .iter()
        .position(|packet| packet == &body)
        .ok_or_else(invalid)?;
    let extracted = body.extracted_text.as_ref().ok_or_else(invalid)?;
    let mut units = vec![
        format!(
            "前回の検索候補本文の取得根拠（取得結果ID: {}）。",
            previous.id
        ),
        format!(
            "元の検索結果ID: {} / 検索語: {:?}",
            root_result.id, selected.query
        ),
        format!(
            "採用した検索先: {} / 一覧packet ID: {} / 選択記録SHA-256: {}",
            selected.provider.provider_id(),
            selected.listing.id,
            root.proof_ref
        ),
        format!(
            "掲載順位 {rank} / 題名: {:?} / 候補ID: {} / 候補URL: {}",
            candidate.title, candidate.id, candidate.uri
        ),
        format!(
            "本文要求URL: {} / 本文最終URL: {}",
            body.request_uri, body.final_uri
        ),
        format!(
            "本文packet ID: {} / 取得モード: {} / HTTP {} / 取得時刻（Unix ms）: {}",
            body.id, body.mode, body.http_status, body.observed_at_unix_ms
        ),
        format!(
            "原本文SHA-256: {} / 原文バイト数: {} / 抽出本文SHA-256: {} / 読み取り方式: {}",
            body.raw_sha256,
            body.raw_body.len(),
            digest(extracted.as_bytes()),
            body.parser_version
        ),
    ];
    if root_result.lifecycle == Lifecycle::Suspend {
        units.push("元の出典質問は本文量の上限で保留となり、確認できた検索一覧を今回の本文取得に使いました。".into());
    }
    if let Some(failed) = root.rejected_primary {
        units.push(format!(
            "最初の検索先: {} / HTTP {} / {} / 一覧packet ID: {}",
            failed.provider.provider_id(),
            failed.listing.http_status,
            failed
                .listing
                .failure
                .as_deref()
                .unwrap_or("SEARCH_LIST_FETCH_FAILED"),
            failed.listing.id
        ));
    }
    if body.redirect_chain.len() > 1 {
        units.push(format!("本文取得経路: {}", body.redirect_chain.join(" → ")));
    }
    units.push("一覧と本文は別々の取得記録です。本文の事実性は未検証です。".into());
    if units.join("\n").len() > request.budget.max_evidence_bytes {
        return Err(failure(
            "RESOURCE_LIMIT",
            "the candidate body explanation exceeds its evidence budget",
        ));
    }
    dependencies.extend(previous.dependencies.iter().cloned());
    for reference in std::iter::once(&root_result.id)
        .chain(&root_result.dependencies)
        .chain(std::iter::once(&root.proof_ref))
    {
        if !dependencies.contains(reference) {
            dependencies.push(reference.clone());
        }
    }
    Ok(SearchExplanation {
        units,
        dependencies,
        selected_body_index: Some(selected_body_index),
    })
}

pub(crate) fn previous_observation(
    store: &crate::store::Store,
    actor: &str,
    case: &str,
    previous: &ResultRecord,
) -> Result<Option<(String, SearchObservation)>> {
    previous_observation_with_history(store, actor, case, previous, None)
}

pub(crate) fn previous_observation_with_history(
    store: &crate::store::Store,
    actor: &str,
    case: &str,
    previous: &ResultRecord,
    history: Option<&VerifiedCaseHistory<'_>>,
) -> Result<Option<(String, SearchObservation)>> {
    let mut found = None;
    let mut failed: Option<SearchObservation> = None;
    for dependency in &previous.dependencies {
        let Some(bytes) = store.get_bytes_if_present(actor, case, dependency)? else {
            continue;
        };
        let Ok(observation) = serde_json::from_slice::<SearchObservation>(&bytes) else {
            continue;
        };
        if observation.listing.packet_kind != "SEARCH_LIST" {
            continue;
        }
        if canonical(&observation)? != bytes
            || !previous
                .evidence
                .iter()
                .any(|packet| packet == &observation.listing)
        {
            return Err(failure(
                "SEARCH_CONTEXT_INVALID",
                "The retained search report is not the listing observed by the prior result",
            ));
        }
        if observation.failure.is_some() {
            if failed.is_some() || found.is_some() || !allows_fallback(&observation) {
                return Err(failure(
                    "SEARCH_CONTEXT_INVALID",
                    "Unexpected failed search in prior listing order",
                ));
            }
            validate_failed_observation(&observation, None)?;
            failed = Some(observation);
            continue;
        }
        validate_stored_observation(&observation, None)?;
        if failed.as_ref().is_some_and(|earlier| {
            earlier.provider == observation.provider || earlier.query != observation.query
        }) {
            return Err(failure(
                "SEARCH_CONTEXT_INVALID",
                "The selected search differs from the retained alternate attempt",
            ));
        }
        if found.is_some() {
            return Err(failure(
                "SEARCH_CONTEXT_AMBIGUOUS",
                "More than one live search listing is attached to the prior result",
            ));
        }
        found = Some((dependency.clone(), observation));
    }
    if let Some((_, selected)) = &found {
        let sealed = match history {
            Some(history) => history.input_bundle(&previous.input_id, previous.input_version),
            None => store.get_input_bundle(actor, case, &previous.input_id, previous.input_version),
        }?;
        let plan = match &sealed {
            Some((_, bundle)) => search_plan(&bundle.meaning.intent)?,
            None => None,
        };
        if let Some(plan) = plan
            && let Some(fallback) = plan.fallback
        {
            let expected = if failed.is_some() {
                fallback
            } else {
                plan.primary
            };
            let count = 1 + usize::from(failed.is_some());
            let (_, bundle) = sealed.as_ref().expect("a stored plan has a sealed input");
            if identity(&bundle.request)? != previous.id
                || selected.provider != expected
                || selected.query != plan.query
                || failed
                    .as_ref()
                    .is_some_and(|earlier| earlier.provider != plan.primary)
                || previous.dependencies.len() != count
                || previous.evidence.len() != count
            {
                return Err(failure(
                    "SEARCH_CONTEXT_INVALID",
                    "The selected listing differs from the sealed search order",
                ));
            }
        } else if failed.is_some() {
            return Err(failure(
                "SEARCH_CONTEXT_INVALID",
                "The failed search has no sealed alternate-provider goal",
            ));
        }
    } else if failed.is_some() {
        return Err(failure(
            "SEARCH_CONTEXT_INVALID",
            "The alternate search has no successfully selected listing",
        ));
    }
    Ok(found)
}

/// Resolve the committed context used by the immediately failed candidate read.
/// With no requested rank, the context must contain a previously fetched body.
pub(crate) fn committed_context_after_candidate_failure<'a>(
    store: &crate::store::Store,
    case: &'a CaseRevision,
    requested_rank: Option<u32>,
) -> Result<Option<&'a ResultRecord>> {
    let (Some(failed), Some(saved_bundle)) = (&case.result, &case.bundle) else {
        return Ok(None);
    };
    let Intent::FetchPreviousSearchCandidate { rank } = saved_bundle.meaning.intent else {
        return Ok(None);
    };
    let reason_code = failed
        .reason
        .as_deref()
        .and_then(|reason| reason.split_once(": "))
        .map(|(code, _)| code);
    if saved_bundle.meaning.parser_version == PARTIAL_WEB_CANDIDATE_FOLLOWUP_PARSER_VERSION
        || case.lifecycle != Lifecycle::Suspend
        || failed.lifecycle != Lifecycle::Suspend
        || requested_rank == Some(rank)
        || !matches!(
            reason_code,
            Some(
                "FETCH_OR_PARSER_FAILURE" | ports::REDIRECT_RETRY_DEFERRED | ports::RETRY_DEFERRED
            )
        )
        || failed.content_plan.is_some()
        || failed.render.is_some()
        || failed.execution_graph_ref.is_some()
        || !failed.receipts.is_empty()
        || !failed
            .evidence
            .first()
            .is_some_and(|packet| packet.packet_kind == "SEARCH_LIST")
    {
        return Ok(None);
    }
    if reason_code == Some(ports::RETRY_DEFERRED)
        && !failed.evidence.last().is_some_and(|packet| {
            packet.failure.as_deref() == Some(ports::RETRY_DEFERRED)
                && matches!(packet.http_status, 301 | 302 | 303 | 307 | 308)
                && packet.retry_after.is_none()
                && packet.raw_body.is_empty()
        })
    {
        return Ok(None);
    }

    let invalid = || {
        failure(
            "SEARCH_CONTEXT_INVALID",
            "The failed candidate does not match its sealed committed search context",
        )
    };
    let actor = &case.actor_id;
    let case_id = &case.case_id;
    let (_, bundle) = store
        .get_input_bundle(actor, case_id, &failed.input_id, failed.input_version)?
        .ok_or_else(invalid)?;
    if &bundle != saved_bundle
        || identity(&bundle.request)? != failed.id
        || case.input_fingerprint.as_deref() != Some(failed.id.as_str())
        || case.reason != failed.reason
    {
        return Err(invalid());
    }
    let context_id = bundle
        .meaning
        .context_result_ref
        .as_ref()
        .ok_or_else(invalid)?;
    let context = case.prior_results.get(context_id).ok_or_else(invalid)?;
    if context.lifecycle != Lifecycle::Commit
        || context.id != *context_id
        || identity(&Some(context))? != bundle.context_sha256
    {
        return Err(invalid());
    }

    let (listing_ref, observation) =
        previous_observation(store, actor, case_id, context)?.ok_or_else(invalid)?;
    let candidate = observation
        .candidates
        .iter()
        .find(|candidate| candidate.rank == rank)
        .ok_or_else(invalid)?;
    let (packet, prefix) = failed.evidence.split_last().ok_or_else(invalid)?;
    let carried = carry_forward_candidate_evidence(context, &observation)?;
    if carried != prefix
        || packet.mode != "LIVE"
        || packet.packet_kind != "FETCH_FAILURE"
        || packet.provider_id != "https-direct-url-v1"
        || packet.parser_version != "strict-utf8-identity-v1"
        || packet.failure.is_none()
        || packet.extracted_text.is_some()
        || packet.request_uri != candidate_body_uri(candidate)?
        || packet.independence_group != packet.final_uri
        || packet.raw_body.len() > bundle.request.budget.max_evidence_bytes
        || packet.raw_sha256 != digest(&packet.raw_body)
        || packet.id != ports::fetch_packet_identity(packet)?
    {
        return Err(invalid());
    }
    ports::validate_observed_route(packet, &bundle.request.permissions)?;

    let mut expected = vec![context.id.clone(), listing_ref];
    for body in &carried[1..] {
        expected.push(digest(&canonical(body)?));
    }
    let bytes = canonical(packet)?;
    let reference = digest(&bytes);
    expected.push(reference.clone());
    if failed.dependencies != expected || store.get_bytes(actor, case_id, &reference)? != bytes {
        return Err(invalid());
    }
    Ok((requested_rank.is_some() || carried.len() > 1).then_some(context))
}

/// Keep the validated search listing and previously fetched candidate bodies
/// on the next body-fetch result. This lets a later candidate selection reuse
/// the same listing and lets a following source question consume all retained
/// body packets. The listing's LIVE authority is checked by
/// `previous_observation`; these body records are checked here for exact
/// content identity without requesting their original read grants again.
pub(crate) fn carry_forward_candidate_evidence(
    previous: &ResultRecord,
    observation: &SearchObservation,
) -> Result<Vec<EvidencePacket>> {
    if !previous
        .evidence
        .iter()
        .any(|packet| packet == &observation.listing)
        || observation.listing.packet_kind != "SEARCH_LIST"
    {
        return Err(failure(
            "SEARCH_CONTEXT_INVALID",
            "The prior result does not retain the exact validated search listing",
        ));
    }

    let candidate_body_uris = observation
        .candidates
        .iter()
        .map(candidate_body_uri)
        .collect::<Result<BTreeSet<_>>>()?;
    let mut carried = vec![observation.listing.clone()];
    for packet in previous
        .evidence
        .iter()
        .filter(|packet| packet.packet_kind == "FETCHED_BODY")
    {
        let extracted_text = std::str::from_utf8(&packet.raw_body).map_err(|_| {
            failure(
                "SEARCH_CONTEXT_INVALID",
                "A retained candidate body is not valid UTF-8",
            )
        })?;
        let identity_is_valid = packet.id
            == identity(&(
                &packet.mode,
                &packet.packet_kind,
                &packet.request_uri,
                &packet.final_uri,
                packet.http_status,
                &packet.raw_sha256,
                &packet.parser_version,
                &packet.independence_group,
                &packet.failure,
                &packet.redirect_chain,
                &packet.provider_id,
            ))?;
        let live_contract_is_valid = packet.mode != "LIVE"
            || (packet.provider_id == "https-direct-url-v1"
                && packet.parser_version == "strict-utf8-identity-v1");
        if packet.failure.is_some()
            || packet.retry_after.is_some()
            || packet.http_status != 200
            || !packet.request_uri.starts_with("https://")
            || !packet.final_uri.starts_with("https://")
            || packet.independence_group != packet.final_uri
            || packet.redirect_chain.first() != Some(&packet.request_uri)
            || packet.redirect_chain.last() != Some(&packet.final_uri)
            || packet
                .redirect_chain
                .iter()
                .any(|uri| !uri.starts_with("https://"))
            || !candidate_body_uris.contains(&packet.request_uri)
            || packet.raw_sha256 != digest(&packet.raw_body)
            || packet.extracted_text.as_deref() != Some(extracted_text)
            || extracted_text.trim().is_empty()
            || extracted_text.chars().count() > 100_000
            || !matches!(packet.mode.as_str(), "LIVE" | "FIXTURE" | "FIXED_PACKET")
            || !live_contract_is_valid
            || !identity_is_valid
        {
            return Err(failure(
                "SEARCH_CONTEXT_INVALID",
                "A retained candidate body no longer matches its selected source and packet identity",
            ));
        }
        if !carried.iter().any(|retained| retained == packet) {
            carried.push(packet.clone());
        }
    }

    if carried
        .iter()
        .filter(|packet| packet.packet_kind == "FETCHED_BODY")
        .count()
        > MAX_RETAINED_BODY_PACKETS
    {
        return Err(failure(
            "SOURCE_BODY_PACKET_LIMIT",
            "A search case can retain at most 16 candidate body packets",
        ));
    }
    Ok(carried)
}

pub(crate) fn validate_next_candidate_body_capacity(evidence: &[EvidencePacket]) -> Result<()> {
    if evidence
        .iter()
        .filter(|packet| packet.packet_kind == "FETCHED_BODY")
        .count()
        >= MAX_RETAINED_BODY_PACKETS
    {
        return Err(failure(
            "SOURCE_BODY_PACKET_LIMIT",
            "A search case can retain at most 16 candidate body packets",
        ));
    }
    Ok(())
}

pub(crate) fn candidate_body_uri(candidate: &SearchCandidate) -> Result<String> {
    let checked = source_uri(&candidate.uri)?;
    let mut uri = Url::parse(&checked)
        .map_err(|_| failure("INVALID_SEARCH_CANDIDATE", "Candidate URL is malformed"))?;
    if candidate.provider == SearchProvider::Wikipedia {
        uri.query_pairs_mut().append_pair("action", "raw");
    }
    source_uri(uri.as_str())
}

pub(crate) fn candidate_body_lines(
    candidate: &SearchCandidate,
    packet: &EvidencePacket,
) -> Vec<String> {
    vec![
        format!(
            "検索候補 {}: {}\n取得した本文URI: {}",
            candidate.rank, candidate.title, packet.final_uri
        ),
        format!(
            "実取得本文を同一caseの証拠packetに保存しました。bytes={}; SHA-256={}; 次の「前回の本文を要約して」でこの本文を使えます。",
            packet.raw_body.len(),
            packet.raw_sha256
        ),
    ]
}

/// Candidate selection is not permission. The Core must separately authorize
/// the actual article hostname and verify the candidate against its saved list.
pub fn fetch_candidate(
    candidate: &SearchCandidate,
    permissions: &Permissions,
    budget: &ResourceBudget,
) -> Result<EvidencePacket> {
    fetch_candidate_with_budget(
        candidate,
        permissions,
        &mut ports::RetrievalBudget::standalone(budget)?,
        &|| Ok(()),
    )
    .map(|read| read.observation)
}

pub(crate) fn fetch_candidate_with_budget(
    candidate: &SearchCandidate,
    permissions: &Permissions,
    retrieval: &mut ports::RetrievalBudget,
    check_admission: &dyn Fn() -> Result<()>,
) -> Result<ports::ReadObservation<EvidencePacket>> {
    if candidate.rank == 0
        || candidate.provider_id != candidate.provider.provider_id()
        || candidate.search_packet_id.len() != 64
        || candidate.expected_identity()? != candidate.id
    {
        return Err(failure(
            "INVALID_SEARCH_CANDIDATE",
            "Candidate identity or provider provenance changed",
        ));
    }
    let body_uri = candidate_body_uri(candidate)?;
    ports::fetch_with_budget(&body_uri, permissions, retrieval, check_admission)
}

#[cfg(test)]
mod tests {
    use super::*;
    use minidora_domain::{Lifecycle, digest};
    fn packet(raw: &str) -> EvidencePacket {
        EvidencePacket {
            id: digest(raw.as_bytes()),
            mode: "FIXED_PACKET".into(),
            packet_kind: "FETCHED_BODY".into(),
            request_uri: "https://fixture.invalid/list".into(),
            final_uri: "https://fixture.invalid/list".into(),
            http_status: 200,
            raw_body: raw.as_bytes().to_vec(),
            raw_sha256: digest(raw.as_bytes()),
            extracted_text: Some(raw.into()),
            parser_version: "fixture-json-v1".into(),
            independence_group: "fixture".into(),
            failure: None,
            retry_after: None,
            redirect_chain: vec!["https://fixture.invalid/list".into()],
            observed_at_unix_ms: 0,
            provider_id: "fixture".into(),
        }
    }
    fn fixture(provider: SearchProvider, query: &str, raw: &str) -> EvidencePacket {
        let mut value = packet(raw);
        value.request_uri = provider.request_uri(query).unwrap().to_string();
        value.final_uri = value.request_uri.clone();
        value.redirect_chain = vec![value.request_uri.clone()];
        value
    }
    fn live_fixture(provider: SearchProvider, query: &str, raw: &str) -> EvidencePacket {
        let mut value = fixture(provider, query, raw);
        value.mode = "LIVE".into();
        value.packet_kind = "FETCHED_BODY".into();
        value.provider_id = "https-direct-url-v1".into();
        value.parser_version = "strict-utf8-identity-v1".into();
        value.independence_group = value.final_uri.clone();
        value.raw_sha256 = digest(&value.raw_body);
        value.id = identity(&(
            &value.mode,
            &value.packet_kind,
            &value.request_uri,
            &value.final_uri,
            value.http_status,
            &value.raw_sha256,
            &value.parser_version,
            &value.independence_group,
            &value.failure,
            &value.redirect_chain,
            &value.provider_id,
        ))
        .unwrap();
        value
    }

    fn fixture_candidate_body(candidate: &SearchCandidate, text: &str) -> EvidencePacket {
        let request_uri = candidate_body_uri(candidate).unwrap();
        let raw_body = text.as_bytes().to_vec();
        let mut value = EvidencePacket {
            id: String::new(),
            mode: "FIXTURE".into(),
            packet_kind: "FETCHED_BODY".into(),
            request_uri: request_uri.clone(),
            final_uri: request_uri.clone(),
            redirect_chain: vec![request_uri.clone()],
            observed_at_unix_ms: 123,
            provider_id: "test-body-fixture".into(),
            http_status: 200,
            raw_sha256: digest(&raw_body),
            extracted_text: Some(text.into()),
            raw_body,
            parser_version: "test-exact-utf8-v1".into(),
            independence_group: request_uri,
            failure: None,
            retry_after: None,
        };
        value.id = identity(&(
            &value.mode,
            &value.packet_kind,
            &value.request_uri,
            &value.final_uri,
            value.http_status,
            &value.raw_sha256,
            &value.parser_version,
            &value.independence_group,
            &value.failure,
            &value.redirect_chain,
            &value.provider_id,
        ))
        .unwrap();
        value
    }

    #[test]
    fn candidate_body_lineage_retains_the_listing_and_all_verified_prior_bodies() {
        let observation = parse_listing(
            SearchProvider::Wikipedia,
            "photosynthesis",
            fixture(
                SearchProvider::Wikipedia,
                "photosynthesis",
                r#"{"pages":[{"key":"First","title":"First"},{"key":"Second","title":"Second"}]}"#,
            ),
        )
        .unwrap();
        let first_body = fixture_candidate_body(
            &observation.candidates[0],
            "Photosynthesis stores light energy.",
        );
        let second_body = fixture_candidate_body(
            &observation.candidates[1],
            "Photosynthesis converts sunlight into chemical energy.",
        );
        let mut repeated_body = first_body.clone();
        repeated_body.observed_at_unix_ms += 1;
        assert_eq!(repeated_body.id, first_body.id);
        let prior = ResultRecord {
            id: "a".repeat(64),
            input_id: "first-fetch".into(),
            input_version: 1,
            lifecycle: Lifecycle::Commit,
            text: "fixture candidate body".into(),
            reason: None,
            guarantee: minidora_domain::Guarantee::SourceReport,
            calculation: None,
            content_plan: None,
            render: None,
            receipts: Vec::new(),
            evidence: vec![
                observation.listing.clone(),
                first_body.clone(),
                second_body.clone(),
                repeated_body.clone(),
            ],
            dependencies: Vec::new(),
            actor_memory_revision: None,
            execution_graph_ref: None,
        };

        let carried = carry_forward_candidate_evidence(&prior, &observation).unwrap();

        assert_eq!(
            carried,
            vec![
                observation.listing.clone(),
                first_body,
                second_body,
                repeated_body
            ]
        );
        assert_eq!(carried[0].packet_kind, "SEARCH_LIST");
        assert_eq!(carried[1].mode, "FIXTURE");
        assert_eq!(carried[2].mode, "FIXTURE");
        assert_eq!(carried[3].mode, "FIXTURE");
    }

    #[test]
    fn candidate_body_lineage_rejects_changed_packet_or_unlisted_source() {
        let observation = parse_listing(
            SearchProvider::Wikipedia,
            "photosynthesis",
            fixture(
                SearchProvider::Wikipedia,
                "photosynthesis",
                r#"{"pages":[{"key":"First","title":"First"}]}"#,
            ),
        )
        .unwrap();
        let mut body = fixture_candidate_body(
            &observation.candidates[0],
            "Photosynthesis stores light energy.",
        );
        body.extracted_text = Some("altered content".into());
        let prior = ResultRecord {
            id: "a".repeat(64),
            input_id: "first-fetch".into(),
            input_version: 1,
            lifecycle: Lifecycle::Commit,
            text: "fixture candidate body".into(),
            reason: None,
            guarantee: minidora_domain::Guarantee::SourceReport,
            calculation: None,
            content_plan: None,
            render: None,
            receipts: Vec::new(),
            evidence: vec![observation.listing.clone(), body],
            dependencies: Vec::new(),
            actor_memory_revision: None,
            execution_graph_ref: None,
        };

        let error = carry_forward_candidate_evidence(&prior, &observation).unwrap_err();
        assert!(error.to_string().contains("SEARCH_CONTEXT_INVALID"));
    }

    #[test]
    fn candidate_body_lineage_stops_before_the_seventeenth_packet() {
        let observation = parse_listing(
            SearchProvider::Wikipedia,
            "photosynthesis",
            fixture(
                SearchProvider::Wikipedia,
                "photosynthesis",
                r#"{"pages":[{"key":"First","title":"First"},{"key":"Second","title":"Second"}]}"#,
            ),
        )
        .unwrap();
        let mut evidence = vec![observation.listing.clone()];
        for index in 0..MAX_RETAINED_BODY_PACKETS {
            evidence.push(fixture_candidate_body(
                &observation.candidates[index % observation.candidates.len()],
                &format!("Photosynthesis fixture candidate {index} has a distinct packet."),
            ));
        }

        let error = validate_next_candidate_body_capacity(&evidence).unwrap_err();
        assert!(error.to_string().contains("SOURCE_BODY_PACKET_LIMIT"));
    }
    #[test]
    fn wikipedia_listing_preserves_rank_raw_source_and_separates_snippets() {
        let raw = r#"{"pages":[{"key":"東京","title":"東京","excerpt":"snippet is not body"},{"key":"東京","title":"duplicate"},{"key":"大阪","title":"大阪"}]}"#;
        let observation = parse_listing(
            SearchProvider::Wikipedia,
            "都市",
            fixture(SearchProvider::Wikipedia, "都市", raw),
        )
        .unwrap();
        assert_eq!(observation.listing.raw_body, raw.as_bytes());
        assert_eq!(observation.listing.mode, "FIXED_PACKET");
        assert_eq!(observation.listing.packet_kind, "SEARCH_LIST");
        assert!(observation.listing.extracted_text.is_none());
        assert_eq!(
            observation
                .candidates
                .iter()
                .map(|c| c.rank)
                .collect::<Vec<_>>(),
            vec![1, 3]
        );
        assert_eq!(observation.candidates[0].source_key, "東京");
        assert_eq!(observation.rejected[0].reason, "DUPLICATE_SOURCE");
        assert!(
            observation.candidates[0]
                .uri
                .starts_with("https://ja.wikipedia.org/wiki/")
        );
        assert!(
            fetch_candidate(
                &observation.candidates[0],
                &Permissions::default(),
                &ResourceBudget::development()
            )
            .is_err()
        );
    }
    #[test]
    fn stored_live_listing_revalidates_candidates_and_selects_raw_wikitext_route() {
        let observation = parse_listing(
            SearchProvider::Wikipedia,
            "city",
            live_fixture(
                SearchProvider::Wikipedia,
                "city",
                r#"{"pages":[{"key":"Tokyo","title":"Tokyo","excerpt":"not body"}]}"#,
            ),
        )
        .unwrap();
        validate_stored_observation(&observation, None).unwrap();
        let candidate = &observation.candidates[0];
        assert_eq!(
            candidate_body_uri(candidate).unwrap(),
            "https://ja.wikipedia.org/wiki/Tokyo?action=raw"
        );
        let mut changed = observation;
        changed.candidates[0].title = "unverified replacement".into();
        assert!(validate_stored_observation(&changed, None).is_err());
    }
    #[test]
    fn crossref_uses_returned_title_doi_and_uri_not_abstract_text() {
        let raw = r#"{"message":{"items":[{"title":["Measured article"],"DOI":"10.1234/AbC","URL":"https://doi.org/10.1234/AbC","abstract":"not a fetched paper"}]}}"#;
        let observation = parse_listing(
            SearchProvider::Crossref,
            "measured",
            fixture(SearchProvider::Crossref, "measured", raw),
        )
        .unwrap();
        assert_eq!(observation.candidates[0].title, "Measured article");
        assert_eq!(observation.candidates[0].source_key, "10.1234/abc");
        assert_eq!(observation.candidates[0].uri, "https://doi.org/10.1234/AbC");
        assert!(observation.failure.is_none());
    }
    #[test]
    fn europe_pmc_distinguishes_source_id_and_actual_fulltext_uri() {
        let raw = r#"{"resultList":{"result":[{"id":"123","source":"MED","title":"Paper one","fullTextUrlList":{"fullTextUrl":[{"url":"https://papers.example.org/full"}]}},{"id":"PMC456","source":"PMC","title":"Paper two"}]}}"#;
        let observation = parse_listing(
            SearchProvider::EuropePmc,
            "paper",
            fixture(SearchProvider::EuropePmc, "paper", raw),
        )
        .unwrap();
        assert_eq!(
            observation.candidates[0].uri,
            "https://papers.example.org/full"
        );
        assert_eq!(observation.candidates[0].source_key, "MED:123");
        assert_eq!(
            observation.candidates[1].uri,
            "https://europepmc.org/article/PMC/PMC456"
        );
    }
    #[test]
    fn malformed_duplicate_and_failed_lists_retain_original_packet() {
        for raw in [
            r#"{"pages":[],"pages":[]}"#,
            r#"{"unexpected":[]}"#,
            "not JSON",
        ] {
            let observation = parse_listing(
                SearchProvider::Wikipedia,
                "query",
                fixture(SearchProvider::Wikipedia, "query", raw),
            )
            .unwrap();
            assert_eq!(
                observation.failure.as_deref(),
                Some("MALFORMED_SEARCH_LIST")
            );
            assert!(observation.candidates.is_empty());
            assert_eq!(observation.listing.raw_body, raw.as_bytes());
        }
        let mut failed = fixture(SearchProvider::Crossref, "query", "rate limited");
        failed.http_status = 429;
        failed.failure = Some("HTTP_RATE_LIMITED".into());
        let observation = parse_listing(SearchProvider::Crossref, "query", failed).unwrap();
        assert_eq!(
            observation.failure.as_deref(),
            Some("SEARCH_LIST_FETCH_FAILED")
        );
        assert_eq!(observation.listing.http_status, 429);
    }
    #[test]
    fn source_hash_and_query_identity_cannot_be_substituted() {
        let raw = r#"{"pages":[{"key":"Tokyo","title":"Tokyo"}]}"#;
        let wrong_query = fixture(SearchProvider::Wikipedia, "different query", raw);
        assert_eq!(
            parse_listing(SearchProvider::Wikipedia, "city", wrong_query)
                .unwrap()
                .failure
                .as_deref(),
            Some("SEARCH_LIST_IDENTITY_MISMATCH")
        );
        let mut changed = fixture(SearchProvider::Wikipedia, "city", raw);
        changed.raw_body = br#"{"pages":[]}"#.to_vec();
        let observation = parse_listing(SearchProvider::Wikipedia, "city", changed).unwrap();
        assert_eq!(
            observation.failure.as_deref(),
            Some("SEARCH_LIST_IDENTITY_MISMATCH")
        );
        assert!(observation.candidates.is_empty());
    }
    #[test]
    fn changed_candidate_and_unapproved_provider_require_no_network() {
        let mut candidate = parse_listing(
            SearchProvider::Wikipedia,
            "city",
            fixture(
                SearchProvider::Wikipedia,
                "city",
                r#"{"pages":[{"key":"Tokyo","title":"Tokyo"}]}"#,
            ),
        )
        .unwrap()
        .candidates
        .remove(0);
        candidate.uri = "https://different.example.org/".into();
        assert!(
            fetch_candidate(
                &candidate,
                &Permissions::default(),
                &ResourceBudget::development()
            )
            .is_err()
        );
        assert!(
            SearchProvider::Crossref
                .search(
                    "query",
                    &Permissions::default(),
                    &ResourceBudget::development()
                )
                .is_err()
        );
    }
    #[test]
    fn advertised_metadata_hosts_match_pre_network_admission() {
        let capabilities = crate::Engine::capabilities();
        for (name, id) in [
            ("wikipedia", "search-wikipedia-metadata"),
            ("crossref", "search-crossref-metadata"),
            ("europepmc", "search-europepmc-metadata"),
        ] {
            let provider = SearchProvider::from_name(name).unwrap();
            let capability = capabilities
                .capabilities
                .iter()
                .find(|capability| capability.id == id)
                .unwrap();
            let Some(crate::CapabilityPermission::ExactHttpsHost { host }) = capability.permission
            else {
                panic!("no advertised HTTPS permission for {id}");
            };
            let budget = ResourceBudget::development();
            let stop_before_network = || Err(failure("TEST_STOP_BEFORE_NETWORK", id));
            let granted = Permissions {
                read_hosts: [host.into()].into(),
                ..Permissions::default()
            };
            for (permissions, expected) in [
                (Permissions::default(), ports::READ_PERMISSION_REQUIRED),
                (granted, "TEST_STOP_BEFORE_NETWORK"),
            ] {
                let mut retrieval = ports::RetrievalBudget::standalone(&budget).unwrap();
                let outcome = search_with_budget(
                    provider,
                    "metadata question",
                    &permissions,
                    &budget,
                    &mut retrieval,
                    &stop_before_network,
                );
                assert!(
                    matches!(outcome, Err(minidora_domain::Error::Contract { code, .. }) if code == expected),
                    "{id} did not reach {expected}"
                );
            }
        }
    }

    #[test]
    fn query_encoding_cannot_inject_provider_parameters() {
        let uri = SearchProvider::Crossref
            .request_uri("a&rows=99 #not-an-effect")
            .unwrap();
        let pairs: Vec<_> = uri.query_pairs().collect();
        assert_eq!(pairs[0].1, "a&rows=99 #not-an-effect");
        assert_eq!(pairs[1].1, "5");
        assert_eq!(uri.host_str(), Some("api.crossref.org"));
    }
}

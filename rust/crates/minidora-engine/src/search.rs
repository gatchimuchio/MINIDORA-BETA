//! Public metadata search and separately authorized source-body acquisition.
//!
//! Search listings and article bodies are different evidence records. Candidate
//! snippets/abstracts are not promoted to article text. This module executes a
//! Core-selected provider; it neither chooses goals nor grants read authority.

use crate::ports;
use minidora_domain::{
    EvidencePacket, Permissions, ResourceBudget, Result, ResultRecord, digest, failure, identity,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use url::{Host, Url};

const SEARCH_ROWS: usize = 5;
const MAX_RETAINED_BODY_PACKETS: usize = crate::text_summary::MAX_QUESTION_SOURCE_PACKETS;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SearchProvider {
    Wikipedia,
    Crossref,
    EuropePmc,
}

impl SearchProvider {
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

pub fn search(
    provider: SearchProvider,
    query: &str,
    permissions: &Permissions,
    budget: &ResourceBudget,
) -> Result<SearchObservation> {
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
    let listing = ports::fetch(uri.as_str(), permissions, budget)?;
    parse_listing(provider, query, listing)
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
    if observation.listing.failure.is_some() || observation.listing.http_status != 200 {
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

pub(crate) fn validate_stored_observation(observation: &SearchObservation) -> Result<()> {
    let listing = &observation.listing;
    if observation.failure.is_some()
        || listing.packet_kind != "SEARCH_LIST"
        || listing.provider_id != observation.provider.provider_id()
        || listing.mode != "LIVE"
        || listing.failure.is_some()
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
    if parse_listing(observation.provider, &observation.query, fetched_packet)? != *observation {
        return Err(failure(
            "SEARCH_CONTEXT_INVALID",
            "The retained candidates differ from the exact saved search listing",
        ));
    }
    Ok(())
}

pub(crate) fn previous_observation(
    store: &crate::store::Store,
    actor: &str,
    case: &str,
    previous: &ResultRecord,
) -> Result<Option<(String, SearchObservation)>> {
    let mut found = None;
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
        validate_stored_observation(&observation)?;
        if !previous
            .evidence
            .iter()
            .any(|packet| packet == &observation.listing)
        {
            return Err(failure(
                "SEARCH_CONTEXT_INVALID",
                "The retained search report is not the listing observed by the prior result",
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
    Ok(found)
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
    ports::fetch(&body_uri, permissions, budget)
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
        validate_stored_observation(&observation).unwrap();
        let candidate = &observation.candidates[0];
        assert_eq!(
            candidate_body_uri(candidate).unwrap(),
            "https://ja.wikipedia.org/wiki/Tokyo?action=raw"
        );
        let mut changed = observation;
        changed.candidates[0].title = "unverified replacement".into();
        assert!(validate_stored_observation(&changed).is_err());
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

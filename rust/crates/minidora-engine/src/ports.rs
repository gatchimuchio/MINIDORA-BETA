//! Bounded evidence observers and a source-preserving JSON selection adapter.
//! Direct URL retrieval is not a search provider, and retrieved bytes carry no
//! instruction or effect authority. HTML/PDF interpretation is not implemented.

use minidora_domain::{
    EvidencePacket, Permissions, ResourceBudget, Result, digest, failure, identity,
};
use regex::Regex;
use reqwest::blocking::Client;
use reqwest::redirect::Policy;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::ops::Range;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use url::{Host, Url};

fn approved_url(raw: &str, permissions: &Permissions) -> Result<Url> {
    let url = Url::parse(raw)
        .map_err(|_| failure("INVALID_READ_URL", "A valid absolute HTTPS URL is required"))?;
    let authority = raw
        .split_once("://")
        .map(|(_, tail)| tail.split(['/', '?', '#']).next().unwrap_or(""))
        .unwrap_or("");
    if url.scheme() != "https"
        || authority.contains('@')
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port_or_known_default() != Some(443)
    {
        return Err(failure(
            "INVALID_READ_URL",
            "Only HTTPS:443 without user information or fragments is supported",
        ));
    }
    let host = match url.host() {
        Some(Host::Domain(host)) => host,
        _ => {
            return Err(failure(
                "PRIVATE_READ_TARGET",
                "IP literal targets are unsupported",
            ));
        }
    };
    if !host.contains('.')
        || host.ends_with('.')
        || host == "localhost"
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
            "PRIVATE_READ_TARGET",
            "Local or ambiguous hostnames are unsupported",
        ));
    }
    if !permissions.read_hosts.contains(host) {
        return Err(failure(
            "READ_PERMISSION_REQUIRED",
            "The exact destination hostname is not in the authorized read scope",
        ));
    }
    Ok(url)
}

fn env_present(names: &[&str]) -> bool {
    names
        .iter()
        .any(|name| std::env::var_os(name).is_some_and(|v| !v.is_empty()))
}

fn uses_inherited_proxy(host: &str) -> bool {
    if !env_present(&["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"]) {
        return false;
    }
    let bypass = std::env::var("NO_PROXY")
        .or_else(|_| std::env::var("no_proxy"))
        .unwrap_or_default();
    !proxy_bypass_matches(host, &bypass)
}

fn proxy_bypass_matches(host: &str, bypass: &str) -> bool {
    bypass.split(',').any(|entry| {
        let entry = entry
            .trim()
            .trim_start_matches("*.")
            .trim_start_matches('.')
            .to_ascii_lowercase();
        let entry = entry.strip_suffix(":443").unwrap_or(&entry);
        entry == "*"
            || (!entry.is_empty() && (host == entry || host.ends_with(&format!(".{entry}"))))
    })
}

fn public_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, c, _] = v4.octets();
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_multicast()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_unspecified()
                || a == 0
                || a >= 240
                || (a == 100 && (64..=127).contains(&b))
                || (a == 198 && (b == 18 || b == 19))
                || (a == 192 && b == 0 && c == 0))
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return public_address(IpAddr::V4(v4));
            }
            let [first, second, ..] = v6.segments();
            // Restrict direct connections to global unicast; reject ULA,
            // link-local, multicast, transition and documentation ranges.
            (0x2000..=0x3fff).contains(&first)
                // Documentation prefixes: 2001:db8::/32 and 3fff::/20.
                && !(first == 0x3fff && second & 0xf000 == 0)
                && first != 0x2002
                && !(first == 0x2001 && (second == 0x0db8 || second < 0x0200))
        }
    }
}

// Blocking OS resolvers cannot be cancelled portably. Bound the number of
// outstanding resolver tasks and the caller's wait; timed-out read-only tasks
// hold their slot until the operating-system resolver actually returns.
static DNS_TASKS: AtomicUsize = AtomicUsize::new(0);

fn bounded_resolve(host: &str, remaining: Duration) -> Result<Vec<SocketAddr>> {
    if DNS_TASKS
        .try_update(Ordering::SeqCst, Ordering::SeqCst, |count| {
            (count < 4).then_some(count + 1)
        })
        .is_err()
    {
        return Err(failure(
            "RESOURCE_LIMIT",
            "The DNS observer concurrency envelope is exhausted",
        ));
    }
    let host = host.to_owned();
    let (sender, receiver) = mpsc::sync_channel(1);
    let thread = std::thread::Builder::new()
        .name("minidora-dns-observer".into())
        .spawn(move || {
            let result = (host.as_str(), 443)
                .to_socket_addrs()
                .map(|rows| rows.collect::<Vec<_>>());
            DNS_TASKS.fetch_sub(1, Ordering::SeqCst);
            let _ = sender.send(result);
        });
    if let Err(error) = thread {
        DNS_TASKS.fetch_sub(1, Ordering::SeqCst);
        return Err(error.into());
    }
    receiver
        .recv_timeout(remaining)
        .map_err(|_| {
            failure(
                "HTTP_DEADLINE_EXCEEDED",
                "Direct DNS resolution did not complete within its budget",
            )
        })?
        .map_err(Into::into)
}

fn client(url: &Url, remaining: Duration) -> Result<Client> {
    let start = Instant::now();
    let host = url.host_str().expect("approved domain");
    let mut builder = Client::builder()
        .redirect(Policy::none())
        .timeout(remaining)
        .connect_timeout(remaining)
        .user_agent("MINIDORA-Rust/0.7 evidence-observer");
    // A configured HTTPS proxy owns target DNS resolution and must enforce its
    // destination policy. Never bypass inherited proxy/NO_PROXY or CA trust.
    // For direct connections, validate every answer and pin inspected IPs to
    // prevent a second DNS lookup from changing the destination.
    if !uses_inherited_proxy(host) {
        let mut addresses = bounded_resolve(host, remaining)?;
        addresses.sort();
        addresses.dedup();
        if addresses.is_empty()
            || addresses
                .iter()
                .any(|address| !public_address(address.ip()))
        {
            return Err(failure(
                "PRIVATE_READ_TARGET",
                "Direct DNS did not yield exclusively public addresses",
            ));
        }
        builder = builder.resolve_to_addrs(host, &addresses);
    }
    let remaining = remaining
        .checked_sub(start.elapsed())
        .filter(|duration| !duration.is_zero())
        .ok_or_else(|| {
            failure(
                "HTTP_DEADLINE_EXCEEDED",
                "The connection preparation budget is exhausted",
            )
        })?;
    builder = builder.timeout(remaining).connect_timeout(remaining);
    builder.build().map_err(|_| {
        failure(
            "HTTP_CLIENT_UNAVAILABLE",
            "A verified HTTPS client could not be initialized",
        )
    })
}

fn packet(
    redirect_chain: &[String],
    request: &str,
    final_uri: &str,
    status: u16,
    raw: Vec<u8>,
    text: Option<String>,
    error: Option<&str>,
) -> Result<EvidencePacket> {
    let raw_sha256 = digest(&raw);
    let mut packet = EvidencePacket {
        id: String::new(),
        mode: "LIVE".into(),
        redirect_chain: redirect_chain.to_vec(),
        observed_at_unix_ms: u64::try_from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| failure("CLOCK_UNAVAILABLE", "Observation time precedes the epoch"))?
                .as_millis(),
        )
        .map_err(|_| {
            failure(
                "CLOCK_UNAVAILABLE",
                "Observation time exceeds its representation",
            )
        })?,
        provider_id: "https-direct-url-v1".into(),
        packet_kind: if error.is_some() {
            "FETCH_FAILURE"
        } else {
            "FETCHED_BODY"
        }
        .into(),
        request_uri: request.into(),
        final_uri: final_uri.into(),
        http_status: status,
        raw_body: raw,
        raw_sha256,
        extracted_text: text,
        parser_version: "strict-utf8-identity-v1".into(),
        independence_group: final_uri.into(),
        failure: error.map(str::to_owned),
    };
    packet.id = identity(&(
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
    Ok(packet)
}

fn decode_body(content_type: &str, bytes: &[u8]) -> Result<String> {
    let mut parts = content_type.split(';');
    let mime = parts.next().unwrap_or("").trim().to_ascii_lowercase();
    if !matches!(
        mime.as_str(),
        "text/plain" | "text/markdown" | "application/json"
    ) {
        return Err(failure(
            "UNSUPPORTED_EVIDENCE_FORMAT",
            "Only UTF-8 plain text, Markdown and JSON are supported",
        ));
    }
    let mut charset = None;
    for part in parts {
        let (key, value) = part
            .trim()
            .split_once('=')
            .ok_or_else(|| failure("UNSUPPORTED_CHARSET", "Malformed content type parameters"))?;
        if !key.trim().eq_ignore_ascii_case("charset") || charset.is_some() {
            return Err(failure(
                "UNSUPPORTED_CHARSET",
                "Unsupported or duplicate content type parameter",
            ));
        }
        charset = Some(value.trim().trim_matches('"').to_ascii_lowercase());
    }
    if charset.as_deref().is_some_and(|v| v != "utf-8") {
        return Err(failure(
            "UNSUPPORTED_CHARSET",
            "Evidence must explicitly use UTF-8 or its format default",
        ));
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| failure("EVIDENCE_DECODE_FAILED", "Invalid UTF-8 evidence"))?;
    if text.trim().is_empty() {
        return Err(failure(
            "EMPTY_EVIDENCE_BODY",
            "The fetched body contains no text",
        ));
    }
    if mime == "application/json" {
        JsonParser::new(text).parse()?;
    }
    Ok(text.into())
}

pub fn fetch(
    raw_url: &str,
    permissions: &Permissions,
    budget: &ResourceBudget,
) -> Result<EvidencePacket> {
    let mut url = approved_url(raw_url, permissions)?;
    if budget.max_evidence_bytes == 0 || budget.deadline_ms == 0 {
        return Err(failure(
            "RESOURCE_LIMIT",
            "A finite positive evidence and time budget is required",
        ));
    }
    let start = Instant::now();
    let deadline = Duration::from_millis(budget.deadline_ms);
    let mut redirects = vec![url.to_string()];
    loop {
        let Some(remaining) = deadline
            .checked_sub(start.elapsed())
            .filter(|r| !r.is_zero())
        else {
            return packet(
                &redirects,
                raw_url,
                url.as_str(),
                0,
                vec![],
                None,
                Some("HTTP_DEADLINE_EXCEEDED"),
            );
        };
        let http = match client(&url, remaining) {
            Ok(client) => client,
            Err(error) => {
                if matches!(&error, minidora_domain::Error::Contract { code,.. } if code == "PRIVATE_READ_TARGET")
                {
                    return Err(error);
                }
                return packet(
                    &redirects,
                    raw_url,
                    url.as_str(),
                    0,
                    vec![],
                    None,
                    Some("HTTP_CLIENT_OR_DNS_FAILED"),
                );
            }
        };
        let mut response = match http
            .get(url.clone())
            .header("Accept", "text/plain, text/markdown, application/json")
            .header("Accept-Encoding", "identity")
            .send()
        {
            Ok(response) => response,
            Err(_) => {
                return packet(
                    &redirects,
                    raw_url,
                    url.as_str(),
                    0,
                    vec![],
                    None,
                    Some("HTTP_TRANSPORT_FAILED"),
                );
            }
        };
        let status = response.status().as_u16();
        if matches!(status, 301 | 302 | 303 | 307 | 308) {
            if redirects.len() >= 4 {
                return packet(
                    &redirects,
                    raw_url,
                    url.as_str(),
                    status,
                    vec![],
                    None,
                    Some("REDIRECT_LIMIT"),
                );
            }
            let target = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|h| h.to_str().ok())
                .and_then(|s| url.join(s).ok());
            let Some(target) = target else {
                return packet(
                    &redirects,
                    raw_url,
                    url.as_str(),
                    status,
                    vec![],
                    None,
                    Some("INVALID_REDIRECT"),
                );
            };
            let target = match approved_url(target.as_str(), permissions) {
                Ok(target) => target,
                Err(_) => {
                    return packet(
                        &redirects,
                        raw_url,
                        url.as_str(),
                        status,
                        vec![],
                        None,
                        Some("REDIRECT_PERMISSION_OR_TARGET_REJECTED"),
                    );
                }
            };
            url = target;
            if redirects.iter().any(|prior| prior == url.as_str()) {
                return packet(
                    &redirects,
                    raw_url,
                    url.as_str(),
                    status,
                    vec![],
                    None,
                    Some("REDIRECT_CYCLE"),
                );
            }
            redirects.push(url.to_string());
            continue;
        }
        if response
            .content_length()
            .is_some_and(|n| n > budget.max_evidence_bytes as u64)
        {
            return packet(
                &redirects,
                raw_url,
                url.as_str(),
                status,
                vec![],
                None,
                Some("EVIDENCE_BYTE_LIMIT"),
            );
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|h| h.to_str().ok())
            .unwrap_or("")
            .to_owned();
        let encoding = response
            .headers()
            .get(reqwest::header::CONTENT_ENCODING)
            .and_then(|h| h.to_str().ok())
            .unwrap_or("identity")
            .to_owned();
        let mut raw = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            if start.elapsed() >= deadline {
                return packet(
                    &redirects,
                    raw_url,
                    url.as_str(),
                    status,
                    raw,
                    None,
                    Some("HTTP_DEADLINE_EXCEEDED"),
                );
            }
            let count = (budget.max_evidence_bytes.saturating_sub(raw.len()))
                .saturating_add(1)
                .min(chunk.len());
            match response.read(&mut chunk[..count]) {
                Ok(0) => break,
                Ok(n) => {
                    if n > budget.max_evidence_bytes - raw.len() {
                        return packet(
                            &redirects,
                            raw_url,
                            url.as_str(),
                            status,
                            raw,
                            None,
                            Some("EVIDENCE_BYTE_LIMIT_PARTIAL_BODY"),
                        );
                    }
                    raw.extend_from_slice(&chunk[..n]);
                }
                Err(_) => {
                    return packet(
                        &redirects,
                        raw_url,
                        url.as_str(),
                        status,
                        raw,
                        None,
                        Some("HTTP_BODY_INCOMPLETE"),
                    );
                }
            }
        }
        if status != 200 {
            return packet(
                &redirects,
                raw_url,
                url.as_str(),
                status,
                raw,
                None,
                Some(if status == 429 {
                    "HTTP_RATE_LIMITED"
                } else {
                    "HTTP_STATUS_FAILURE"
                }),
            );
        }
        if !encoding.eq_ignore_ascii_case("identity") {
            return packet(
                &redirects,
                raw_url,
                url.as_str(),
                status,
                raw,
                None,
                Some("UNSUPPORTED_CONTENT_ENCODING"),
            );
        }
        match decode_body(&content_type, &raw) {
            Ok(text) => {
                return packet(
                    &redirects,
                    raw_url,
                    url.as_str(),
                    status,
                    raw,
                    Some(text),
                    None,
                );
            }
            Err(minidora_domain::Error::Contract { code, .. }) => {
                return packet(
                    &redirects,
                    raw_url,
                    url.as_str(),
                    status,
                    raw,
                    None,
                    Some(&code),
                );
            }
            Err(_) => {
                return packet(
                    &redirects,
                    raw_url,
                    url.as_str(),
                    status,
                    raw,
                    None,
                    Some("EVIDENCE_DECODE_FAILED"),
                );
            }
        }
    }
}

#[derive(Debug)]
enum JsonKind {
    Object(BTreeMap<String, JsonNode>),
    Array(Vec<JsonNode>),
    Scalar,
    Number,
}
#[derive(Debug)]
struct JsonNode {
    span: Range<usize>,
    kind: JsonKind,
    object_key_spans: BTreeMap<String, Range<usize>>,
}
struct JsonParser<'a> {
    text: &'a str,
    position: usize,
    count: usize,
}
impl<'a> JsonParser<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            text,
            position: 0,
            count: 0,
        }
    }
    fn error(&self) -> minidora_domain::Error {
        failure(
            "INVALID_JSON_DOCUMENT",
            format!("Invalid JSON at byte {}", self.position),
        )
    }
    fn whitespace(&mut self) {
        while self
            .text
            .as_bytes()
            .get(self.position)
            .is_some_and(|b| matches!(b, b' ' | b'\n' | b'\r' | b'\t'))
        {
            self.position += 1;
        }
    }
    fn string(&mut self) -> Result<String> {
        let start = self.position;
        if self.text.as_bytes().get(start) != Some(&b'"') {
            return Err(self.error());
        }
        self.position += 1;
        let mut escaped = false;
        while let Some(&b) = self.text.as_bytes().get(self.position) {
            self.position += 1;
            if !escaped && b == b'"' {
                return serde_json::from_str(&self.text[start..self.position]).map_err(Into::into);
            }
            escaped = !escaped && b == b'\\';
        }
        Err(self.error())
    }
    fn parse(mut self) -> Result<JsonNode> {
        if self.text.len() > 2_000_000 {
            return Err(failure(
                "RESOURCE_LIMIT",
                "JSON adapter input exceeds its finite admission envelope",
            ));
        }
        let value = self.node(0)?;
        self.whitespace();
        if self.position != self.text.len() {
            return Err(self.error());
        }
        Ok(value)
    }
    fn node(&mut self, depth: usize) -> Result<JsonNode> {
        self.count += 1;
        if depth > 64 || self.count > 32768 {
            return Err(failure(
                "RESOURCE_LIMIT",
                "JSON nesting or node budget exceeded",
            ));
        }
        self.whitespace();
        let start = self.position;
        let byte = *self
            .text
            .as_bytes()
            .get(self.position)
            .ok_or_else(|| self.error())?;
        let mut object_key_spans = BTreeMap::new();
        let kind = match byte {
            b'{' => {
                self.position += 1;
                self.whitespace();
                let mut entries = BTreeMap::new();
                if self.text.as_bytes().get(self.position) == Some(&b'}') {
                    self.position += 1;
                } else {
                    loop {
                        self.whitespace();
                        let key_start = self.position;
                        let key = self.string()?;
                        let key_end = self.position;
                        self.whitespace();
                        if self.text.as_bytes().get(self.position) != Some(&b':') {
                            return Err(self.error());
                        }
                        self.position += 1;
                        let node = self.node(depth + 1)?;
                        if entries.contains_key(&key) {
                            return Err(failure(
                                "DUPLICATE_JSON_KEY",
                                "Decoded JSON object keys must be unique",
                            ));
                        }
                        entries.insert(key.clone(), node);
                        object_key_spans.insert(key, key_start..key_end);
                        self.whitespace();
                        match self.text.as_bytes().get(self.position) {
                            Some(b'}') => {
                                self.position += 1;
                                break;
                            }
                            Some(b',') => self.position += 1,
                            _ => return Err(self.error()),
                        }
                    }
                }
                JsonKind::Object(entries)
            }
            b'[' => {
                self.position += 1;
                self.whitespace();
                let mut entries = Vec::new();
                if self.text.as_bytes().get(self.position) == Some(&b']') {
                    self.position += 1;
                } else {
                    loop {
                        entries.push(self.node(depth + 1)?);
                        self.whitespace();
                        match self.text.as_bytes().get(self.position) {
                            Some(b']') => {
                                self.position += 1;
                                break;
                            }
                            Some(b',') => self.position += 1,
                            _ => return Err(self.error()),
                        }
                    }
                }
                JsonKind::Array(entries)
            }
            b'"' => {
                self.string()?;
                JsonKind::Scalar
            }
            b't' | b'f' | b'n' => {
                let literal = match byte {
                    b't' => "true",
                    b'f' => "false",
                    _ => "null",
                };
                if !self.text[self.position..].starts_with(literal) {
                    return Err(self.error());
                }
                self.position += literal.len();
                JsonKind::Scalar
            }
            b'-' | b'0'..=b'9' => {
                self.number()?;
                JsonKind::Number
            }
            _ => return Err(self.error()),
        };
        Ok(JsonNode {
            span: start..self.position,
            kind,
            object_key_spans,
        })
    }
    fn number(&mut self) -> Result<()> {
        let bytes = self.text.as_bytes();
        if bytes.get(self.position) == Some(&b'-') {
            self.position += 1;
        }
        match bytes.get(self.position) {
            Some(b'0') => self.position += 1,
            Some(b'1'..=b'9') => {
                self.position += 1;
                while bytes.get(self.position).is_some_and(u8::is_ascii_digit) {
                    self.position += 1;
                }
            }
            _ => return Err(self.error()),
        }
        if bytes.get(self.position) == Some(&b'.') {
            self.position += 1;
            let p = self.position;
            while bytes.get(self.position).is_some_and(u8::is_ascii_digit) {
                self.position += 1;
            }
            if p == self.position {
                return Err(self.error());
            }
        }
        if bytes
            .get(self.position)
            .is_some_and(|b| matches!(b, b'e' | b'E'))
        {
            self.position += 1;
            if bytes
                .get(self.position)
                .is_some_and(|b| matches!(b, b'+' | b'-'))
            {
                self.position += 1;
            }
            let p = self.position;
            while bytes.get(self.position).is_some_and(u8::is_ascii_digit) {
                self.position += 1;
            }
            if p == self.position {
                return Err(self.error());
            }
        }
        Ok(())
    }
}

/// Select exact original JSON bytes. No floating point conversion, duplicate
/// key overwrite, implicit missing value, or document rewrite is performed.
fn json_node_at_pointer<'a>(root: &'a JsonNode, pointer: &str) -> Result<&'a JsonNode> {
    if !pointer.is_empty() && !pointer.starts_with('/') {
        return Err(failure(
            "INVALID_JSON_POINTER",
            "A pointer is empty or begins with '/'",
        ));
    }
    let mut node = root;
    for raw in pointer.split('/').skip(1) {
        let mut key = String::new();
        let mut chars = raw.chars();
        while let Some(c) = chars.next() {
            if c != '~' {
                key.push(c);
            } else {
                match chars.next() {
                    Some('0') => key.push('~'),
                    Some('1') => key.push('/'),
                    _ => {
                        return Err(failure(
                            "INVALID_JSON_POINTER",
                            "Only ~0 and ~1 escapes are valid",
                        ));
                    }
                }
            }
        }
        node = match &node.kind {
            JsonKind::Object(entries) => entries.get(&key),
            JsonKind::Array(entries) => {
                if key.is_empty()
                    || (key.len() > 1 && key.starts_with('0'))
                    || !key.bytes().all(|b| b.is_ascii_digit())
                {
                    return Err(failure(
                        "INVALID_JSON_POINTER",
                        "Array indices require canonical nonnegative integers",
                    ));
                }
                key.parse::<usize>().ok().and_then(|i| entries.get(i))
            }
            JsonKind::Scalar | JsonKind::Number => None,
        }
        .ok_or_else(|| {
            failure(
                "JSON_POINTER_NOT_FOUND",
                "The required original document location does not exist",
            )
        })?;
    }
    Ok(node)
}

pub fn json_pointer(document: &str, pointer: &str) -> Result<String> {
    let root = JsonParser::new(document).parse()?;
    let node = json_node_at_pointer(&root, pointer)?;
    Ok(document[node.span.clone()].to_owned())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonSourceSpan {
    pub kind: String,
    pub source_start: usize,
    pub source_end: usize,
    pub source_text: String,
    pub array_index: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonStatisticsReport {
    pub source_sha256: String,
    pub pointer: String,
    pub statistics: crate::math::Statistics,
    pub spans: Vec<JsonSourceSpan>,
    pub guarantee: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonComparisonSpan {
    pub source_start: usize,
    pub source_end: usize,
    pub source_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonObjectProjectField {
    pub name: String,
    pub source_pointer: String,
    pub key_source: JsonComparisonSpan,
    pub value_source: JsonComparisonSpan,
    pub output_key_start: usize,
    pub output_key_end: usize,
    pub output_value_start: usize,
    pub output_value_end: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonObjectProjectReport {
    pub source_sha256: String,
    pub output_sha256: String,
    pub json: String,
    pub fields: Vec<JsonObjectProjectField>,
    pub guarantee: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonDifference {
    pub pointer: String,
    pub change: String,
    pub left: Option<JsonComparisonSpan>,
    pub right: Option<JsonComparisonSpan>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonComparisonReport {
    pub source_sha256: String,
    pub left_sha256: String,
    pub right_sha256: String,
    pub equal: bool,
    pub comparison: String,
    pub differences: Vec<JsonDifference>,
    pub guarantee: String,
}

const MAX_JSON_COMPARE_BYTES: usize = 256_000;
const MAX_JSON_COMPARE_DIFFERENCES: usize = 4096;
const MAX_JSON_COMPARE_OUTPUT_BYTES: usize = 256_000;

fn json_comparison_span(document: &str, node: &JsonNode) -> JsonComparisonSpan {
    json_range_span(document, &node.span)
}

fn json_range_span(document: &str, range: &Range<usize>) -> JsonComparisonSpan {
    JsonComparisonSpan {
        source_start: range.start,
        source_end: range.end,
        source_sha256: digest(document[range.clone()].as_bytes()),
    }
}

/// Project selected root-object fields in caller order, preserving each value's
/// exact source JSON lexeme and mapping key/value source bytes to output bytes.
pub fn json_object_project(document: &str, fields: &[String]) -> Result<JsonObjectProjectReport> {
    if document.len() > MAX_JSON_COMPARE_BYTES {
        return Err(failure(
            "JSON_OBJECT_PROJECT_INPUT_LIMIT",
            "object document exceeds 256000 source bytes",
        ));
    }
    if fields.is_empty() || fields.len() > 256 {
        return Err(failure(
            "JSON_OBJECT_PROJECT_FIELDS",
            "projection requires 1..256 field names",
        ));
    }
    let root = JsonParser::new(document).parse()?;
    let JsonKind::Object(values) = &root.kind else {
        return Err(failure(
            "JSON_OBJECT_PROJECT_SHAPE",
            "the source document must be one JSON object",
        ));
    };
    let mut seen = BTreeSet::new();
    let mut selected = Vec::with_capacity(fields.len());
    for name in fields {
        if !seen.insert(name) {
            return Err(failure(
                "JSON_OBJECT_PROJECT_DUPLICATE_FIELD",
                "requested field names must be unique",
            ));
        }
        let value = values.get(name).ok_or_else(|| {
            failure(
                "JSON_OBJECT_PROJECT_FIELD",
                format!("requested field {name:?} does not exist"),
            )
        })?;
        let key_span = root.object_key_spans.get(name).ok_or_else(|| {
            failure(
                "JSON_OBJECT_PROJECT_SOURCE_MAP",
                "the source key range is unavailable",
            )
        })?;
        selected.push((name, value, key_span));
    }

    let mut json = String::from("{");
    let mut output_budget = 1024usize;
    let mut output_fields = Vec::with_capacity(selected.len());
    for (index, (name, value, key_span)) in selected.into_iter().enumerate() {
        if index > 0 {
            json.push(',');
        }
        let output_key_start = json.len();
        json.push_str(&serde_json::to_string(name)?);
        let output_key_end = json.len();
        json.push(':');
        let output_value_start = json.len();
        json.push_str(&document[value.span.clone()]);
        let output_value_end = json.len();
        let pointer = json_pointer_child("", name);
        output_budget = output_budget
            .saturating_add(output_key_end - output_key_start)
            .saturating_add(output_value_end - output_value_start)
            .saturating_add(pointer.len())
            .saturating_add(512);
        if output_budget > MAX_JSON_COMPARE_OUTPUT_BYTES {
            return Err(failure(
                "JSON_OBJECT_PROJECT_OUTPUT_LIMIT",
                "projected document and source maps exceed the 256000-byte output envelope",
            ));
        }
        output_fields.push(JsonObjectProjectField {
            name: name.clone(),
            source_pointer: pointer,
            key_source: json_range_span(document, key_span),
            value_source: json_comparison_span(document, value),
            output_key_start,
            output_key_end,
            output_value_start,
            output_value_end,
        });
    }
    json.push('}');
    let report = JsonObjectProjectReport {
        source_sha256: digest(document.as_bytes()),
        output_sha256: digest(json.as_bytes()),
        json,
        fields: output_fields,
        guarantee: "bounded root-object projection in explicit caller order; selected value JSON lexemes are preserved byte-for-byte; source key/value ranges map to the projected output; no schema validity or factual truth is inferred".into(),
    };
    if serde_json::to_vec(&report)?.len() > MAX_JSON_COMPARE_OUTPUT_BYTES {
        return Err(failure(
            "JSON_OBJECT_PROJECT_OUTPUT_LIMIT",
            "projection report exceeds 256000 output bytes",
        ));
    }
    Ok(report)
}

fn add_json_difference(
    document: &str,
    pointer: String,
    change: &str,
    left: Option<&JsonNode>,
    right: Option<&JsonNode>,
    differences: &mut Vec<JsonDifference>,
    output_budget: &mut usize,
) -> Result<()> {
    if differences.len() >= MAX_JSON_COMPARE_DIFFERENCES {
        return Err(failure(
            "JSON_COMPARE_DIFFERENCE_LIMIT",
            "comparison exceeds 4096 recorded differences",
        ));
    }
    *output_budget = output_budget.saturating_add(pointer.len() + 512);
    if *output_budget > MAX_JSON_COMPARE_OUTPUT_BYTES {
        return Err(failure(
            "JSON_COMPARE_OUTPUT_LIMIT",
            "comparison report exceeds its bounded output envelope",
        ));
    }
    differences.push(JsonDifference {
        pointer,
        change: change.into(),
        left: left.map(|node| json_comparison_span(document, node)),
        right: right.map(|node| json_comparison_span(document, node)),
    });
    Ok(())
}

fn json_pointer_child(parent: &str, child: &str) -> String {
    let escaped = child.replace('~', "~0").replace('/', "~1");
    format!("{parent}/{escaped}")
}

fn json_scalar_equal(document: &str, left: &JsonNode, right: &JsonNode) -> Result<bool> {
    let left_text = &document[left.span.clone()];
    let right_text = &document[right.span.clone()];
    if left_text.starts_with('"') && right_text.starts_with('"') {
        Ok(serde_json::from_str::<String>(left_text)?
            == serde_json::from_str::<String>(right_text)?)
    } else {
        Ok(left_text == right_text)
    }
}

fn compare_json_nodes(
    document: &str,
    left: &JsonNode,
    right: &JsonNode,
    pointer: &str,
    differences: &mut Vec<JsonDifference>,
    output_budget: &mut usize,
) -> Result<()> {
    match (&left.kind, &right.kind) {
        (JsonKind::Object(left_entries), JsonKind::Object(right_entries)) => {
            let keys: BTreeSet<_> = left_entries.keys().chain(right_entries.keys()).collect();
            for key in keys {
                let child_pointer = json_pointer_child(pointer, key);
                match (left_entries.get(key), right_entries.get(key)) {
                    (Some(left_value), Some(right_value)) => compare_json_nodes(
                        document,
                        left_value,
                        right_value,
                        &child_pointer,
                        differences,
                        output_budget,
                    )?,
                    (Some(left_value), None) => add_json_difference(
                        document,
                        child_pointer,
                        "REMOVED",
                        Some(left_value),
                        None,
                        differences,
                        output_budget,
                    )?,
                    (None, Some(right_value)) => add_json_difference(
                        document,
                        child_pointer,
                        "ADDED",
                        None,
                        Some(right_value),
                        differences,
                        output_budget,
                    )?,
                    (None, None) => unreachable!("union key must occur in one object"),
                }
            }
        }
        (JsonKind::Array(left_values), JsonKind::Array(right_values)) => {
            for index in 0..left_values.len().max(right_values.len()) {
                let child_pointer = json_pointer_child(pointer, &index.to_string());
                match (left_values.get(index), right_values.get(index)) {
                    (Some(left_value), Some(right_value)) => compare_json_nodes(
                        document,
                        left_value,
                        right_value,
                        &child_pointer,
                        differences,
                        output_budget,
                    )?,
                    (Some(left_value), None) => add_json_difference(
                        document,
                        child_pointer,
                        "REMOVED",
                        Some(left_value),
                        None,
                        differences,
                        output_budget,
                    )?,
                    (None, Some(right_value)) => add_json_difference(
                        document,
                        child_pointer,
                        "ADDED",
                        None,
                        Some(right_value),
                        differences,
                        output_budget,
                    )?,
                    (None, None) => unreachable!("index must occur in one array"),
                }
            }
        }
        (JsonKind::Scalar, JsonKind::Scalar) => {
            if !json_scalar_equal(document, left, right)? {
                add_json_difference(
                    document,
                    pointer.into(),
                    "CHANGED",
                    Some(left),
                    Some(right),
                    differences,
                    output_budget,
                )?;
            }
        }
        (JsonKind::Number, JsonKind::Number) => {
            let left_text = &document[left.span.clone()];
            let right_text = &document[right.span.clone()];
            if !crate::math::json_number_equal(left_text, right_text)? {
                add_json_difference(
                    document,
                    pointer.into(),
                    "CHANGED",
                    Some(left),
                    Some(right),
                    differences,
                    output_budget,
                )?;
            }
        }
        _ => add_json_difference(
            document,
            pointer.into(),
            "CHANGED",
            Some(left),
            Some(right),
            differences,
            output_budget,
        )?,
    }
    Ok(())
}

/// Compare JSON values in `{"left": ..., "right": ...}`. Object key order and
/// JSON number spelling are ignored, while array order and decoded string
/// contents remain significant. Difference spans are byte offsets into the
/// original wrapper document.
pub fn json_compare(document: &str) -> Result<JsonComparisonReport> {
    if document.len() > MAX_JSON_COMPARE_BYTES {
        return Err(failure(
            "JSON_COMPARE_INPUT_LIMIT",
            "comparison wrapper exceeds 256000 source bytes",
        ));
    }
    let root = JsonParser::new(document).parse()?;
    let JsonKind::Object(wrapper) = &root.kind else {
        return Err(failure(
            "JSON_COMPARE_SHAPE",
            "comparison input must be an object with left and right fields",
        ));
    };
    if wrapper.len() != 2 || !wrapper.contains_key("left") || !wrapper.contains_key("right") {
        return Err(failure(
            "JSON_COMPARE_SHAPE",
            "comparison wrapper requires exactly left and right fields",
        ));
    }
    let left = wrapper.get("left").expect("validated left field");
    let right = wrapper.get("right").expect("validated right field");
    let mut differences = Vec::new();
    let mut output_budget = 512usize;
    compare_json_nodes(
        document,
        left,
        right,
        "",
        &mut differences,
        &mut output_budget,
    )?;
    let report = JsonComparisonReport {
        source_sha256: digest(document.as_bytes()),
        left_sha256: digest(document[left.span.clone()].as_bytes()),
        right_sha256: digest(document[right.span.clone()].as_bytes()),
        equal: differences.is_empty(),
        comparison: "JSON_STRUCTURAL_EXACT_DECIMAL".into(),
        differences,
        guarantee: "bounded structural JSON comparison; object order is ignored, array order and decoded strings are significant, numeric values are compared as exact decimals; no factual truth is inferred".into(),
    };
    if serde_json::to_vec(&report)?.len() > MAX_JSON_COMPARE_OUTPUT_BYTES {
        return Err(failure(
            "JSON_COMPARE_OUTPUT_LIMIT",
            "comparison report exceeds 256000 output bytes",
        ));
    }
    Ok(report)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonTableChange {
    pub change: String,
    pub key: String,
    pub column: Option<String>,
    pub left_value: Option<String>,
    pub right_value: Option<String>,
    pub left_pointer: Option<String>,
    pub right_pointer: Option<String>,
    pub left: Option<JsonComparisonSpan>,
    pub right: Option<JsonComparisonSpan>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonTableComparisonReport {
    pub source_sha256: String,
    pub left_sha256: String,
    pub right_sha256: String,
    pub key_column: String,
    pub equal: bool,
    pub changes: Vec<JsonTableChange>,
    pub guarantee: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonTableFilterDecision {
    pub source_row_index: usize,
    pub value: String,
    pub matched: bool,
    pub source: JsonComparisonSpan,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonTableFilterReport {
    pub source_sha256: String,
    pub columns: Vec<String>,
    pub column_source_spans: Vec<JsonComparisonSpan>,
    pub rows: Vec<Vec<String>>,
    pub source_row_indices: Vec<usize>,
    pub cell_source_spans: Vec<Vec<JsonComparisonSpan>>,
    pub filter_column: String,
    pub filter_column_index: usize,
    pub equals: String,
    pub decisions: Vec<JsonTableFilterDecision>,
    pub guarantee: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonTableSortReport {
    pub source_sha256: String,
    pub columns: Vec<String>,
    pub column_source_spans: Vec<JsonComparisonSpan>,
    pub rows: Vec<Vec<String>>,
    pub source_row_indices: Vec<usize>,
    pub cell_source_spans: Vec<Vec<JsonComparisonSpan>>,
    pub sort_column: String,
    pub sort_column_index: usize,
    pub sort_order: String,
    pub guarantee: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonTableGroup {
    pub value: String,
    pub count: usize,
    pub source_row_indices: Vec<usize>,
    pub member_source_spans: Vec<JsonComparisonSpan>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonTableGroupCountReport {
    pub source_sha256: String,
    pub group_by: String,
    pub group_by_source: JsonComparisonSpan,
    pub group_by_index: usize,
    pub row_count: usize,
    pub groups: Vec<JsonTableGroup>,
    pub guarantee: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonTableGroupedStatistics {
    pub value: String,
    pub source_row_indices: Vec<usize>,
    pub group_source_spans: Vec<JsonComparisonSpan>,
    pub value_source_spans: Vec<JsonComparisonSpan>,
    pub statistics: crate::math::Statistics,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonTableGroupStatisticsReport {
    pub source_sha256: String,
    pub group_by: String,
    pub group_by_index: usize,
    pub group_by_source: JsonComparisonSpan,
    pub value_column: String,
    pub value_column_index: usize,
    pub value_column_source: JsonComparisonSpan,
    pub row_count: usize,
    pub groups: Vec<JsonTableGroupedStatistics>,
    pub guarantee: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonTableSelectReport {
    pub source_sha256: String,
    pub column_name: String,
    pub column_index: usize,
    pub column_source: JsonComparisonSpan,
    pub values: Vec<String>,
    pub source_row_indices: Vec<usize>,
    pub source_spans: Vec<JsonComparisonSpan>,
    pub guarantee: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonTableProjectReport {
    pub source_sha256: String,
    pub columns: Vec<String>,
    pub selected_column_indices: Vec<usize>,
    pub column_source_spans: Vec<JsonComparisonSpan>,
    pub rows: Vec<Vec<String>>,
    pub source_row_indices: Vec<usize>,
    pub cell_source_spans: Vec<Vec<JsonComparisonSpan>>,
    pub guarantee: String,
}

struct JsonStringTableView<'a> {
    columns: Vec<String>,
    column_nodes: Vec<&'a JsonNode>,
    rows: Vec<Vec<&'a JsonNode>>,
    row_nodes: Vec<&'a JsonNode>,
}

fn json_string_value(document: &str, node: &JsonNode, code: &str) -> Result<String> {
    if !matches!(&node.kind, JsonKind::Scalar) {
        return Err(failure(code, "table labels and cells must be JSON strings"));
    }
    let raw = &document[node.span.clone()];
    if !raw.starts_with('"') {
        return Err(failure(code, "table labels and cells must be JSON strings"));
    }
    Ok(serde_json::from_str(raw)?)
}

fn json_string_table<'a>(document: &str, node: &'a JsonNode) -> Result<JsonStringTableView<'a>> {
    let JsonKind::Object(fields) = &node.kind else {
        return Err(failure(
            "JSON_TABLE_COMPARE_SHAPE",
            "each side must be an object with columns and rows",
        ));
    };
    if fields.len() != 2 || !fields.contains_key("columns") || !fields.contains_key("rows") {
        return Err(failure(
            "JSON_TABLE_COMPARE_SHAPE",
            "each table requires exactly columns and rows fields",
        ));
    }
    let columns_node = fields.get("columns").expect("validated columns field");
    let JsonKind::Array(column_nodes) = &columns_node.kind else {
        return Err(failure(
            "JSON_TABLE_COMPARE_COLUMNS",
            "keyed table comparison requires a named columns array",
        ));
    };
    if !(1..=256).contains(&column_nodes.len()) {
        return Err(failure(
            "JSON_TABLE_COMPARE_COLUMNS",
            "expected 1..256 named columns",
        ));
    }
    let mut columns = Vec::with_capacity(column_nodes.len());
    let mut decoded_column_nodes = Vec::with_capacity(column_nodes.len());
    let mut unique_columns = BTreeSet::new();
    for column_node in column_nodes {
        let value = json_string_value(document, column_node, "JSON_TABLE_COMPARE_COLUMNS")?;
        if !unique_columns.insert(value.clone()) {
            return Err(failure(
                "DUPLICATE_COLUMNS",
                "decoded table column names must be unique",
            ));
        }
        columns.push(value);
        decoded_column_nodes.push(column_node);
    }
    let rows_node = fields.get("rows").expect("validated rows field");
    let JsonKind::Array(row_nodes) = &rows_node.kind else {
        return Err(failure(
            "JSON_TABLE_COMPARE_ROWS",
            "table rows must be an array",
        ));
    };
    if row_nodes.len() > 4096 {
        return Err(failure(
            "JSON_TABLE_COMPARE_ROWS",
            "table exceeds 4096 rows",
        ));
    }
    let mut rows = Vec::with_capacity(row_nodes.len());
    let mut cells = columns.len();
    for row_node in row_nodes {
        let JsonKind::Array(cell_nodes) = &row_node.kind else {
            return Err(failure(
                "JSON_TABLE_COMPARE_ROWS",
                "each table row must be an array",
            ));
        };
        if cell_nodes.len() != columns.len() {
            return Err(failure(
                "JSON_TABLE_COMPARE_ROWS",
                "every row must match the named column width",
            ));
        }
        cells = cells.saturating_add(cell_nodes.len());
        if cells > 65_536 {
            return Err(failure(
                "JSON_TABLE_COMPARE_CELL_LIMIT",
                "table exceeds 65536 cells",
            ));
        }
        for cell in cell_nodes {
            let _ = json_string_value(document, cell, "JSON_TABLE_COMPARE_CELL")?;
        }
        rows.push(cell_nodes.iter().collect());
    }
    Ok(JsonStringTableView {
        columns,
        column_nodes: decoded_column_nodes,
        rows,
        row_nodes: row_nodes.iter().collect(),
    })
}

/// Select rows from one strict JSON string-table object using an exact decoded
/// string equality and retain original source byte ranges for decisions and
/// emitted cells.
pub fn json_table_filter(
    document: &str,
    column: &str,
    equals: &str,
) -> Result<JsonTableFilterReport> {
    if document.len() > MAX_JSON_COMPARE_BYTES {
        return Err(failure(
            "JSON_TABLE_FILTER_INPUT_LIMIT",
            "table document exceeds 256000 source bytes",
        ));
    }
    let root = JsonParser::new(document).parse()?;
    let table = json_string_table(document, &root)?;
    let column_index = table
        .columns
        .iter()
        .position(|name| name == column)
        .ok_or_else(|| failure("JSON_TABLE_FILTER_COLUMN", "named column does not exist"))?;

    let mut output_budget = 1024usize;
    let account = |budget: &mut usize, bytes: usize| -> Result<()> {
        *budget = budget.saturating_add(bytes);
        if *budget > MAX_JSON_COMPARE_OUTPUT_BYTES {
            return Err(failure(
                "JSON_TABLE_FILTER_OUTPUT_LIMIT",
                "filtered table and source evidence exceed the 256000-byte output envelope",
            ));
        }
        Ok(())
    };
    account(&mut output_budget, serde_json::to_vec(column)?.len())?;
    account(&mut output_budget, serde_json::to_vec(equals)?.len())?;
    let column_source_spans = table
        .column_nodes
        .iter()
        .zip(&table.columns)
        .map(|(node, name)| {
            let encoded_len = serde_json::to_vec(name)?.len();
            account(&mut output_budget, 192 + encoded_len)?;
            Ok(json_comparison_span(document, node))
        })
        .collect::<Result<Vec<_>>>()?;

    let mut rows = Vec::new();
    let mut source_row_indices = Vec::new();
    let mut cell_source_spans = Vec::new();
    let mut decisions = Vec::with_capacity(table.rows.len());
    for (source_row_index, row) in table.rows.iter().enumerate() {
        let value = json_string_value(document, row[column_index], "JSON_TABLE_FILTER_CELL")?;
        let matched = value == equals;
        let encoded_value_len = serde_json::to_vec(&value)?.len();
        account(&mut output_budget, 256 + encoded_value_len)?;
        decisions.push(JsonTableFilterDecision {
            source_row_index,
            value,
            matched,
            source: json_comparison_span(document, row[column_index]),
        });
        if matched {
            let mut output_row = Vec::with_capacity(row.len());
            let mut output_spans = Vec::with_capacity(row.len());
            for cell in row {
                let value = json_string_value(document, cell, "JSON_TABLE_FILTER_CELL")?;
                let encoded_len = serde_json::to_vec(&value)?.len();
                account(&mut output_budget, 216 + encoded_len)?;
                output_row.push(value);
                output_spans.push(json_comparison_span(document, cell));
            }
            rows.push(output_row);
            source_row_indices.push(source_row_index);
            cell_source_spans.push(output_spans);
        }
    }
    let report = JsonTableFilterReport {
        source_sha256: digest(document.as_bytes()),
        columns: table.columns,
        column_source_spans,
        rows,
        source_row_indices,
        cell_source_spans,
        filter_column: column.into(),
        filter_column_index: column_index,
        equals: equals.into(),
        decisions,
        guarantee: "bounded exact-string filtering over an explicitly named, JSON string-typed table; no trimming, numeric conversion, case folding, Unicode normalization or factual inference; source byte ranges retained".into(),
    };
    if serde_json::to_vec(&report)?.len() > MAX_JSON_COMPARE_OUTPUT_BYTES {
        return Err(failure(
            "JSON_TABLE_FILTER_OUTPUT_LIMIT",
            "filtered table report exceeds 256000 output bytes",
        ));
    }
    Ok(report)
}

/// Stably sort rows from one strict JSON string-table object by an exact
/// decoded string column, retaining each output cell's original byte range.
pub fn json_table_sort(
    document: &str,
    column: &str,
    descending: bool,
) -> Result<JsonTableSortReport> {
    if document.len() > MAX_JSON_COMPARE_BYTES {
        return Err(failure(
            "JSON_TABLE_SORT_INPUT_LIMIT",
            "table document exceeds 256000 source bytes",
        ));
    }
    let root = JsonParser::new(document).parse()?;
    let table = json_string_table(document, &root)?;
    let column_index = table
        .columns
        .iter()
        .position(|name| name == column)
        .ok_or_else(|| failure("JSON_TABLE_SORT_COLUMN", "named column does not exist"))?;
    let mut sort_keys = Vec::with_capacity(table.rows.len());
    for row in &table.rows {
        sort_keys.push(json_string_value(
            document,
            row[column_index],
            "JSON_TABLE_SORT_CELL",
        )?);
    }
    let mut order: Vec<usize> = (0..table.rows.len()).collect();
    order.sort_by(|left, right| {
        let order = if descending {
            sort_keys[*right].cmp(&sort_keys[*left])
        } else {
            sort_keys[*left].cmp(&sort_keys[*right])
        };
        order.then_with(|| left.cmp(right))
    });

    let mut output_budget = 1024usize;
    let account = |budget: &mut usize, bytes: usize| -> Result<()> {
        *budget = budget.saturating_add(bytes);
        if *budget > MAX_JSON_COMPARE_OUTPUT_BYTES {
            return Err(failure(
                "JSON_TABLE_SORT_OUTPUT_LIMIT",
                "sorted table and source evidence exceed the 256000-byte output envelope",
            ));
        }
        Ok(())
    };
    account(&mut output_budget, serde_json::to_vec(column)?.len())?;
    let column_source_spans = table
        .column_nodes
        .iter()
        .zip(&table.columns)
        .map(|(node, name)| {
            let encoded_len = serde_json::to_vec(name)?.len();
            account(&mut output_budget, 192 + encoded_len)?;
            Ok(json_comparison_span(document, node))
        })
        .collect::<Result<Vec<_>>>()?;

    let mut rows = Vec::with_capacity(order.len());
    let mut source_row_indices = Vec::with_capacity(order.len());
    let mut cell_source_spans = Vec::with_capacity(order.len());
    for source_row_index in order {
        let mut output_row = Vec::with_capacity(table.rows[source_row_index].len());
        let mut output_spans = Vec::with_capacity(table.rows[source_row_index].len());
        for cell in &table.rows[source_row_index] {
            let value = json_string_value(document, cell, "JSON_TABLE_SORT_CELL")?;
            let encoded_len = serde_json::to_vec(&value)?.len();
            account(&mut output_budget, 216 + encoded_len)?;
            output_row.push(value);
            output_spans.push(json_comparison_span(document, cell));
        }
        rows.push(output_row);
        source_row_indices.push(source_row_index);
        cell_source_spans.push(output_spans);
    }
    let report = JsonTableSortReport {
        source_sha256: digest(document.as_bytes()),
        columns: table.columns,
        column_source_spans,
        rows,
        source_row_indices,
        cell_source_spans,
        sort_column: column.into(),
        sort_column_index: column_index,
        sort_order: if descending {
            "descending"
        } else {
            "ascending"
        }
        .into(),
        guarantee: "bounded stable sort of an explicitly named JSON string-table column by exact decoded strings; ties preserve source order; no locale collation, case folding, Unicode normalization, numeric conversion or factual inference; source byte ranges retained".into(),
    };
    if serde_json::to_vec(&report)?.len() > MAX_JSON_COMPARE_OUTPUT_BYTES {
        return Err(failure(
            "JSON_TABLE_SORT_OUTPUT_LIMIT",
            "sorted table report exceeds 256000 output bytes",
        ));
    }
    Ok(report)
}

/// Count exact decoded string values in a named JSON string-table column.
/// Groups follow the first row in which each value appears.
pub fn json_table_group_count(document: &str, column: &str) -> Result<JsonTableGroupCountReport> {
    if document.len() > MAX_JSON_COMPARE_BYTES {
        return Err(failure(
            "JSON_TABLE_COUNT_INPUT_LIMIT",
            "table document exceeds 256000 source bytes",
        ));
    }
    let root = JsonParser::new(document).parse()?;
    let table = json_string_table(document, &root)?;
    let column_index = table
        .columns
        .iter()
        .position(|name| name == column)
        .ok_or_else(|| failure("JSON_TABLE_COUNT_COLUMN", "named column does not exist"))?;

    let mut group_indices = BTreeMap::<String, usize>::new();
    let mut members = Vec::<(String, Vec<(usize, &JsonNode)>)>::new();
    for (row_index, row) in table.rows.iter().enumerate() {
        let value = json_string_value(document, row[column_index], "JSON_TABLE_COUNT_CELL")?;
        let group_index = if let Some(index) = group_indices.get(&value) {
            *index
        } else {
            let index = members.len();
            group_indices.insert(value.clone(), index);
            members.push((value.clone(), Vec::new()));
            index
        };
        members[group_index].1.push((row_index, row[column_index]));
    }

    let mut output_budget = 1024usize;
    let account = |budget: &mut usize, bytes: usize| -> Result<()> {
        *budget = budget.saturating_add(bytes);
        if *budget > MAX_JSON_COMPARE_OUTPUT_BYTES {
            return Err(failure(
                "JSON_TABLE_COUNT_OUTPUT_LIMIT",
                "group report and source evidence exceed the 256000-byte output envelope",
            ));
        }
        Ok(())
    };
    account(&mut output_budget, serde_json::to_vec(column)?.len())?;
    account(
        &mut output_budget,
        json_comparison_span(document, table.column_nodes[column_index])
            .source_sha256
            .len()
            + 192,
    )?;
    let mut groups = Vec::with_capacity(members.len());
    for (value, group_members) in members {
        let encoded_len = serde_json::to_vec(&value)?.len();
        account(&mut output_budget, 256 + encoded_len)?;
        let mut source_row_indices = Vec::with_capacity(group_members.len());
        let mut member_source_spans = Vec::with_capacity(group_members.len());
        for (row_index, node) in &group_members {
            account(&mut output_budget, 224)?;
            source_row_indices.push(*row_index);
            member_source_spans.push(json_comparison_span(document, node));
        }
        groups.push(JsonTableGroup {
            value,
            count: group_members.len(),
            source_row_indices,
            member_source_spans,
        });
    }
    let report = JsonTableGroupCountReport {
        source_sha256: digest(document.as_bytes()),
        group_by: column.into(),
        group_by_source: json_comparison_span(document, table.column_nodes[column_index]),
        group_by_index: column_index,
        row_count: table.rows.len(),
        groups,
        guarantee: "bounded exact-string counts over an explicitly named JSON string-table column; groups retain first-seen order and every member's source range; no numeric inference, case folding, Unicode normalization or factual inference".into(),
    };
    if serde_json::to_vec(&report)?.len() > MAX_JSON_COMPARE_OUTPUT_BYTES {
        return Err(failure(
            "JSON_TABLE_COUNT_OUTPUT_LIMIT",
            "group report exceeds 256000 output bytes",
        ));
    }
    Ok(report)
}

/// Compute exact descriptive statistics for one explicitly named numeric
/// column within each exact decoded string group of a strict JSON string table.
pub fn json_table_group_statistics(
    document: &str,
    group_column: &str,
    value_column: &str,
) -> Result<JsonTableGroupStatisticsReport> {
    static QUANTITY: OnceLock<Regex> = OnceLock::new();
    let quantity = QUANTITY.get_or_init(|| {
        Regex::new(
            r"^\s*([+-]?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][+-]?[0-9]+)?)\s*([A-Za-zμµΩ°%][A-Za-z0-9μµΩ°%*/^._-]{0,63})\s*$",
        )
        .expect("constant JSON table grouped-statistics quantity pattern")
    });
    if document.len() > MAX_JSON_COMPARE_BYTES {
        return Err(failure(
            "JSON_TABLE_GROUP_STATISTICS_INPUT_LIMIT",
            "table document exceeds 256000 source bytes",
        ));
    }
    let root = JsonParser::new(document).parse()?;
    let table = json_string_table(document, &root)?;
    if table.rows.is_empty() {
        return Err(failure(
            "JSON_TABLE_GROUP_STATISTICS_EMPTY",
            "grouped statistics require at least one data row",
        ));
    }
    let group_index = table
        .columns
        .iter()
        .position(|name| name == group_column)
        .ok_or_else(|| {
            failure(
                "JSON_TABLE_GROUP_STATISTICS_COLUMN",
                "group column does not exist",
            )
        })?;
    let value_index = table
        .columns
        .iter()
        .position(|name| name == value_column)
        .ok_or_else(|| {
            failure(
                "JSON_TABLE_GROUP_STATISTICS_COLUMN",
                "value column does not exist",
            )
        })?;
    if group_index == value_index {
        return Err(failure(
            "JSON_TABLE_GROUP_STATISTICS_COLUMNS",
            "group and measured-value columns must be distinct",
        ));
    }

    let mut group_indices = BTreeMap::<String, usize>::new();
    let mut grouped = Vec::<(String, Vec<(usize, &'_ JsonNode, &'_ JsonNode)>)>::new();
    for (row_index, row) in table.rows.iter().enumerate() {
        let value = json_string_value(
            document,
            row[group_index],
            "JSON_TABLE_GROUP_STATISTICS_CELL",
        )?;
        let group_index_in_result = if let Some(index) = group_indices.get(&value) {
            *index
        } else {
            let index = grouped.len();
            group_indices.insert(value.clone(), index);
            grouped.push((value, Vec::new()));
            index
        };
        grouped[group_index_in_result]
            .1
            .push((row_index, row[group_index], row[value_index]));
    }

    let mut output_budget = 1024usize;
    let account = |budget: &mut usize, bytes: usize| -> Result<()> {
        *budget = budget.saturating_add(bytes);
        if *budget > MAX_JSON_COMPARE_OUTPUT_BYTES {
            return Err(failure(
                "JSON_TABLE_GROUP_STATISTICS_OUTPUT_LIMIT",
                "group report and source evidence exceed the 256000-byte output envelope",
            ));
        }
        Ok(())
    };
    account(&mut output_budget, serde_json::to_vec(group_column)?.len())?;
    account(&mut output_budget, serde_json::to_vec(value_column)?.len())?;
    account(
        &mut output_budget,
        json_comparison_span(document, table.column_nodes[group_index])
            .source_sha256
            .len()
            + json_comparison_span(document, table.column_nodes[value_index])
                .source_sha256
                .len()
            + 384,
    )?;

    let mut groups = Vec::with_capacity(grouped.len());
    for (value, members) in grouped {
        account(&mut output_budget, serde_json::to_vec(&value)?.len() + 256)?;
        let mut values = Vec::with_capacity(members.len());
        let mut units = Vec::with_capacity(members.len());
        let mut quantity_count = 0usize;
        let mut source_row_indices = Vec::with_capacity(members.len());
        let mut group_source_spans = Vec::with_capacity(members.len());
        let mut value_source_spans = Vec::with_capacity(members.len());
        for (row_index, group_node, value_node) in &members {
            let measured =
                json_string_value(document, value_node, "JSON_TABLE_GROUP_STATISTICS_CELL")?;
            source_row_indices.push(*row_index);
            group_source_spans.push(json_comparison_span(document, group_node));
            value_source_spans.push(json_comparison_span(document, value_node));
            if let Some(capture) = quantity.captures(&measured) {
                quantity_count += 1;
                values.push(capture[1].trim().to_string());
                units.push(capture[2].to_string());
            } else {
                values.push(measured);
                units.push(String::new());
            }
        }
        if quantity_count != 0 && quantity_count != members.len() {
            return Err(failure(
                "JSON_TABLE_GROUP_STATISTICS_MIXED_VALUES",
                format!(
                    "group {value:?} mixes unit-bearing numbers with unitless or nonnumeric cells"
                ),
            ));
        }
        let statistics = if quantity_count == members.len() {
            crate::math::summarize(&values, Some(&units))?
        } else {
            crate::math::summarize(&values, None)?
        };
        for _ in 0..members.len() {
            account(&mut output_budget, 448)?;
        }
        groups.push(JsonTableGroupedStatistics {
            value,
            source_row_indices,
            group_source_spans,
            value_source_spans,
            statistics,
        });
    }
    let report = JsonTableGroupStatisticsReport {
        source_sha256: digest(document.as_bytes()),
        group_by: group_column.into(),
        group_by_index: group_index,
        group_by_source: json_comparison_span(document, table.column_nodes[group_index]),
        value_column: value_column.into(),
        value_column_index: value_index,
        value_column_source: json_comparison_span(document, table.column_nodes[value_index]),
        row_count: table.rows.len(),
        groups,
        guarantee: "bounded exact descriptive statistics grouped by exact decoded JSON string values; groups retain first-seen order and original key/value source ranges; no numeric inference for unselected columns, normalization or population inference".into(),
    };
    if serde_json::to_vec(&report)?.len() > MAX_JSON_COMPARE_OUTPUT_BYTES {
        return Err(failure(
            "JSON_TABLE_GROUP_STATISTICS_OUTPUT_LIMIT",
            "group statistics report exceeds 256000 output bytes",
        ));
    }
    Ok(report)
}

/// Select one explicitly named column from a strict JSON string table and
/// retain the source range for the header and every returned cell.
pub fn json_table_select(document: &str, column: &str) -> Result<JsonTableSelectReport> {
    if document.len() > MAX_JSON_COMPARE_BYTES {
        return Err(failure(
            "JSON_TABLE_SELECT_INPUT_LIMIT",
            "table document exceeds 256000 source bytes",
        ));
    }
    let root = JsonParser::new(document).parse()?;
    let table = json_string_table(document, &root)?;
    let column_index = table
        .columns
        .iter()
        .position(|name| name == column)
        .ok_or_else(|| failure("JSON_TABLE_SELECT_COLUMN", "named column does not exist"))?;
    let mut output_budget = 1024usize;
    let account = |budget: &mut usize, bytes: usize| -> Result<()> {
        *budget = budget.saturating_add(bytes);
        if *budget > MAX_JSON_COMPARE_OUTPUT_BYTES {
            return Err(failure(
                "JSON_TABLE_SELECT_OUTPUT_LIMIT",
                "selected values and source evidence exceed the 256000-byte output envelope",
            ));
        }
        Ok(())
    };
    account(&mut output_budget, serde_json::to_vec(column)?.len())?;
    let column_source = json_comparison_span(document, table.column_nodes[column_index]);
    account(&mut output_budget, 192)?;
    let mut values = Vec::with_capacity(table.rows.len());
    let mut source_row_indices = Vec::with_capacity(table.rows.len());
    let mut source_spans = Vec::with_capacity(table.rows.len());
    for (source_row_index, row) in table.rows.iter().enumerate() {
        let cell = row[column_index];
        let value = json_string_value(document, cell, "JSON_TABLE_SELECT_CELL")?;
        account(&mut output_budget, 192 + serde_json::to_vec(&value)?.len())?;
        values.push(value);
        source_row_indices.push(source_row_index);
        source_spans.push(json_comparison_span(document, cell));
    }
    let report = JsonTableSelectReport {
        source_sha256: digest(document.as_bytes()),
        column_name: column.into(),
        column_index,
        column_source,
        values,
        source_row_indices,
        source_spans,
        guarantee: "bounded selection from an explicitly named JSON string-table column; values remain exact decoded strings; source byte ranges retained; no numeric inference or factual inference".into(),
    };
    if serde_json::to_vec(&report)?.len() > MAX_JSON_COMPARE_OUTPUT_BYTES {
        return Err(failure(
            "JSON_TABLE_SELECT_OUTPUT_LIMIT",
            "selected column report exceeds 256000 output bytes",
        ));
    }
    Ok(report)
}

/// Project a strict JSON string table onto a nonempty ordered list of unique
/// named columns, retaining source locations for each projected value.
pub fn json_table_project(document: &str, columns: &[String]) -> Result<JsonTableProjectReport> {
    if document.len() > MAX_JSON_COMPARE_BYTES {
        return Err(failure(
            "JSON_TABLE_PROJECT_INPUT_LIMIT",
            "table document exceeds 256000 source bytes",
        ));
    }
    if columns.is_empty() || columns.len() > 256 {
        return Err(failure(
            "JSON_TABLE_PROJECT_COLUMNS",
            "projection requires 1..256 named columns",
        ));
    }
    let root = JsonParser::new(document).parse()?;
    let table = json_string_table(document, &root)?;
    let mut seen = BTreeSet::new();
    let mut selected_column_indices = Vec::with_capacity(columns.len());
    for column in columns {
        if !seen.insert(column) {
            return Err(failure(
                "JSON_TABLE_PROJECT_DUPLICATE_COLUMN",
                "projection column names must be unique",
            ));
        }
        selected_column_indices.push(
            table
                .columns
                .iter()
                .position(|name| name == column)
                .ok_or_else(|| {
                    failure(
                        "JSON_TABLE_PROJECT_COLUMN",
                        format!("named column {column:?} does not exist"),
                    )
                })?,
        );
    }

    let mut output_budget = 1024usize;
    let account = |budget: &mut usize, bytes: usize| -> Result<()> {
        *budget = budget.saturating_add(bytes);
        if *budget > MAX_JSON_COMPARE_OUTPUT_BYTES {
            return Err(failure(
                "JSON_TABLE_PROJECT_OUTPUT_LIMIT",
                "projected table and source evidence exceed the 256000-byte output envelope",
            ));
        }
        Ok(())
    };
    let column_source_spans = selected_column_indices
        .iter()
        .map(|index| {
            let name = &table.columns[*index];
            account(&mut output_budget, serde_json::to_vec(name)?.len() + 192)?;
            Ok(json_comparison_span(document, table.column_nodes[*index]))
        })
        .collect::<Result<Vec<_>>>()?;

    let mut rows = Vec::with_capacity(table.rows.len());
    let mut source_row_indices = Vec::with_capacity(table.rows.len());
    let mut cell_source_spans = Vec::with_capacity(table.rows.len());
    for (source_row_index, row) in table.rows.iter().enumerate() {
        let mut projected_row = Vec::with_capacity(selected_column_indices.len());
        let mut projected_spans = Vec::with_capacity(selected_column_indices.len());
        for column_index in &selected_column_indices {
            let cell = row[*column_index];
            let value = json_string_value(document, cell, "JSON_TABLE_PROJECT_CELL")?;
            account(&mut output_budget, serde_json::to_vec(&value)?.len() + 216)?;
            projected_row.push(value);
            projected_spans.push(json_comparison_span(document, cell));
        }
        rows.push(projected_row);
        source_row_indices.push(source_row_index);
        cell_source_spans.push(projected_spans);
    }
    let report = JsonTableProjectReport {
        source_sha256: digest(document.as_bytes()),
        columns: columns.to_vec(),
        selected_column_indices,
        column_source_spans,
        rows,
        source_row_indices,
        cell_source_spans,
        guarantee: "bounded projection of an explicitly named JSON string table; requested column order and original row order are preserved; values remain exact decoded strings; source ranges retained; no numeric or factual inference".into(),
    };
    if serde_json::to_vec(&report)?.len() > MAX_JSON_COMPARE_OUTPUT_BYTES {
        return Err(failure(
            "JSON_TABLE_PROJECT_OUTPUT_LIMIT",
            "projected table report exceeds 256000 output bytes",
        ));
    }
    Ok(report)
}

fn record_json_table_change(
    mut change: JsonTableChange,
    left_node: Option<&JsonNode>,
    right_node: Option<&JsonNode>,
    document: &str,
    changes: &mut Vec<JsonTableChange>,
    output_budget: &mut usize,
) -> Result<()> {
    if changes.len() >= MAX_JSON_COMPARE_DIFFERENCES {
        return Err(failure(
            "JSON_TABLE_COMPARE_DIFFERENCE_LIMIT",
            "comparison exceeds 4096 recorded changes",
        ));
    }
    let estimate = change.key.len()
        + change.column.as_ref().map_or(0, String::len)
        + change.left_value.as_ref().map_or(0, String::len)
        + change.right_value.as_ref().map_or(0, String::len)
        + change.left_pointer.as_ref().map_or(0, String::len)
        + change.right_pointer.as_ref().map_or(0, String::len)
        + 512;
    *output_budget = (*output_budget).saturating_add(estimate);
    if *output_budget > MAX_JSON_COMPARE_OUTPUT_BYTES {
        return Err(failure(
            "JSON_TABLE_COMPARE_OUTPUT_LIMIT",
            "comparison report exceeds its bounded output envelope",
        ));
    }
    change.left = left_node.map(|node| json_comparison_span(document, node));
    change.right = right_node.map(|node| json_comparison_span(document, node));
    changes.push(change);
    Ok(())
}

/// Compare string-table JSON sides by one unique exact-string key column.
/// Wrapper shape is `{"left":{"columns":[...],"rows":[...]},"right":...}`.
pub fn json_table_compare(document: &str, key_column: &str) -> Result<JsonTableComparisonReport> {
    if document.len() > MAX_JSON_COMPARE_BYTES {
        return Err(failure(
            "JSON_TABLE_COMPARE_INPUT_LIMIT",
            "comparison wrapper exceeds 256000 source bytes",
        ));
    }
    let root = JsonParser::new(document).parse()?;
    let JsonKind::Object(wrapper) = &root.kind else {
        return Err(failure(
            "JSON_TABLE_COMPARE_SHAPE",
            "comparison wrapper must be an object with left and right tables",
        ));
    };
    if wrapper.len() != 2 || !wrapper.contains_key("left") || !wrapper.contains_key("right") {
        return Err(failure(
            "JSON_TABLE_COMPARE_SHAPE",
            "comparison wrapper requires exactly left and right fields",
        ));
    }
    let left_node = wrapper.get("left").expect("validated left field");
    let right_node = wrapper.get("right").expect("validated right field");
    let left = json_string_table(document, left_node)?;
    let right = json_string_table(document, right_node)?;
    if left.columns != right.columns {
        return Err(failure(
            "JSON_TABLE_COMPARE_SCHEMA",
            "both tables must have identical decoded column names in the same order",
        ));
    }
    let key_index = left
        .columns
        .iter()
        .position(|column| column == key_column)
        .ok_or_else(|| failure("JSON_TABLE_COMPARE_KEY", "key column does not exist"))?;

    let mut left_keys = BTreeMap::new();
    for (row_index, row) in left.rows.iter().enumerate() {
        let key = json_string_value(document, row[key_index], "JSON_TABLE_COMPARE_KEY")?;
        if left_keys.insert(key, row_index).is_some() {
            return Err(failure(
                "JSON_TABLE_COMPARE_DUPLICATE_KEY",
                "left table key values must be unique",
            ));
        }
    }
    let mut right_keys = BTreeMap::new();
    for (row_index, row) in right.rows.iter().enumerate() {
        let key = json_string_value(document, row[key_index], "JSON_TABLE_COMPARE_KEY")?;
        if right_keys.insert(key, row_index).is_some() {
            return Err(failure(
                "JSON_TABLE_COMPARE_DUPLICATE_KEY",
                "right table key values must be unique",
            ));
        }
    }

    let mut changes = Vec::new();
    let mut output_budget = 512usize;
    for (left_index, left_row) in left.rows.iter().enumerate() {
        let key = json_string_value(document, left_row[key_index], "JSON_TABLE_COMPARE_KEY")?;
        let Some(right_index) = right_keys.get(&key).copied() else {
            record_json_table_change(
                JsonTableChange {
                    change: "ROW_REMOVED".into(),
                    key,
                    column: None,
                    left_value: None,
                    right_value: None,
                    left_pointer: Some(format!("/left/rows/{left_index}")),
                    right_pointer: None,
                    left: None,
                    right: None,
                },
                Some(left.row_nodes[left_index]),
                None,
                document,
                &mut changes,
                &mut output_budget,
            )?;
            continue;
        };
        let right_row = &right.rows[right_index];
        for column_index in 0..left.columns.len() {
            if column_index == key_index {
                continue;
            }
            let left_value =
                json_string_value(document, left_row[column_index], "JSON_TABLE_COMPARE_CELL")?;
            let right_value =
                json_string_value(document, right_row[column_index], "JSON_TABLE_COMPARE_CELL")?;
            if left_value != right_value {
                record_json_table_change(
                    JsonTableChange {
                        change: "CELL_CHANGED".into(),
                        key: key.clone(),
                        column: Some(left.columns[column_index].clone()),
                        left_value: Some(left_value),
                        right_value: Some(right_value),
                        left_pointer: Some(format!("/left/rows/{left_index}/{column_index}")),
                        right_pointer: Some(format!("/right/rows/{right_index}/{column_index}")),
                        left: None,
                        right: None,
                    },
                    Some(left_row[column_index]),
                    Some(right_row[column_index]),
                    document,
                    &mut changes,
                    &mut output_budget,
                )?;
            }
        }
    }
    for (right_index, right_row) in right.rows.iter().enumerate() {
        let key = json_string_value(document, right_row[key_index], "JSON_TABLE_COMPARE_KEY")?;
        if !left_keys.contains_key(&key) {
            record_json_table_change(
                JsonTableChange {
                    change: "ROW_ADDED".into(),
                    key,
                    column: None,
                    left_value: None,
                    right_value: None,
                    left_pointer: None,
                    right_pointer: Some(format!("/right/rows/{right_index}")),
                    left: None,
                    right: None,
                },
                None,
                Some(right.row_nodes[right_index]),
                document,
                &mut changes,
                &mut output_budget,
            )?;
        }
    }
    let report = JsonTableComparisonReport {
        source_sha256: digest(document.as_bytes()),
        left_sha256: digest(document[left_node.span.clone()].as_bytes()),
        right_sha256: digest(document[right_node.span.clone()].as_bytes()),
        key_column: key_column.into(),
        equal: changes.is_empty(),
        changes,
        guarantee: "bounded keyed comparison of explicitly string-typed tables; exact key/value matching, identical column order and unique keys required; source ranges retained; no semantic truth inferred".into(),
    };
    if serde_json::to_vec(&report)?.len() > MAX_JSON_COMPARE_OUTPUT_BYTES {
        return Err(failure(
            "JSON_TABLE_COMPARE_OUTPUT_LIMIT",
            "comparison report exceeds 256000 output bytes",
        ));
    }
    Ok(report)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceSourceIdentity {
    pub source_id: String,
    pub independence_group: String,
    pub source_id_span: JsonComparisonSpan,
    pub independence_group_span: JsonComparisonSpan,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceCitation {
    pub source_index: usize,
    pub claim_text_span: JsonComparisonSpan,
    pub stance_span: JsonComparisonSpan,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceClaimSynthesis {
    pub claim_id: String,
    pub text: String,
    pub status: String,
    pub consensus_stance: Option<String>,
    pub supporting_independence_group_count: usize,
    pub refuting_independence_group_count: usize,
    pub supporting: Vec<EvidenceCitation>,
    pub refuting: Vec<EvidenceCitation>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeclaredInference {
    pub inference_id: String,
    pub text: String,
    pub premise_ids: Vec<String>,
    pub classification: String,
    pub inference_id_span: JsonComparisonSpan,
    pub text_span: JsonComparisonSpan,
    pub premise_spans: Vec<JsonComparisonSpan>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExplicitEvidenceSynthesisReport {
    pub source_sha256: String,
    pub source_count: usize,
    pub evidence_record_count: usize,
    pub sources: Vec<EvidenceSourceIdentity>,
    pub claims: Vec<EvidenceClaimSynthesis>,
    pub declared_inferences: Vec<DeclaredInference>,
    pub independence_basis: String,
    pub guarantee: String,
}

#[derive(Default)]
struct EvidenceClaimAccumulator {
    text: String,
    supporting: Vec<EvidenceCitation>,
    refuting: Vec<EvidenceCitation>,
}

fn evidence_object<'a>(
    node: &'a JsonNode,
    fields: &[&str],
    code: &str,
) -> Result<&'a BTreeMap<String, JsonNode>> {
    let JsonKind::Object(entries) = &node.kind else {
        return Err(failure(code, "expected a JSON object"));
    };
    if entries.len() != fields.len() || fields.iter().any(|field| !entries.contains_key(*field)) {
        return Err(failure(code, "object has missing or unknown fields"));
    }
    Ok(entries)
}

fn evidence_array<'a>(node: &'a JsonNode, code: &str) -> Result<&'a [JsonNode]> {
    let JsonKind::Array(entries) = &node.kind else {
        return Err(failure(code, "expected a JSON array"));
    };
    Ok(entries)
}

fn evidence_nonempty_label(value: &str, code: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        return Err(failure(
            code,
            "identifier/group labels must be nonempty, single-line strings of at most 256 bytes",
        ));
    }
    Ok(())
}

const MAX_EVIDENCE_SYNTHESIS_BYTES: usize = 256_000;
const MAX_EVIDENCE_SOURCES: usize = 128;
const MAX_EVIDENCE_RECORDS: usize = 4096;
const MAX_DECLARED_INFERENCES: usize = 512;
const MAX_EVIDENCE_SYNTHESIS_OUTPUT: usize = 256_000;

/// Classify only a caller-supplied evidence ledger. Independence groups and
/// inference links are explicit metadata, not independently verified claims.
pub fn synthesize_explicit_evidence(document: &str) -> Result<ExplicitEvidenceSynthesisReport> {
    if document.len() > MAX_EVIDENCE_SYNTHESIS_BYTES {
        return Err(failure(
            "EVIDENCE_SYNTHESIS_INPUT_LIMIT",
            "evidence ledger exceeds 256000 source bytes",
        ));
    }
    let root = JsonParser::new(document).parse()?;
    let wrapper = evidence_object(&root, &["sources", "inferences"], "EVIDENCE_LEDGER_SHAPE")?;
    let source_nodes = evidence_array(
        wrapper.get("sources").expect("validated sources field"),
        "EVIDENCE_LEDGER_SOURCES",
    )?;
    if !(2..=MAX_EVIDENCE_SOURCES).contains(&source_nodes.len()) {
        return Err(failure(
            "EVIDENCE_LEDGER_SOURCE_LIMIT",
            "expected 2..128 explicit source records",
        ));
    }

    let mut sources = Vec::with_capacity(source_nodes.len());
    let mut source_ids = BTreeSet::new();
    let mut claim_labels = BTreeMap::<String, String>::new();
    let mut grouped = BTreeMap::<String, EvidenceClaimAccumulator>::new();
    let mut record_count = 0usize;
    let mut output_estimate = 1024usize;

    for (source_index, source_node) in source_nodes.iter().enumerate() {
        let source_fields = evidence_object(
            source_node,
            &["source_id", "independence_group", "claims"],
            "EVIDENCE_SOURCE_SHAPE",
        )?;
        let source_id_node = source_fields
            .get("source_id")
            .expect("validated source id field");
        let group_node = source_fields
            .get("independence_group")
            .expect("validated independence group field");
        let source_id = json_string_value(document, source_id_node, "EVIDENCE_SOURCE_ID")?;
        let independence_group =
            json_string_value(document, group_node, "EVIDENCE_INDEPENDENCE_GROUP")?;
        evidence_nonempty_label(&source_id, "EVIDENCE_SOURCE_ID")?;
        evidence_nonempty_label(&independence_group, "EVIDENCE_INDEPENDENCE_GROUP")?;
        if !source_ids.insert(source_id.clone()) {
            return Err(failure(
                "EVIDENCE_DUPLICATE_SOURCE_ID",
                "source identifiers must be unique",
            ));
        }
        output_estimate = output_estimate
            .saturating_add(source_id.len())
            .saturating_add(independence_group.len())
            .saturating_add(512);
        if output_estimate > MAX_EVIDENCE_SYNTHESIS_OUTPUT {
            return Err(failure(
                "EVIDENCE_SYNTHESIS_OUTPUT_LIMIT",
                "evidence report exceeds its bounded output envelope",
            ));
        }
        sources.push(EvidenceSourceIdentity {
            source_id,
            independence_group,
            source_id_span: json_comparison_span(document, source_id_node),
            independence_group_span: json_comparison_span(document, group_node),
        });

        let claims = evidence_array(
            source_fields.get("claims").expect("validated claims field"),
            "EVIDENCE_SOURCE_CLAIMS",
        )?;
        if claims.is_empty() {
            return Err(failure(
                "EVIDENCE_SOURCE_NO_CLAIMS",
                "each explicit source must provide at least one claim record",
            ));
        }
        let mut source_claim_ids = BTreeSet::new();
        for claim_node in claims {
            record_count += 1;
            if record_count > MAX_EVIDENCE_RECORDS {
                return Err(failure(
                    "EVIDENCE_RECORD_LIMIT",
                    "evidence ledger exceeds 4096 source claim records",
                ));
            }
            let claim_fields = evidence_object(
                claim_node,
                &["id", "text", "stance"],
                "EVIDENCE_CLAIM_SHAPE",
            )?;
            let id_node = claim_fields.get("id").expect("validated claim id field");
            let text_node = claim_fields
                .get("text")
                .expect("validated claim text field");
            let stance_node = claim_fields
                .get("stance")
                .expect("validated claim stance field");
            let claim_id = json_string_value(document, id_node, "EVIDENCE_CLAIM_ID")?;
            let text = json_string_value(document, text_node, "EVIDENCE_CLAIM_TEXT")?;
            let stance = json_string_value(document, stance_node, "EVIDENCE_STANCE")?;
            evidence_nonempty_label(&claim_id, "EVIDENCE_CLAIM_ID")?;
            if text.trim().is_empty() || text.len() > 16_384 {
                return Err(failure(
                    "EVIDENCE_CLAIM_TEXT",
                    "claim text must be nonempty and at most 16384 bytes",
                ));
            }
            if !matches!(stance.as_str(), "SUPPORTS" | "REFUTES") {
                return Err(failure(
                    "EVIDENCE_STANCE",
                    "claim stance must be exactly SUPPORTS or REFUTES",
                ));
            }
            if !source_claim_ids.insert(claim_id.clone()) {
                return Err(failure(
                    "EVIDENCE_DUPLICATE_SOURCE_CLAIM",
                    "one source may state each claim id only once",
                ));
            }
            if let Some(existing) = claim_labels.get(&claim_id) {
                if existing != &text {
                    return Err(failure(
                        "EVIDENCE_CLAIM_LABEL_CONFLICT",
                        "the same claim id must use identical decoded claim text across sources",
                    ));
                }
            } else {
                output_estimate = output_estimate.saturating_add(claim_id.len() + text.len());
                claim_labels.insert(claim_id.clone(), text.clone());
            }
            output_estimate = output_estimate.saturating_add(400);
            if output_estimate > MAX_EVIDENCE_SYNTHESIS_OUTPUT {
                return Err(failure(
                    "EVIDENCE_SYNTHESIS_OUTPUT_LIMIT",
                    "evidence report exceeds its bounded output envelope",
                ));
            }
            let citation = EvidenceCitation {
                source_index,
                claim_text_span: json_comparison_span(document, text_node),
                stance_span: json_comparison_span(document, stance_node),
            };
            let accumulator = grouped.entry(claim_id).or_default();
            if accumulator.text.is_empty() {
                accumulator.text = text;
            }
            match stance.as_str() {
                "SUPPORTS" => accumulator.supporting.push(citation),
                "REFUTES" => accumulator.refuting.push(citation),
                _ => unreachable!("stance value was validated"),
            }
        }
    }
    if record_count == 0 {
        return Err(failure(
            "EVIDENCE_LEDGER_EMPTY",
            "at least one explicit claim record is required",
        ));
    }

    let mut claims = Vec::with_capacity(grouped.len());
    for (claim_id, accumulator) in grouped {
        let support_groups: BTreeSet<_> = accumulator
            .supporting
            .iter()
            .map(|citation| sources[citation.source_index].independence_group.as_str())
            .collect();
        let refute_groups: BTreeSet<_> = accumulator
            .refuting
            .iter()
            .map(|citation| sources[citation.source_index].independence_group.as_str())
            .collect();
        let (status, consensus_stance) =
            if !accumulator.supporting.is_empty() && !accumulator.refuting.is_empty() {
                ("CONTRADICTION", None)
            } else if support_groups.len() >= 2 || refute_groups.len() >= 2 {
                (
                    "AGREEMENT",
                    Some(if support_groups.len() >= 2 {
                        "SUPPORTS"
                    } else {
                        "REFUTES"
                    }),
                )
            } else if accumulator.supporting.len() + accumulator.refuting.len() > 1 {
                (
                    "SAME_GROUP_REPETITION",
                    Some(if accumulator.supporting.is_empty() {
                        "REFUTES"
                    } else {
                        "SUPPORTS"
                    }),
                )
            } else {
                (
                    "SINGLE_SOURCE",
                    Some(if accumulator.supporting.is_empty() {
                        "REFUTES"
                    } else {
                        "SUPPORTS"
                    }),
                )
            };
        claims.push(EvidenceClaimSynthesis {
            claim_id,
            text: accumulator.text,
            status: status.into(),
            consensus_stance: consensus_stance.map(str::to_owned),
            supporting_independence_group_count: support_groups.len(),
            refuting_independence_group_count: refute_groups.len(),
            supporting: accumulator.supporting,
            refuting: accumulator.refuting,
        });
    }

    let inference_nodes = evidence_array(
        wrapper
            .get("inferences")
            .expect("validated inferences field"),
        "EVIDENCE_INFERENCES",
    )?;
    if inference_nodes.len() > MAX_DECLARED_INFERENCES {
        return Err(failure(
            "EVIDENCE_INFERENCE_LIMIT",
            "evidence ledger exceeds 512 declared inferences",
        ));
    }
    let mut known_ids: BTreeSet<String> = claim_labels.keys().cloned().collect();
    let mut used_ids = known_ids.clone();
    used_ids.extend(source_ids);
    let mut declared_inferences = Vec::with_capacity(inference_nodes.len());
    for inference_node in inference_nodes {
        let inference_fields = evidence_object(
            inference_node,
            &["id", "text", "premises"],
            "EVIDENCE_INFERENCE_SHAPE",
        )?;
        let id_node = inference_fields.get("id").expect("validated inference id");
        let text_node = inference_fields
            .get("text")
            .expect("validated inference text");
        let inference_id = json_string_value(document, id_node, "EVIDENCE_INFERENCE_ID")?;
        let text = json_string_value(document, text_node, "EVIDENCE_INFERENCE_TEXT")?;
        evidence_nonempty_label(&inference_id, "EVIDENCE_INFERENCE_ID")?;
        if text.trim().is_empty() || text.len() > 16_384 {
            return Err(failure(
                "EVIDENCE_INFERENCE_TEXT",
                "inference text must be nonempty and at most 16384 bytes",
            ));
        }
        if !used_ids.insert(inference_id.clone()) {
            return Err(failure(
                "EVIDENCE_DUPLICATE_INFERENCE_ID",
                "inference ids must be unique and distinct from source/claim ids",
            ));
        }
        let premise_nodes = evidence_array(
            inference_fields
                .get("premises")
                .expect("validated premises field"),
            "EVIDENCE_INFERENCE_PREMISES",
        )?;
        if premise_nodes.is_empty() || premise_nodes.len() > 64 {
            return Err(failure(
                "EVIDENCE_INFERENCE_PREMISES",
                "each declared inference needs 1..64 earlier premises",
            ));
        }
        let mut premise_ids = Vec::with_capacity(premise_nodes.len());
        let mut premise_seen = BTreeSet::new();
        let mut premise_spans = Vec::with_capacity(premise_nodes.len());
        for premise_node in premise_nodes {
            let premise = json_string_value(document, premise_node, "EVIDENCE_INFERENCE_PREMISE")?;
            if !known_ids.contains(&premise) || !premise_seen.insert(premise.clone()) {
                return Err(failure(
                    "EVIDENCE_INFERENCE_PREMISE",
                    "premises must reference distinct claim ids or earlier inference ids",
                ));
            }
            output_estimate = output_estimate
                .saturating_add(premise.len())
                .saturating_add(128);
            premise_ids.push(premise);
            premise_spans.push(json_comparison_span(document, premise_node));
        }
        output_estimate = output_estimate
            .saturating_add(inference_id.len())
            .saturating_add(text.len())
            .saturating_add(512);
        if output_estimate > MAX_EVIDENCE_SYNTHESIS_OUTPUT {
            return Err(failure(
                "EVIDENCE_SYNTHESIS_OUTPUT_LIMIT",
                "evidence report exceeds its bounded output envelope",
            ));
        }
        known_ids.insert(inference_id.clone());
        declared_inferences.push(DeclaredInference {
            inference_id,
            text,
            premise_ids,
            classification: "USER_DECLARED_INFERENCE_NOT_VALIDATED".into(),
            inference_id_span: json_comparison_span(document, id_node),
            text_span: json_comparison_span(document, text_node),
            premise_spans,
        });
    }

    let report = ExplicitEvidenceSynthesisReport {
        source_sha256: digest(document.as_bytes()),
        source_count: sources.len(),
        evidence_record_count: record_count,
        sources,
        claims,
        declared_inferences,
        independence_basis: "independence_group labels are caller-supplied and not independently verified".into(),
        guarantee: "explicit evidence ledger classification only; source identities, claim text, stances and inference links are caller-supplied; no source fact extraction, claim truth or inference validity is asserted".into(),
    };
    if serde_json::to_vec(&report)?.len() > MAX_EVIDENCE_SYNTHESIS_OUTPUT {
        return Err(failure(
            "EVIDENCE_SYNTHESIS_OUTPUT_LIMIT",
            "evidence synthesis report exceeds 256000 output bytes",
        ));
    }
    Ok(report)
}

/// Compute exact descriptive statistics for one selected array of JSON number
/// literals. Every observation retains its original source bytes and offset.
pub fn json_pointer_statistics(document: &str, pointer: &str) -> Result<JsonStatisticsReport> {
    let root = JsonParser::new(document).parse()?;
    let node = json_node_at_pointer(&root, pointer)?;
    let JsonKind::Array(elements) = &node.kind else {
        return Err(failure(
            "JSON_STATISTICS_NOT_ARRAY",
            "the selected JSON Pointer must identify an array",
        ));
    };
    if elements.is_empty() || elements.len() > 4096 {
        return Err(failure(
            "JSON_STATISTICS_INPUT_LIMIT",
            "expected an array containing 1..4096 numeric observations",
        ));
    }
    let mut values = Vec::with_capacity(elements.len());
    let mut spans = Vec::with_capacity(elements.len() + 1);
    spans.push(JsonSourceSpan {
        kind: "JSON_ARRAY".into(),
        source_start: node.span.start,
        source_end: node.span.end,
        source_text: document[node.span.clone()].into(),
        array_index: None,
    });
    for (index, element) in elements.iter().enumerate() {
        if !matches!(&element.kind, JsonKind::Number) {
            return Err(failure(
                "JSON_STATISTICS_NON_NUMERIC",
                format!(
                    "array element {index} at byte {} is not a JSON number literal",
                    element.span.start
                ),
            ));
        }
        let source_text = &document[element.span.clone()];
        values.push(source_text.to_owned());
        spans.push(JsonSourceSpan {
            kind: "JSON_NUMBER".into(),
            source_start: element.span.start,
            source_end: element.span.end,
            source_text: source_text.into(),
            array_index: Some(index),
        });
    }
    Ok(JsonStatisticsReport {
        source_sha256: digest(document.as_bytes()),
        pointer: pointer.into(),
        statistics: crate::math::summarize(&values, None)?,
        spans,
        guarantee: "exact descriptive statistics of explicitly selected JSON number literals; source bytes retained; no population inference".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_json_numbers_and_spans_are_preserved() {
        let doc = r#"{"数":90071992547409931234567890123456789,"a/b":{"~key":[null,1.2300e+40]}}"#;
        assert_eq!(
            json_pointer(doc, "/数").unwrap(),
            "90071992547409931234567890123456789"
        );
        assert_eq!(json_pointer(doc, "/a~1b/~0key/1").unwrap(), "1.2300e+40");
        assert_eq!(json_pointer(doc, "").unwrap(), doc);
        assert!(json_pointer(doc, "/a~1b/~0key/01").is_err());
        assert!(json_pointer(doc, "/absent").is_err());
    }
    #[test]
    fn duplicates_nonfinite_and_malformed_documents_fail() {
        for doc in [
            r#"{"a":1,"\u0061":2}"#,
            r#"{"x":[{"a":1,"a":2}]}"#,
            "NaN",
            "Infinity",
            "1e",
            "01",
            "[1,]",
            r#""\ud800""#,
            r#"{"a":true}trailing"#,
        ] {
            assert!(json_pointer(doc, "").is_err(), "accepted {doc}");
        }
    }
    #[test]
    fn url_authority_and_exact_host_scope_precede_network() {
        let mut permissions = Permissions::default();
        permissions.read_hosts.insert("example.com".into());
        for url in [
            "http://example.com/",
            "https://127.1/",
            "https://[::1]/",
            "https://@example.com/",
            "https://user@example.com/",
            "https://example.com:8443/",
            "https://localhost/",
            "https://sub.example.com/",
            "https://example.com/#x",
        ] {
            assert!(approved_url(url, &permissions).is_err(), "accepted {url}");
        }
        assert!(approved_url("https://example.com/a?q=x", &permissions).is_ok());
    }
    #[test]
    fn unsupported_interpretations_are_not_body_success() {
        assert!(decode_body("text/html; charset=utf-8", b"<p>fact</p>").is_err());
        assert!(decode_body("application/pdf", b"%PDF-").is_err());
        assert!(decode_body("text/plain; charset=cp932", b"text").is_err());
        assert!(decode_body("text/plain", b"\xff").is_err());
        assert!(decode_body("application/json", br#"{"x":1,"x":2}"#).is_err());
        assert_eq!(
            decode_body("text/plain; charset=utf-8", "観測原文".as_bytes()).unwrap(),
            "観測原文"
        );
    }
    #[test]
    fn direct_private_and_transition_addresses_are_rejected() {
        for address in [
            "127.0.0.1",
            "10.0.0.1",
            "169.254.169.254",
            "100.64.0.1",
            "192.0.0.1",
            "198.19.0.1",
            "::1",
            "fc00::1",
            "fe80::1",
            "2002:7f00:1::1",
            "2001:db8::1",
            "3fff::1",
            "3fff:0fff:ffff::1",
            "::ffff:127.0.0.1",
            "::ffff:10.0.0.1",
            "::ffff:169.254.169.254",
        ] {
            assert!(
                !public_address(address.parse().unwrap()),
                "accepted {address}"
            );
        }
        assert!(public_address("1.1.1.1".parse().unwrap()));
        assert!(public_address("2606:4700:4700::1111".parse().unwrap()));
        assert!(public_address("::ffff:1.1.1.1".parse().unwrap()));
    }
    #[test]
    fn inherited_proxy_bypass_patterns_are_conservatively_recognized() {
        assert!(proxy_bypass_matches("sub.example.com", "*.example.com"));
        assert!(proxy_bypass_matches("example.com", ".EXAMPLE.com:443"));
        assert!(proxy_bypass_matches("example.com", "*"));
        assert!(!proxy_bypass_matches(
            "example.com",
            "different.example.com"
        ));
    }
}

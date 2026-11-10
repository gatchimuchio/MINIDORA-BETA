//! Bounded evidence observers and a source-preserving JSON selection adapter.
//! Direct URL retrieval is not a search provider, and retrieved bytes carry no
//! instruction or effect authority. HTML/PDF interpretation is not implemented.

use minidora_domain::{
    EvidencePacket, Permissions, ResourceBudget, Result, digest, failure, identity,
};
use reqwest::blocking::Client;
use reqwest::redirect::Policy;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::ops::Range;
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
                        let key = self.string()?;
                        self.whitespace();
                        if self.text.as_bytes().get(self.position) != Some(&b':') {
                            return Err(self.error());
                        }
                        self.position += 1;
                        let node = self.node(depth + 1)?;
                        if entries.insert(key, node).is_some() {
                            return Err(failure(
                                "DUPLICATE_JSON_KEY",
                                "Decoded JSON object keys must be unique",
                            ));
                        }
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
    JsonComparisonSpan {
        source_start: node.span.start,
        source_end: node.span.end,
        source_sha256: digest(document[node.span.clone()].as_bytes()),
    }
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

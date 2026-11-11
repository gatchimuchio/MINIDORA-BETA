//! Bounded reader and interpreter for the inherited Python-function subset.
//! This module never invokes a host Python process, compiler, `exec`, or `eval`.
use crate::ports::{JsonKind, JsonNode, strict_json_document};
use minidora_domain::{Error, Result, digest, failure};
use num_bigint::BigInt;
use num_traits::{One, Signed, ToPrimitive, Zero};
use regex::Regex;
use serde::Serialize;
use std::collections::{BTreeMap, VecDeque};
use std::sync::OnceLock;
use std::time::Instant;
use unicode_normalization::UnicodeNormalization;

const VERSION: &str = "bounded-python-function-reader-interpreter-v1";
const CALLS: &[&str] = &[
    "abs", "len", "min", "max", "sum", "range", "sorted", "list", "all", "any",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodeOperation {
    Read,
    Evaluate,
    Verify,
}

#[derive(Debug, Serialize)]
pub struct CodeCapabilityReport {
    pub version: String,
    pub operation: String,
    pub source: String,
    pub source_sha256: String,
    pub function_name: String,
    pub parameters: Vec<String>,
    pub ast_nodes: usize,
    pub ast_depth: usize,
    pub structure: Vec<CodeStructureNode>,
    pub summary: String,
    pub evaluation: Option<CodeEvaluationReport>,
    pub verification: Option<CodeVerificationReport>,
    pub assurance: String,
}

#[derive(Debug, Serialize)]
pub struct CodeStructureNode {
    pub syntax: String,
    pub line: usize,
    pub utf8_column: usize,
    pub end_line: usize,
    pub end_utf8_column: usize,
    pub source_segment: String,
}

#[derive(Debug, Serialize)]
pub struct CodeEvaluationReport {
    pub status: String,
    pub actual_json: Option<String>,
    pub result_type: Option<String>,
    pub arguments_sha256: String,
    pub steps: usize,
    pub max_steps: usize,
    pub branches: Vec<BranchObservation>,
    pub diagnostic: Option<String>,
    pub line: Option<usize>,
    #[serde(skip_serializing)]
    canonical_json: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CodeVerificationReport {
    pub cases_sha256: String,
    pub case_count: usize,
    pub all_matched: bool,
    pub cases: Vec<CodeCaseObservation>,
    pub assurance: String,
}

#[derive(Debug, Serialize)]
pub struct CodeCaseObservation {
    pub number: usize,
    pub matched: bool,
    pub evaluation_status: String,
    pub actual_json: Option<String>,
    pub expected_json: String,
    pub steps: usize,
    pub diagnostic: Option<String>,
    pub line: Option<usize>,
}

#[derive(Clone, Debug, Serialize)]
pub struct BranchObservation {
    pub line: usize,
    pub condition: bool,
}

#[derive(Clone, Debug)]
enum CodeValue {
    Null,
    Bool(bool),
    Integer(BigInt),
    String(String),
    List(Vec<CodeValue>),
    Dict(Vec<(String, CodeValue)>),
    Range {
        start: BigInt,
        step: BigInt,
        len: usize,
    },
}

impl CodeValue {
    fn type_name(&self) -> &'static str {
        match self {
            Self::Null => "NoneType",
            Self::Bool(_) => "bool",
            Self::Integer(_) => "int",
            Self::String(_) => "str",
            Self::List(_) => "list",
            Self::Dict(_) => "dict",
            Self::Range { .. } => "range",
        }
    }
    fn truthy(&self) -> bool {
        match self {
            Self::Null => false,
            Self::Bool(value) => *value,
            Self::Integer(value) => !value.is_zero(),
            Self::String(value) => !value.is_empty(),
            Self::List(value) => !value.is_empty(),
            Self::Dict(value) => !value.is_empty(),
            Self::Range { len, .. } => *len != 0,
        }
    }
    fn len(&self) -> Option<usize> {
        match self {
            Self::String(value) => Some(value.chars().count()),
            Self::List(value) => Some(value.len()),
            Self::Dict(value) => Some(value.len()),
            Self::Range { len, .. } => Some(*len),
            _ => None,
        }
    }
    fn iterable(&self) -> Option<Vec<CodeValue>> {
        match self {
            Self::String(value) => Some(
                value
                    .chars()
                    .map(|ch| Self::String(ch.to_string()))
                    .collect(),
            ),
            Self::List(value) => Some(value.clone()),
            Self::Dict(value) => Some(
                value
                    .iter()
                    .map(|(key, _)| Self::String(key.clone()))
                    .collect(),
            ),
            Self::Range { start, step, len } => Some(
                (0..*len)
                    .map(|index| Self::Integer(start + step * BigInt::from(index)))
                    .collect(),
            ),
            _ => None,
        }
    }
    fn to_json(&self, sorted_keys: bool) -> Result<String> {
        match self {
            Self::Null => Ok("null".into()),
            Self::Bool(value) => Ok(value.to_string()),
            Self::Integer(value) => Ok(value.to_string()),
            Self::String(value) => serde_json::to_string(value).map_err(Into::into),
            Self::List(values) => {
                let mut encoded = Vec::with_capacity(values.len());
                for value in values {
                    encoded.push(value.to_json(sorted_keys)?);
                }
                Ok(format!("[{}]", encoded.join(",")))
            }
            Self::Dict(values) => {
                let mut pairs = values.iter().collect::<Vec<_>>();
                if sorted_keys {
                    pairs.sort_by(|left, right| left.0.cmp(&right.0));
                }
                let mut encoded = Vec::with_capacity(pairs.len());
                for (key, value) in pairs {
                    encoded.push(format!(
                        "{}:{}",
                        serde_json::to_string(key)?,
                        value.to_json(sorted_keys)?
                    ));
                }
                Ok(format!("{{{}}}", encoded.join(",")))
            }
            Self::Range { .. } => Err(input_error(
                "range is an internal value and cannot be serialized",
            )),
        }
    }
}

#[derive(Default)]
struct ValueBudget {
    nodes: usize,
    text_bytes: usize,
}
impl ValueBudget {
    fn add_node(&mut self, depth: usize) -> Result<()> {
        self.nodes += 1;
        if self.nodes > 4096 || depth > 16 {
            return Err(input_error("value node/depth limit exceeded"));
        }
        Ok(())
    }
    fn add_string(&mut self, value: &str, depth: usize) -> Result<()> {
        self.add_node(depth)?;
        self.text_bytes = self.text_bytes.saturating_add(value.len());
        if value.chars().count() > 8192 || self.text_bytes > 65_536 {
            return Err(input_error("value string/text limit exceeded"));
        }
        Ok(())
    }
    fn visit(&mut self, source: &str, node: &JsonNode, depth: usize) -> Result<()> {
        if matches!(node.kind, JsonKind::Scalar) && source[node.span.clone()].starts_with('"') {
            let value: String = serde_json::from_str(&source[node.span.clone()])?;
            return self.add_string(&value, depth);
        }
        self.add_node(depth)?;
        match &node.kind {
            JsonKind::Object(values) => {
                if values.len() > 2048 {
                    return Err(input_error("dictionary entry limit exceeded"));
                }
                for (key, child) in values {
                    self.add_string(key, depth + 1)?;
                    self.visit(source, child, depth + 1)?;
                }
            }
            JsonKind::Array(values) => {
                if values.len() > 2048 {
                    return Err(input_error("list item limit exceeded"));
                }
                for child in values {
                    self.visit(source, child, depth + 1)?;
                }
            }
            JsonKind::Number => {
                parse_integer(&source[node.span.clone()])?;
            }
            JsonKind::Scalar => {}
        }
        Ok(())
    }
}

fn input_error(message: impl Into<String>) -> Error {
    failure("CODE_CAPABILITY_INPUT", message)
}

fn parse_integer(raw: &str) -> Result<BigInt> {
    if raw.contains(['.', 'e', 'E']) {
        return Err(input_error("only integer values are supported"));
    }
    let negative = raw.starts_with('-');
    let digits = raw.strip_prefix('-').unwrap_or(raw);
    let (base, digits, prefixed) = if let Some(rest) = digits
        .strip_prefix("0x")
        .or_else(|| digits.strip_prefix("0X"))
    {
        (16, rest, true)
    } else if let Some(rest) = digits
        .strip_prefix("0o")
        .or_else(|| digits.strip_prefix("0O"))
    {
        (8, rest, true)
    } else if let Some(rest) = digits
        .strip_prefix("0b")
        .or_else(|| digits.strip_prefix("0B"))
    {
        (2, rest, true)
    } else {
        (10, digits, false)
    };
    for (index, _) in digits.bytes().enumerate().filter(|(_, byte)| *byte == b'_') {
        let prefix_underscore = prefixed
            && index == 0
            && digits
                .as_bytes()
                .get(1)
                .is_some_and(|next| digit_in_base(*next, base));
        let between_digits = index > 0
            && digit_in_base(digits.as_bytes()[index - 1], base)
            && digits
                .as_bytes()
                .get(index + 1)
                .is_some_and(|next| digit_in_base(*next, base));
        if !prefix_underscore && !between_digits {
            return Err(input_error("integer underscores must separate digits"));
        }
    }
    let digits = digits.replace('_', "");
    if digits.is_empty() || digits.len() > 80 {
        return Err(input_error(
            "integer lexeme is invalid or exceeds the 256-bit envelope",
        ));
    }
    if base == 10
        && digits.len() > 1
        && digits.starts_with('0')
        && digits.chars().any(|ch| ch != '0')
    {
        return Err(input_error(
            "decimal integer literal has a forbidden leading zero",
        ));
    }
    let mut value = BigInt::parse_bytes(digits.as_bytes(), base)
        .ok_or_else(|| input_error("invalid integer literal"))?;
    if negative {
        value = -value;
    }
    if value.bits() > 256 {
        return Err(input_error("integer exceeds 256 bits"));
    }
    Ok(value)
}

fn digit_in_base(value: u8, base: u32) -> bool {
    (value as char).is_digit(base)
}

fn json_object_order(source: &str, node: &JsonNode) -> Result<Vec<String>> {
    let JsonKind::Object(entries) = &node.kind else {
        return Err(input_error("expected object"));
    };
    let raw = &source[node.span.clone()];
    let bytes = raw.as_bytes();
    let mut pos = 0;
    let skip_ws = |pos: &mut usize| {
        while bytes
            .get(*pos)
            .is_some_and(|b| matches!(b, b' ' | b'\n' | b'\r' | b'\t'))
        {
            *pos += 1;
        }
    };
    skip_ws(&mut pos);
    if bytes.get(pos) != Some(&b'{') {
        return Err(input_error("malformed object boundaries"));
    }
    pos += 1;
    let mut order = Vec::with_capacity(entries.len());
    loop {
        skip_ws(&mut pos);
        if bytes.get(pos) == Some(&b'}') {
            pos += 1;
            break;
        }
        let key_start = pos;
        let key_end = json_string_end(bytes, pos)?;
        let key: String = serde_json::from_str(&raw[key_start..key_end])?;
        pos = key_end;
        skip_ws(&mut pos);
        if bytes.get(pos) != Some(&b':') {
            return Err(input_error("malformed object key separator"));
        }
        pos += 1;
        skip_ws(&mut pos);
        pos = json_value_end(bytes, pos)?;
        if !entries.contains_key(&key) {
            return Err(input_error("object field is absent from strict parse tree"));
        }
        order.push(key);
        skip_ws(&mut pos);
        match bytes.get(pos) {
            Some(b',') => pos += 1,
            Some(b'}') => {
                pos += 1;
                break;
            }
            _ => return Err(input_error("malformed object separator")),
        }
    }
    if order.len() != entries.len() || pos != bytes.len() {
        return Err(input_error("object order does not match strict parse tree"));
    }
    Ok(order)
}

fn json_string_end(bytes: &[u8], start: usize) -> Result<usize> {
    if bytes.get(start) != Some(&b'"') {
        return Err(input_error("expected JSON object string key"));
    }
    let mut escaped = false;
    for (index, byte) in bytes.iter().enumerate().skip(start + 1) {
        if escaped {
            escaped = false;
        } else if *byte == b'\\' {
            escaped = true;
        } else if *byte == b'"' {
            return Ok(index + 1);
        }
    }
    Err(input_error("unterminated JSON object key"))
}

fn json_value_end(bytes: &[u8], start: usize) -> Result<usize> {
    let Some(first) = bytes.get(start).copied() else {
        return Err(input_error("missing JSON object value"));
    };
    if first == b'"' {
        return json_string_end(bytes, start);
    }
    if first != b'{' && first != b'[' {
        let mut pos = start;
        while bytes
            .get(pos)
            .is_some_and(|b| !matches!(b, b',' | b'}' | b']' | b' ' | b'\n' | b'\r' | b'\t'))
        {
            pos += 1;
        }
        return Ok(pos);
    }
    let mut stack = vec![if first == b'{' { b'}' } else { b']' }];
    let mut pos = start + 1;
    while pos < bytes.len() {
        match bytes[pos] {
            b'"' => pos = json_string_end(bytes, pos)?,
            b'{' => {
                stack.push(b'}');
                pos += 1;
            }
            b'[' => {
                stack.push(b']');
                pos += 1;
            }
            b'}' | b']' => {
                if stack.pop() != Some(bytes[pos]) {
                    return Err(input_error("mismatched JSON value delimiter"));
                }
                pos += 1;
                if stack.is_empty() {
                    return Ok(pos);
                }
            }
            _ => pos += 1,
        }
    }
    Err(input_error("unterminated JSON object value"))
}

fn value_from_node(source: &str, node: &JsonNode, depth: usize) -> Result<CodeValue> {
    if depth > 16 {
        return Err(input_error("value nesting exceeds 16"));
    }
    match &node.kind {
        JsonKind::Object(entries) => {
            let order = json_object_order(source, node)?;
            let mut values = Vec::with_capacity(entries.len());
            for key in order {
                let child = entries
                    .get(&key)
                    .ok_or_else(|| input_error("missing object member"))?;
                values.push((key, value_from_node(source, child, depth + 1)?));
            }
            Ok(CodeValue::Dict(values))
        }
        JsonKind::Array(entries) => Ok(CodeValue::List(
            entries
                .iter()
                .map(|child| value_from_node(source, child, depth + 1))
                .collect::<Result<Vec<_>>>()?,
        )),
        JsonKind::Number => Ok(CodeValue::Integer(parse_integer(
            &source[node.span.clone()],
        )?)),
        JsonKind::Scalar => {
            let raw = &source[node.span.clone()];
            if raw == "null" {
                Ok(CodeValue::Null)
            } else if raw == "true" {
                Ok(CodeValue::Bool(true))
            } else if raw == "false" {
                Ok(CodeValue::Bool(false))
            } else if raw.starts_with('"') {
                Ok(CodeValue::String(serde_json::from_str(raw)?))
            } else {
                Err(input_error("unsupported JSON scalar"))
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Span {
    start: usize,
    end: usize,
    line: usize,
    column: usize,
    end_line: usize,
    end_column: usize,
}
impl Span {
    fn join(first: Span, last: Span) -> Self {
        Self {
            start: first.start,
            end: last.end,
            line: first.line,
            column: first.column,
            end_line: last.end_line,
            end_column: last.end_column,
        }
    }
}

#[derive(Clone, Debug)]
enum TokenKind {
    Identifier(String),
    Number(String),
    String(String),
    Symbol(String),
}
#[derive(Clone, Debug)]
struct Token {
    kind: TokenKind,
    span: Span,
}
#[derive(Clone, Debug)]
struct SourceLine {
    indent: usize,
    tokens: Vec<Token>,
}

fn xid_start(ch: char) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    ch == '_'
        || RE
            .get_or_init(|| Regex::new(r"\A[\p{XID_Start}&&\p{Age:15.0}]\z").unwrap())
            .is_match(&ch.to_string())
}
fn xid_continue(ch: char) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\A[\p{XID_Continue}&&\p{Age:15.0}&&[^\u{200c}\u{200d}\u{30fb}\u{ff65}]]\z")
            .unwrap()
    })
    .is_match(&ch.to_string())
}

fn keyword(name: &str) -> bool {
    matches!(
        name,
        "False"
            | "None"
            | "True"
            | "and"
            | "as"
            | "assert"
            | "async"
            | "await"
            | "break"
            | "class"
            | "continue"
            | "def"
            | "del"
            | "elif"
            | "else"
            | "except"
            | "finally"
            | "for"
            | "from"
            | "global"
            | "if"
            | "import"
            | "in"
            | "is"
            | "lambda"
            | "nonlocal"
            | "not"
            | "or"
            | "pass"
            | "raise"
            | "return"
            | "try"
            | "while"
            | "with"
            | "yield"
    )
}

fn validate_identifier(name: &str) -> Result<()> {
    static IDENTIFIER: OnceLock<Regex> = OnceLock::new();
    let pattern = IDENTIFIER.get_or_init(|| {
        Regex::new(r"\A(?:_|[\p{XID_Start}&&\p{Age:15.0}])[\p{XID_Continue}&&\p{Age:15.0}&&[^\u{200c}\u{200d}\u{30fb}\u{ff65}]]*\z").unwrap()
    });
    if name.chars().count() > 80
        || !pattern.is_match(name)
        || keyword(name)
        || name.starts_with("__")
        || name.nfkc().collect::<String>() != name
    {
        return Err(input_error(
            "unsupported, reserved or noncanonical Python identifier",
        ));
    }
    Ok(())
}

fn span(source: &str, line: usize, start: usize, end: usize, line_start: usize) -> Span {
    let column = start.saturating_sub(line_start);
    let end_column = end.saturating_sub(line_start);
    let _ = source;
    Span {
        start,
        end,
        line,
        column,
        end_line: line,
        end_column,
    }
}

fn scan_string(
    source: &str,
    bytes: &[u8],
    mut cursor: usize,
    line: usize,
    line_start: usize,
) -> Result<(Token, usize)> {
    let raw = if matches!(bytes.get(cursor), Some(b'r' | b'R'))
        && bytes
            .get(cursor + 1)
            .is_some_and(|b| matches!(b, b'\'' | b'"'))
    {
        let value = true;
        cursor += 1;
        value
    } else if matches!(bytes.get(cursor), Some(b'b' | b'B' | b'f' | b'F'))
        && bytes
            .get(cursor + 1)
            .is_some_and(|b| matches!(b, b'\'' | b'"'))
    {
        return Err(input_error(
            "bytes and formatted string literals are unsupported",
        ));
    } else {
        false
    };
    let quote = *bytes
        .get(cursor)
        .ok_or_else(|| input_error("missing quote"))?;
    let triple = bytes.get(cursor..cursor + 3) == Some(&[quote, quote, quote]);
    let opening = if triple { 3 } else { 1 };
    let content_start = cursor + opening;
    let mut pos = content_start;
    let mut close = None;
    while pos < bytes.len() {
        if bytes[pos] == b'\\' {
            pos = (pos + 2).min(bytes.len());
            continue;
        }
        let matches_quote = if triple {
            bytes.get(pos..pos + 3) == Some(&[quote, quote, quote])
        } else {
            bytes[pos] == quote
        };
        if matches_quote {
            close = Some(pos);
            break;
        }
        if !triple && matches!(bytes[pos], b'\n' | b'\r') {
            return Err(input_error("single-line string literal crosses a line"));
        }
        pos += 1;
    }
    let close = close.ok_or_else(|| input_error("unterminated string literal"))?;
    let string_content = &source[line_start + content_start..line_start + close];
    let trailing_backslashes = string_content
        .chars()
        .rev()
        .take_while(|ch| *ch == '\\')
        .count();
    if raw && trailing_backslashes % 2 == 1 {
        return Err(input_error("raw string cannot end in an odd backslash"));
    }
    let value = decode_python_string(string_content, raw)?;
    let end = close + opening;
    Ok((
        Token {
            kind: TokenKind::String(value),
            span: span(
                source,
                line,
                line_start + cursor - if raw { 1 } else { 0 },
                line_start + end,
                line_start,
            ),
        },
        end,
    ))
}

fn decode_python_string(raw: &str, is_raw: bool) -> Result<String> {
    if is_raw {
        return Ok(raw.to_string());
    }
    let chars = raw.chars().collect::<Vec<_>>();
    let mut result = String::new();
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        index += 1;
        if ch != '\\' {
            result.push(ch);
            continue;
        }
        let Some(escape) = chars.get(index).copied() else {
            return Err(input_error("string literal ends in an escape"));
        };
        index += 1;
        match escape {
            '\\' => result.push('\\'),
            '\'' => result.push('\''),
            '"' => result.push('"'),
            'a' => result.push('\u{0007}'),
            'b' => result.push('\u{0008}'),
            'f' => result.push('\u{000c}'),
            'n' => result.push('\n'),
            'r' => result.push('\r'),
            't' => result.push('\t'),
            'v' => result.push('\u{000b}'),
            '\n' => {}
            'x' | 'u' | 'U' => {
                let digits = match escape {
                    'x' => 2,
                    'u' => 4,
                    _ => 8,
                };
                if index + digits > chars.len() {
                    return Err(input_error("incomplete hexadecimal string escape"));
                }
                let mut value = 0u32;
                for ch in &chars[index..index + digits] {
                    value = value
                        .checked_mul(16)
                        .and_then(|v| ch.to_digit(16).and_then(|d| v.checked_add(d)))
                        .ok_or_else(|| input_error("invalid hexadecimal string escape"))?;
                }
                let value = char::from_u32(value)
                    .ok_or_else(|| input_error("invalid Unicode scalar in string escape"))?;
                result.push(value);
                index += digits;
            }
            '0'..='7' => {
                let mut value = escape.to_digit(8).unwrap_or_default();
                let mut consumed = 0;
                while consumed < 2
                    && index < chars.len()
                    && chars[index].is_ascii_digit()
                    && chars[index] <= '7'
                {
                    value = value * 8 + chars[index].to_digit(8).unwrap_or_default();
                    index += 1;
                    consumed += 1;
                }
                let value = char::from_u32(value)
                    .ok_or_else(|| input_error("invalid octal string escape"))?;
                result.push(value);
            }
            other => {
                result.push('\\');
                result.push(other);
            }
        }
    }
    Ok(result)
}

fn lex_source(source: &str) -> Result<Vec<SourceLine>> {
    let mut lines = Vec::new();
    let mut absolute = 0usize;
    for (line_index, raw_line) in source.split_inclusive('\n').enumerate() {
        let content = raw_line
            .strip_suffix('\n')
            .unwrap_or(raw_line)
            .strip_suffix('\r')
            .unwrap_or(raw_line.strip_suffix('\n').unwrap_or(raw_line));
        let bytes = content.as_bytes();
        let mut cursor = 0usize;
        while bytes.get(cursor) == Some(&b' ') {
            cursor += 1;
        }
        if bytes.get(cursor) == Some(&b'\t') {
            return Err(input_error(format!(
                "tabs in indentation are unsupported at line {}",
                line_index + 1
            )));
        }
        if cursor == bytes.len() || bytes.get(cursor) == Some(&b'#') {
            absolute += raw_line.len();
            continue;
        }
        let indent = cursor;
        let line_no = line_index + 1;
        let line_start = absolute;
        let mut tokens = Vec::new();
        while cursor < bytes.len() {
            match bytes[cursor] {
                b' ' => cursor += 1,
                b'\t' => {
                    return Err(input_error(format!(
                        "tabs outside string literals are unsupported at line {line_no}"
                    )));
                }
                b'#' => break,
                b'\'' | b'"' => {
                    let (token, next) = scan_string(source, bytes, cursor, line_no, line_start)?;
                    tokens.push(token);
                    cursor = next;
                }
                b'r' | b'R'
                    if bytes
                        .get(cursor + 1)
                        .is_some_and(|b| matches!(b, b'\'' | b'"')) =>
                {
                    let (token, next) = scan_string(source, bytes, cursor, line_no, line_start)?;
                    tokens.push(token);
                    cursor = next;
                }
                b'b' | b'B' | b'f' | b'F'
                    if bytes
                        .get(cursor + 1)
                        .is_some_and(|b| matches!(b, b'\'' | b'"')) =>
                {
                    return Err(input_error(
                        "bytes and formatted string literals are unsupported",
                    ));
                }
                b'0'..=b'9' => {
                    let start = cursor;
                    cursor += 1;
                    while cursor < bytes.len() {
                        let b = bytes[cursor];
                        if b.is_ascii_alphanumeric()
                            || b == b'_'
                            || b == b'.'
                            || (matches!(b, b'+' | b'-')
                                && cursor > start
                                && matches!(bytes[cursor - 1], b'e' | b'E'))
                        {
                            cursor += 1;
                        } else {
                            break;
                        }
                    }
                    let raw = content[start..cursor].to_string();
                    tokens.push(Token {
                        kind: TokenKind::Number(raw),
                        span: span(
                            source,
                            line_no,
                            line_start + start,
                            line_start + cursor,
                            line_start,
                        ),
                    });
                }
                _ => {
                    let ch = content[cursor..]
                        .chars()
                        .next()
                        .ok_or_else(|| input_error("invalid source character boundary"))?;
                    if xid_start(ch) {
                        let start = cursor;
                        cursor += ch.len_utf8();
                        while cursor < content.len() {
                            let next = content[cursor..]
                                .chars()
                                .next()
                                .ok_or_else(|| input_error("invalid source character boundary"))?;
                            if !xid_continue(next) {
                                break;
                            }
                            cursor += next.len_utf8();
                        }
                        tokens.push(Token {
                            kind: TokenKind::Identifier(content[start..cursor].to_string()),
                            span: span(
                                source,
                                line_no,
                                line_start + start,
                                line_start + cursor,
                                line_start,
                            ),
                        });
                    } else {
                        let start = cursor;
                        let rest = &content[cursor..];
                        let symbol = [
                            "//=", "+=", "-=", "*=", "//", "==", "!=", "<=", ">=", "**", "/=",
                            "%=", "->",
                        ]
                        .into_iter()
                        .find(|candidate| rest.starts_with(candidate));
                        let symbol = if let Some(symbol) = symbol {
                            cursor += symbol.len();
                            symbol.to_string()
                        } else if "+-*/%<>=()[]{}:,. ;".contains(ch) && ch != ' ' {
                            cursor += ch.len_utf8();
                            ch.to_string()
                        } else {
                            return Err(input_error(format!(
                                "unsupported token at line {line_no}, UTF-8 column {}",
                                start
                            )));
                        };
                        tokens.push(Token {
                            kind: TokenKind::Symbol(symbol),
                            span: span(
                                source,
                                line_no,
                                line_start + start,
                                line_start + cursor,
                                line_start,
                            ),
                        });
                    }
                }
            }
        }
        if !tokens.is_empty() {
            lines.push(SourceLine { indent, tokens });
        }
        absolute += raw_line.len();
    }
    if source.is_empty() || !source.ends_with('\n') {
        // split_inclusive already emits a final unterminated line when nonempty.
    }
    Ok(lines)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Operator {
    Add,
    Subtract,
    Multiply,
    FloorDivide,
    Modulo,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UnaryOperator {
    Plus,
    Minus,
    Not,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BooleanOperator {
    And,
    Or,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CompareOperator {
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    In,
    NotIn,
}

#[derive(Clone, Debug)]
struct Expr {
    kind: ExprKind,
    span: Span,
    surface_span: Span,
}
impl Expr {
    fn new(kind: ExprKind, span: Span) -> Self {
        Self {
            kind,
            span,
            surface_span: span,
        }
    }
}
#[derive(Clone, Debug)]
enum ExprKind {
    Constant(CodeValue),
    Name(String),
    List(Vec<Expr>),
    Dict(Vec<(Expr, Expr)>),
    Subscript(Box<Expr>, Box<Expr>),
    Binary(Operator, Box<Expr>, Box<Expr>),
    Unary(UnaryOperator, Box<Expr>),
    Boolean(BooleanOperator, Vec<Expr>),
    Compare(Box<Expr>, Vec<(CompareOperator, Expr)>),
    Conditional(Box<Expr>, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
}

struct ExpressionParser<'a> {
    tokens: &'a [Token],
    position: usize,
}
impl<'a> ExpressionParser<'a> {
    fn new(tokens: &'a [Token]) -> Self {
        Self {
            tokens,
            position: 0,
        }
    }
    fn peek_symbol(&self, value: &str) -> bool {
        self.tokens
            .get(self.position)
            .is_some_and(|token| matches!(&token.kind, TokenKind::Symbol(found) if found == value))
    }
    fn peek_word(&self, value: &str) -> bool {
        self.tokens.get(self.position).is_some_and(
            |token| matches!(&token.kind, TokenKind::Identifier(found) if found == value),
        )
    }
    fn take_symbol(&mut self, value: &str) -> bool {
        if self.peek_symbol(value) {
            self.position += 1;
            true
        } else {
            false
        }
    }
    fn expect_symbol(&mut self, value: &str) -> Result<Token> {
        if self.peek_symbol(value) {
            let token = self.tokens[self.position].clone();
            self.position += 1;
            Ok(token)
        } else {
            Err(input_error(format!("expected `{value}` in expression")))
        }
    }
    fn expect_word(&mut self, value: &str) -> Result<Token> {
        if self.peek_word(value) {
            let token = self.tokens[self.position].clone();
            self.position += 1;
            Ok(token)
        } else {
            Err(input_error(format!("expected `{value}` in expression")))
        }
    }
    fn parse_all(mut self) -> Result<Expr> {
        let expression = self.parse_conditional()?;
        if self.position != self.tokens.len() {
            return Err(input_error("unsupported or trailing expression syntax"));
        }
        Ok(expression)
    }
    fn parse_conditional(&mut self) -> Result<Expr> {
        let body = self.parse_or()?;
        if self.take_word("if") {
            let test = self.parse_or()?;
            self.expect_word("else")?;
            let otherwise = self.parse_conditional()?;
            let node_span = Span::join(body.surface_span, otherwise.surface_span);
            Ok(Expr::new(
                ExprKind::Conditional(Box::new(test), Box::new(body), Box::new(otherwise)),
                node_span,
            ))
        } else {
            Ok(body)
        }
    }
    fn parse_or(&mut self) -> Result<Expr> {
        let first = self.parse_and()?;
        let mut values = vec![first];
        while self.take_word("or") {
            values.push(self.parse_and()?);
        }
        if values.len() == 1 {
            Ok(values.remove(0))
        } else {
            let node_span = Span::join(
                values[0].surface_span,
                values[values.len() - 1].surface_span,
            );
            Ok(Expr::new(
                ExprKind::Boolean(BooleanOperator::Or, values),
                node_span,
            ))
        }
    }
    fn parse_and(&mut self) -> Result<Expr> {
        let first = self.parse_not()?;
        let mut values = vec![first];
        while self.take_word("and") {
            values.push(self.parse_not()?);
        }
        if values.len() == 1 {
            Ok(values.remove(0))
        } else {
            let node_span = Span::join(
                values[0].surface_span,
                values[values.len() - 1].surface_span,
            );
            Ok(Expr::new(
                ExprKind::Boolean(BooleanOperator::And, values),
                node_span,
            ))
        }
    }
    fn parse_not(&mut self) -> Result<Expr> {
        if self.peek_word("not")
            && !self.tokens.get(self.position + 1).is_some_and(
                |token| matches!(&token.kind, TokenKind::Identifier(word) if word == "in"),
            )
        {
            let operator = self.tokens[self.position].span;
            self.position += 1;
            let value = self.parse_not()?;
            let node_span = Span::join(operator, value.surface_span);
            return Ok(Expr::new(
                ExprKind::Unary(UnaryOperator::Not, Box::new(value)),
                node_span,
            ));
        }
        self.parse_compare()
    }
    fn parse_compare(&mut self) -> Result<Expr> {
        let left = self.parse_additive()?;
        let mut comparisons = Vec::new();
        loop {
            let operation = if self.take_symbol("==") {
                Some(CompareOperator::Equal)
            } else if self.take_symbol("!=") {
                Some(CompareOperator::NotEqual)
            } else if self.take_symbol("<") {
                Some(CompareOperator::Less)
            } else if self.take_symbol("<=") {
                Some(CompareOperator::LessEqual)
            } else if self.take_symbol(">") {
                Some(CompareOperator::Greater)
            } else if self.take_symbol(">=") {
                Some(CompareOperator::GreaterEqual)
            } else if self.take_word("in") {
                Some(CompareOperator::In)
            } else if self.peek_word("not")
                && self.tokens.get(self.position + 1).is_some_and(
                    |token| matches!(&token.kind, TokenKind::Identifier(word) if word == "in"),
                )
            {
                self.position += 2;
                Some(CompareOperator::NotIn)
            } else {
                None
            };
            let Some(operation) = operation else {
                break;
            };
            comparisons.push((operation, self.parse_additive()?));
        }
        if comparisons.is_empty() {
            Ok(left)
        } else {
            let end = comparisons
                .last()
                .map(|row| row.1.surface_span)
                .unwrap_or(left.surface_span);
            Ok(Expr::new(
                ExprKind::Compare(Box::new(left.clone()), comparisons),
                Span::join(left.surface_span, end),
            ))
        }
    }
    fn parse_additive(&mut self) -> Result<Expr> {
        let mut left = self.parse_multiplicative()?;
        loop {
            let operator = if self.take_symbol("+") {
                Some(Operator::Add)
            } else if self.take_symbol("-") {
                Some(Operator::Subtract)
            } else {
                None
            };
            let Some(operator) = operator else {
                break;
            };
            let right = self.parse_multiplicative()?;
            let node_span = Span::join(left.surface_span, right.surface_span);
            left = Expr::new(
                ExprKind::Binary(operator, Box::new(left), Box::new(right)),
                node_span,
            );
        }
        Ok(left)
    }
    fn parse_multiplicative(&mut self) -> Result<Expr> {
        let mut left = self.parse_unary()?;
        loop {
            let operator = if self.take_symbol("*") {
                Some(Operator::Multiply)
            } else if self.take_symbol("//") {
                Some(Operator::FloorDivide)
            } else if self.take_symbol("%") {
                Some(Operator::Modulo)
            } else {
                None
            };
            let Some(operator) = operator else {
                break;
            };
            let right = self.parse_unary()?;
            let node_span = Span::join(left.surface_span, right.surface_span);
            left = Expr::new(
                ExprKind::Binary(operator, Box::new(left), Box::new(right)),
                node_span,
            );
        }
        Ok(left)
    }
    fn parse_unary(&mut self) -> Result<Expr> {
        let operation = if self.take_symbol("+") {
            Some(UnaryOperator::Plus)
        } else if self.take_symbol("-") {
            Some(UnaryOperator::Minus)
        } else {
            None
        };
        if let Some(operation) = operation {
            let start = self.tokens[self.position - 1].span;
            let value = self.parse_unary()?;
            let node_span = Span::join(start, value.surface_span);
            return Ok(Expr::new(
                ExprKind::Unary(operation, Box::new(value)),
                node_span,
            ));
        }
        self.parse_postfix()
    }
    fn parse_postfix(&mut self) -> Result<Expr> {
        let mut value = self.parse_atom()?;
        loop {
            if self.take_symbol("[") {
                let index = self.parse_conditional()?;
                let end = self.expect_symbol("]")?.span;
                let node_span = Span::join(value.surface_span, end);
                value = Expr::new(
                    ExprKind::Subscript(Box::new(value), Box::new(index)),
                    node_span,
                );
            } else if self.take_symbol("(") {
                let call_name = match &value.kind {
                    ExprKind::Name(name) => name.clone(),
                    _ => return Err(input_error("calls must target one approved builtin name")),
                };
                let mut arguments = Vec::new();
                if !self.peek_symbol(")") {
                    loop {
                        arguments.push(self.parse_conditional()?);
                        if !self.take_symbol(",") {
                            break;
                        }
                        if self.peek_symbol(")") {
                            break;
                        }
                    }
                }
                let end = self.expect_symbol(")")?.span;
                let node_span = Span::join(value.surface_span, end);
                value = Expr::new(ExprKind::Call(call_name, arguments), node_span);
            } else {
                break;
            }
        }
        Ok(value)
    }
    fn parse_atom(&mut self) -> Result<Expr> {
        let token = self
            .tokens
            .get(self.position)
            .cloned()
            .ok_or_else(|| input_error("expression is missing an operand"))?;
        self.position += 1;
        match token.kind {
            TokenKind::Number(raw) => {
                let value = parse_integer(&raw)?;
                Ok(Expr::new(
                    ExprKind::Constant(CodeValue::Integer(value)),
                    token.span,
                ))
            }
            TokenKind::String(value) => Ok(Expr::new(
                ExprKind::Constant(CodeValue::String(value)),
                token.span,
            )),
            TokenKind::Identifier(name) if name == "True" => Ok(Expr::new(
                ExprKind::Constant(CodeValue::Bool(true)),
                token.span,
            )),
            TokenKind::Identifier(name) if name == "False" => Ok(Expr::new(
                ExprKind::Constant(CodeValue::Bool(false)),
                token.span,
            )),
            TokenKind::Identifier(name) if name == "None" => {
                Ok(Expr::new(ExprKind::Constant(CodeValue::Null), token.span))
            }
            TokenKind::Identifier(name) => Ok(Expr::new(ExprKind::Name(name), token.span)),
            TokenKind::Symbol(symbol) if symbol == "(" => {
                let mut value = self.parse_conditional()?;
                let close = self.expect_symbol(")")?.span;
                value.surface_span = Span::join(token.span, close);
                Ok(value)
            }
            TokenKind::Symbol(symbol) if symbol == "[" => {
                let mut values = Vec::new();
                if !self.peek_symbol("]") {
                    loop {
                        values.push(self.parse_conditional()?);
                        if !self.take_symbol(",") {
                            break;
                        }
                        if self.peek_symbol("]") {
                            break;
                        }
                    }
                }
                let end = self.expect_symbol("]")?.span;
                Ok(Expr::new(
                    ExprKind::List(values),
                    Span::join(token.span, end),
                ))
            }
            TokenKind::Symbol(symbol) if symbol == "{" => {
                let mut values = Vec::new();
                if !self.peek_symbol("}") {
                    loop {
                        let key = self.parse_conditional()?;
                        self.expect_symbol(":")?;
                        let value = self.parse_conditional()?;
                        values.push((key, value));
                        if !self.take_symbol(",") {
                            break;
                        }
                        if self.peek_symbol("}") {
                            break;
                        }
                    }
                }
                let end = self.expect_symbol("}")?.span;
                Ok(Expr::new(
                    ExprKind::Dict(values),
                    Span::join(token.span, end),
                ))
            }
            _ => Err(input_error("unsupported expression atom")),
        }
    }
    fn take_word(&mut self, value: &str) -> bool {
        if self.peek_word(value) {
            self.position += 1;
            true
        } else {
            false
        }
    }
}

#[derive(Clone, Debug)]
struct Argument {
    name: String,
}
#[derive(Clone, Debug)]
struct Function {
    name: String,
    args: Vec<Argument>,
    body: Vec<Stmt>,
    span: Span,
}
#[derive(Clone, Debug)]
struct Stmt {
    kind: StmtKind,
    span: Span,
}
#[derive(Clone, Debug)]
enum StmtKind {
    Return(Option<Expr>),
    Assign(String, Expr),
    AugAssign(String, Operator, Expr),
    If(Expr, Vec<Stmt>, Vec<Stmt>),
    For(String, Expr, Vec<Stmt>, Vec<Stmt>),
    Break,
    Continue,
    Pass,
    Expression(Expr),
}

fn symbol(token: &Token, expected: &str) -> bool {
    matches!(&token.kind, TokenKind::Symbol(found) if found == expected)
}
fn word(token: &Token, expected: &str) -> bool {
    matches!(&token.kind, TokenKind::Identifier(found) if found == expected)
}

struct PythonParser<'a> {
    source: &'a str,
    lines: Vec<SourceLine>,
    next_line: usize,
}
impl<'a> PythonParser<'a> {
    fn parse(source: &'a str) -> Result<Function> {
        if source.trim().is_empty() || source.len() > 32_768 {
            return Err(input_error("source must contain 1..32768 UTF-8 bytes"));
        }
        let lines = lex_source(source)?;
        if lines.is_empty() || lines[0].indent != 0 {
            return Err(input_error("module must begin with one top-level function"));
        }
        let mut parser = Self {
            source,
            lines,
            next_line: 0,
        };
        let function = parser.parse_function()?;
        if parser.next_line != parser.lines.len() {
            return Err(input_error(
                "module must contain exactly one function and no trailing statements",
            ));
        }
        validate_function(&function)?;
        Ok(function)
    }

    fn parse_function(&mut self) -> Result<Function> {
        let line = self
            .lines
            .get(self.next_line)
            .cloned()
            .ok_or_else(|| input_error("function is missing"))?;
        if line.indent != 0 || line.tokens.len() < 5 || !word(&line.tokens[0], "def") {
            return Err(input_error(
                "module must contain exactly one function definition",
            ));
        }
        let name = match &line.tokens[1].kind {
            TokenKind::Identifier(name) => name.clone(),
            _ => return Err(input_error("function name must be an identifier")),
        };
        if !symbol(&line.tokens[2], "(") {
            return Err(input_error(
                "function parameters must use simple parentheses",
            ));
        }
        let close = line
            .tokens
            .iter()
            .position(|token| symbol(token, ")"))
            .ok_or_else(|| input_error("function parameter list is unterminated"))?;
        if close + 1 >= line.tokens.len() || !symbol(&line.tokens[close + 1], ":") {
            return Err(input_error(
                "function annotations, defaults, decorators and trailing syntax are unsupported",
            ));
        }
        let mut args = Vec::new();
        let mut index = 3;
        while index < close {
            if let TokenKind::Identifier(name) = &line.tokens[index].kind {
                args.push(Argument { name: name.clone() });
                index += 1;
                if index == close {
                    break;
                }
                if !symbol(&line.tokens[index], ",") {
                    return Err(input_error(
                        "only simple positional parameters are supported",
                    ));
                }
                index += 1;
            } else {
                return Err(input_error(
                    "parameter annotations and special arguments are unsupported",
                ));
            }
        }
        if args.len() > 16 {
            return Err(input_error("function accepts at most 16 parameters"));
        }
        self.next_line += 1;
        let body = if close + 2 < line.tokens.len() {
            parse_simple_list(&line.tokens[close + 2..], 0)?
        } else {
            self.parse_suite(0, 0, &[])?
        };
        if body.is_empty() {
            return Err(input_error(
                "function suite must contain at least one statement",
            ));
        }
        let end = body
            .last()
            .map(|statement| statement.span)
            .unwrap_or(line.tokens[close + 1].span);
        let function_span = Span::join(line.tokens[0].span, end);
        let function = Function {
            name,
            args,
            body,
            span: function_span,
        };
        let _ = self.source;
        Ok(function)
    }

    fn parse_suite(
        &mut self,
        parent_indent: usize,
        loop_depth: usize,
        inline: &[Token],
    ) -> Result<Vec<Stmt>> {
        if !inline.is_empty() {
            return parse_simple_list(inline, loop_depth);
        }
        let Some(next) = self.lines.get(self.next_line) else {
            return Err(input_error("compound statement has no indented suite"));
        };
        if next.indent <= parent_indent {
            return Err(input_error(
                "compound statement requires a deeper indentation level",
            ));
        }
        let child_indent = next.indent;
        let body = self.parse_block(child_indent, loop_depth)?;
        if body.is_empty() {
            return Err(input_error("compound statement suite is empty"));
        }
        Ok(body)
    }

    fn parse_block(&mut self, indent: usize, loop_depth: usize) -> Result<Vec<Stmt>> {
        let mut body = Vec::new();
        while let Some(line) = self.lines.get(self.next_line) {
            if line.indent < indent {
                break;
            }
            if line.indent > indent {
                return Err(input_error(format!(
                    "unexpected indentation at line {}",
                    line.tokens[0].span.line
                )));
            }
            let statements = self.parse_line(indent, loop_depth)?;
            body.extend(statements);
        }
        Ok(body)
    }

    fn parse_line(&mut self, indent: usize, loop_depth: usize) -> Result<Vec<Stmt>> {
        let line = self
            .lines
            .get(self.next_line)
            .cloned()
            .ok_or_else(|| input_error("statement is missing"))?;
        if line.tokens.is_empty() {
            self.next_line += 1;
            return Ok(Vec::new());
        }
        if word(&line.tokens[0], "if") || word(&line.tokens[0], "elif") {
            return Ok(vec![self.parse_if(
                indent,
                loop_depth,
                word(&line.tokens[0], "elif"),
            )?]);
        }
        if word(&line.tokens[0], "for") {
            return Ok(vec![self.parse_for(indent, loop_depth)?]);
        }
        self.next_line += 1;
        parse_simple_list(&line.tokens, loop_depth)
    }

    fn parse_if(&mut self, indent: usize, loop_depth: usize, is_elif: bool) -> Result<Stmt> {
        let line = self
            .lines
            .get(self.next_line)
            .cloned()
            .ok_or_else(|| input_error("if statement is missing"))?;
        if line.indent != indent {
            return Err(input_error("if indentation does not match its suite"));
        }
        let colon = suite_colon(&line.tokens, 1)?;
        let test = parse_expression(&line.tokens[1..colon])?;
        let inline = &line.tokens[colon + 1..];
        self.next_line += 1;
        let body = self.parse_suite(indent, loop_depth, inline)?;
        let mut otherwise = Vec::new();
        if let Some(next) = self.lines.get(self.next_line)
            && next.indent == indent
            && (word(&next.tokens[0], "else") || word(&next.tokens[0], "elif"))
        {
            if word(&next.tokens[0], "elif") {
                otherwise.push(self.parse_if(indent, loop_depth, true)?);
            } else {
                let else_line = self.lines[self.next_line].clone();
                let else_colon = suite_colon(&else_line.tokens, 1)?;
                if else_colon != 1 {
                    return Err(input_error("else header has trailing syntax"));
                }
                self.next_line += 1;
                otherwise =
                    self.parse_suite(indent, loop_depth, &else_line.tokens[else_colon + 1..])?;
            }
        }
        let end = otherwise
            .last()
            .or_else(|| body.last())
            .map(|statement| statement.span)
            .unwrap_or(line.tokens[colon].span);
        let _ = is_elif;
        Ok(Stmt {
            kind: StmtKind::If(test, body, otherwise),
            span: Span::join(line.tokens[0].span, end),
        })
    }

    fn parse_for(&mut self, indent: usize, loop_depth: usize) -> Result<Stmt> {
        let line = self
            .lines
            .get(self.next_line)
            .cloned()
            .ok_or_else(|| input_error("for statement is missing"))?;
        let colon = suite_colon(&line.tokens, 1)?;
        if colon < 4 {
            return Err(input_error(
                "for header requires one target and one iterable",
            ));
        }
        let target = match &line.tokens[1].kind {
            TokenKind::Identifier(name) if word(&line.tokens[2], "in") => name.clone(),
            _ => return Err(input_error("for target must be one identifier")),
        };
        let iterable = parse_expression(&line.tokens[3..colon])?;
        let inline = &line.tokens[colon + 1..];
        self.next_line += 1;
        let body = self.parse_suite(indent, loop_depth + 1, inline)?;
        let mut otherwise = Vec::new();
        if let Some(next) = self.lines.get(self.next_line)
            && next.indent == indent
            && word(&next.tokens[0], "else")
        {
            let else_line = self.lines[self.next_line].clone();
            let else_colon = suite_colon(&else_line.tokens, 1)?;
            if else_colon != 1 {
                return Err(input_error("for-else header has trailing syntax"));
            }
            self.next_line += 1;
            otherwise =
                self.parse_suite(indent, loop_depth, &else_line.tokens[else_colon + 1..])?;
        }
        let end = otherwise
            .last()
            .or_else(|| body.last())
            .map(|statement| statement.span)
            .unwrap_or(line.tokens[colon].span);
        Ok(Stmt {
            kind: StmtKind::For(target, iterable, body, otherwise),
            span: Span::join(line.tokens[0].span, end),
        })
    }
}

fn suite_colon(tokens: &[Token], start: usize) -> Result<usize> {
    let mut depth = 0isize;
    for (index, token) in tokens.iter().enumerate().skip(start) {
        if let TokenKind::Symbol(value) = &token.kind {
            match value.as_str() {
                "(" | "[" | "{" => depth += 1,
                ")" | "]" | "}" => depth -= 1,
                ":" if depth == 0 => return Ok(index),
                _ => {}
            }
        }
    }
    Err(input_error("compound statement is missing a colon"))
}

fn parse_expression(tokens: &[Token]) -> Result<Expr> {
    if tokens.is_empty() {
        return Err(input_error("expression is empty"));
    }
    ExpressionParser::new(tokens).parse_all()
}

fn parse_simple_list(tokens: &[Token], _loop_depth: usize) -> Result<Vec<Stmt>> {
    let mut segments = Vec::new();
    let mut depth = 0isize;
    let mut start = 0;
    for (index, token) in tokens.iter().enumerate() {
        if let TokenKind::Symbol(value) = &token.kind {
            match value.as_str() {
                "(" | "[" | "{" => depth += 1,
                ")" | "]" | "}" => depth -= 1,
                ";" if depth == 0 => {
                    segments.push(&tokens[start..index]);
                    start = index + 1;
                }
                _ => {}
            }
        }
    }
    segments.push(&tokens[start..]);
    let mut statements = Vec::new();
    for segment in segments {
        if segment.is_empty() {
            continue;
        }
        statements.push(parse_simple_statement(segment)?);
    }
    Ok(statements)
}

fn parse_simple_statement(tokens: &[Token]) -> Result<Stmt> {
    let first = &tokens[0];
    if word(first, "return") {
        let value = if tokens.len() == 1 {
            None
        } else {
            Some(parse_expression(&tokens[1..])?)
        };
        let end = value
            .as_ref()
            .map(|expr| expr.surface_span)
            .unwrap_or(first.span);
        return Ok(Stmt {
            kind: StmtKind::Return(value),
            span: Span::join(first.span, end),
        });
    }
    if tokens.len() == 1 && word(first, "break") {
        return Ok(Stmt {
            kind: StmtKind::Break,
            span: first.span,
        });
    }
    if tokens.len() == 1 && word(first, "continue") {
        return Ok(Stmt {
            kind: StmtKind::Continue,
            span: first.span,
        });
    }
    if tokens.len() == 1 && word(first, "pass") {
        return Ok(Stmt {
            kind: StmtKind::Pass,
            span: first.span,
        });
    }
    if let TokenKind::Identifier(name) = &first.kind
        && tokens.len() >= 3
        && let TokenKind::Symbol(op) = &tokens[1].kind
    {
        if op == "=" {
            let expression = parse_expression(&tokens[2..])?;
            return Ok(Stmt {
                span: Span::join(first.span, expression.surface_span),
                kind: StmtKind::Assign(name.clone(), expression),
            });
        }
        let augmented = match op.as_str() {
            "+=" => Some(Operator::Add),
            "-=" => Some(Operator::Subtract),
            "*=" => Some(Operator::Multiply),
            "//=" => Some(Operator::FloorDivide),
            "%=" => Some(Operator::Modulo),
            _ => None,
        };
        if let Some(operator) = augmented {
            let expression = parse_expression(&tokens[2..])?;
            return Ok(Stmt {
                span: Span::join(first.span, expression.surface_span),
                kind: StmtKind::AugAssign(name.clone(), operator, expression),
            });
        }
    }
    let expression = parse_expression(tokens)?;
    Ok(Stmt {
        span: expression.surface_span,
        kind: StmtKind::Expression(expression),
    })
}

fn reserved_store(name: &str) -> bool {
    CALLS.contains(&name)
}

fn validate_function(function: &Function) -> Result<()> {
    validate_identifier(&function.name)?;
    if reserved_store(&function.name) {
        return Err(input_error("function name shadows an approved builtin"));
    }
    let mut args = std::collections::BTreeSet::new();
    for argument in &function.args {
        validate_identifier(&argument.name)?;
        if reserved_store(&argument.name) || !args.insert(argument.name.as_str()) {
            return Err(input_error(
                "duplicate parameter or approved builtin shadowing",
            ));
        }
    }
    if function.args.len() > 16 {
        return Err(input_error("function accepts at most 16 parameters"));
    }
    validate_block(&function.body, 0, true)?;
    let (nodes, depth) = ast_metrics(AstRef::Module(function));
    if nodes > 2048 || depth > 40 {
        return Err(input_error("AST node/depth envelope exceeded"));
    }
    Ok(())
}

fn validate_block(body: &[Stmt], loop_depth: usize, top_function_body: bool) -> Result<()> {
    for (index, statement) in body.iter().enumerate() {
        match &statement.kind {
            StmtKind::Return(value) => {
                if let Some(value) = value {
                    validate_expr(value)?;
                }
            }
            StmtKind::Assign(name, value) => {
                validate_identifier(name)?;
                if reserved_store(name) {
                    return Err(input_error("assignment shadows an approved builtin"));
                }
                validate_expr(value)?;
            }
            StmtKind::AugAssign(name, _, value) => {
                validate_identifier(name)?;
                if reserved_store(name) {
                    return Err(input_error("assignment shadows an approved builtin"));
                }
                validate_expr(value)?;
            }
            StmtKind::If(test, body, otherwise) => {
                validate_expr(test)?;
                validate_block(body, loop_depth, false)?;
                validate_block(otherwise, loop_depth, false)?;
            }
            StmtKind::For(name, iterable, body, otherwise) => {
                validate_identifier(name)?;
                if reserved_store(name) {
                    return Err(input_error("loop target shadows an approved builtin"));
                }
                validate_expr(iterable)?;
                validate_block(body, loop_depth + 1, false)?;
                // A for-else suite does not inherit this loop's break/continue scope.
                validate_block(otherwise, loop_depth, false)?;
            }
            StmtKind::Break | StmtKind::Continue if loop_depth == 0 => {
                return Err(input_error("break/continue appears outside a for body"));
            }
            StmtKind::Break | StmtKind::Continue | StmtKind::Pass => {}
            StmtKind::Expression(expression) => {
                let is_docstring = top_function_body
                    && index == 0
                    && matches!(&expression.kind, ExprKind::Constant(CodeValue::String(_)));
                if !is_docstring {
                    return Err(input_error(
                        "only the first function statement may be a string docstring",
                    ));
                }
                validate_expr(expression)?;
            }
        }
    }
    Ok(())
}

fn validate_expr(expression: &Expr) -> Result<()> {
    match &expression.kind {
        ExprKind::Constant(value) => validate_literal(value),
        ExprKind::Name(name) => validate_identifier(name),
        ExprKind::List(values) => {
            for value in values {
                validate_expr(value)?;
            }
            Ok(())
        }
        ExprKind::Dict(values) => {
            for (key, value) in values {
                validate_expr(key)?;
                validate_expr(value)?;
            }
            Ok(())
        }
        ExprKind::Subscript(value, index) => {
            validate_expr(value)?;
            validate_expr(index)
        }
        ExprKind::Binary(_, left, right) => {
            validate_expr(left)?;
            validate_expr(right)
        }
        ExprKind::Unary(_, value) => validate_expr(value),
        ExprKind::Boolean(_, values) => {
            for value in values {
                validate_expr(value)?;
            }
            Ok(())
        }
        ExprKind::Compare(left, values) => {
            validate_expr(left)?;
            for (_, value) in values {
                validate_expr(value)?;
            }
            Ok(())
        }
        ExprKind::Conditional(test, body, otherwise) => {
            validate_expr(test)?;
            validate_expr(body)?;
            validate_expr(otherwise)
        }
        ExprKind::Call(name, arguments) => {
            if !CALLS.contains(&name.as_str()) {
                return Err(input_error(
                    "call target is outside the fixed builtin allowlist",
                ));
            }
            for argument in arguments {
                validate_expr(argument)?;
            }
            Ok(())
        }
    }
}

fn validate_literal(value: &CodeValue) -> Result<()> {
    match value {
        CodeValue::Null | CodeValue::Bool(_) => Ok(()),
        CodeValue::Integer(value) if value.bits() <= 256 => Ok(()),
        CodeValue::String(value) if value.chars().count() <= 8192 && value.len() <= 65_536 => {
            Ok(())
        }
        _ => Err(input_error(
            "literal value exceeds the bounded Python value contract",
        )),
    }
}

#[derive(Clone, Copy)]
enum AstRef<'a> {
    Module(&'a Function),
    Function(&'a Function),
    Arguments(&'a [Argument]),
    Argument,
    Statement(&'a Stmt),
    Expression(&'a Expr),
    Name,
    Context,
    Operator,
    UnaryOperator,
    BooleanOperator,
    CompareOperator,
}

fn ast_children(node: AstRef<'_>) -> Vec<AstRef<'_>> {
    match node {
        AstRef::Module(function) => vec![AstRef::Function(function)],
        AstRef::Function(function) => {
            let mut children = vec![AstRef::Arguments(&function.args)];
            children.extend(function.body.iter().map(AstRef::Statement));
            children
        }
        AstRef::Arguments(args) => args.iter().map(|_| AstRef::Argument).collect(),
        AstRef::Argument => Vec::new(),
        AstRef::Name => vec![AstRef::Context],
        AstRef::Context
        | AstRef::Operator
        | AstRef::UnaryOperator
        | AstRef::BooleanOperator
        | AstRef::CompareOperator => Vec::new(),
        AstRef::Statement(statement) => match &statement.kind {
            StmtKind::Return(value) => value.iter().map(AstRef::Expression).collect(),
            StmtKind::Assign(_, value) => vec![AstRef::Name, AstRef::Expression(value)],
            StmtKind::AugAssign(_, _, value) => {
                vec![AstRef::Name, AstRef::Operator, AstRef::Expression(value)]
            }
            StmtKind::If(test, body, otherwise) => {
                let mut children = vec![AstRef::Expression(test)];
                children.extend(body.iter().map(AstRef::Statement));
                children.extend(otherwise.iter().map(AstRef::Statement));
                children
            }
            StmtKind::For(_, iterable, body, otherwise) => {
                let mut children = vec![AstRef::Name, AstRef::Expression(iterable)];
                children.extend(body.iter().map(AstRef::Statement));
                children.extend(otherwise.iter().map(AstRef::Statement));
                children
            }
            StmtKind::Break | StmtKind::Continue | StmtKind::Pass => Vec::new(),
            StmtKind::Expression(value) => vec![AstRef::Expression(value)],
        },
        AstRef::Expression(expression) => match &expression.kind {
            ExprKind::Constant(_) => Vec::new(),
            ExprKind::Name(_) => vec![AstRef::Context],
            ExprKind::List(values) => {
                let mut children = values.iter().map(AstRef::Expression).collect::<Vec<_>>();
                children.push(AstRef::Context);
                children
            }
            ExprKind::Dict(values) => {
                let mut children = values
                    .iter()
                    .map(|(key, _)| AstRef::Expression(key))
                    .collect::<Vec<_>>();
                children.extend(values.iter().map(|(_, value)| AstRef::Expression(value)));
                children
            }
            ExprKind::Subscript(value, index) => vec![
                AstRef::Expression(value),
                AstRef::Expression(index),
                AstRef::Context,
            ],
            ExprKind::Binary(_, left, right) => vec![
                AstRef::Expression(left),
                AstRef::Operator,
                AstRef::Expression(right),
            ],
            ExprKind::Unary(_, value) => vec![AstRef::UnaryOperator, AstRef::Expression(value)],
            ExprKind::Boolean(operator, values) => {
                let _ = operator;
                let mut children = vec![AstRef::BooleanOperator];
                children.extend(values.iter().map(AstRef::Expression));
                children
            }
            ExprKind::Compare(left, comparisons) => {
                let mut children = vec![AstRef::Expression(left)];
                children.extend(comparisons.iter().map(|_| AstRef::CompareOperator));
                children.extend(
                    comparisons
                        .iter()
                        .map(|(_, value)| AstRef::Expression(value)),
                );
                children
            }
            ExprKind::Conditional(test, body, otherwise) => vec![
                AstRef::Expression(test),
                AstRef::Expression(body),
                AstRef::Expression(otherwise),
            ],
            ExprKind::Call(_, args) => {
                let mut children = vec![AstRef::Name];
                children.extend(args.iter().map(AstRef::Expression));
                children
            }
        },
    }
}

fn ast_metrics(root: AstRef<'_>) -> (usize, usize) {
    let mut queue = VecDeque::from([(root, 0usize)]);
    let mut count = 0usize;
    let mut max_depth = 0usize;
    while let Some((node, depth)) = queue.pop_front() {
        count += 1;
        max_depth = max_depth.max(depth);
        queue.extend(
            ast_children(node)
                .into_iter()
                .map(|child| (child, depth + 1)),
        );
    }
    (count, max_depth)
}

fn syntax_name(node: AstRef<'_>) -> Option<&'static str> {
    match node {
        AstRef::Function(_) => Some("FunctionDef"),
        AstRef::Statement(statement) => Some(match statement.kind {
            StmtKind::Return(_) => "Return",
            StmtKind::Assign(..) => "Assign",
            StmtKind::AugAssign(..) => "AugAssign",
            StmtKind::If(..) => "If",
            StmtKind::For(..) => "For",
            StmtKind::Break => "Break",
            StmtKind::Continue => "Continue",
            StmtKind::Pass => "Pass",
            StmtKind::Expression(_) => "Expr",
        }),
        AstRef::Expression(expression) => match expression.kind {
            ExprKind::Call(..) => Some("Call"),
            ExprKind::Compare(..) => Some("Compare"),
            _ => None,
        },
        _ => None,
    }
}

fn node_span(node: AstRef<'_>) -> Option<Span> {
    match node {
        AstRef::Function(function) => Some(function.span),
        AstRef::Statement(statement) => Some(statement.span),
        AstRef::Expression(expression)
            if matches!(expression.kind, ExprKind::Call(..) | ExprKind::Compare(..)) =>
        {
            Some(expression.span)
        }
        _ => None,
    }
}

fn structure(source: &str, function: &Function) -> Vec<CodeStructureNode> {
    let mut queue = VecDeque::from([AstRef::Function(function)]);
    let mut nodes = Vec::new();
    while let Some(node) = queue.pop_front() {
        if let (Some(syntax), Some(span)) = (syntax_name(node), node_span(node)) {
            nodes.push(CodeStructureNode {
                syntax: syntax.into(),
                line: span.line,
                utf8_column: span.column,
                end_line: span.end_line,
                end_utf8_column: span.end_column,
                source_segment: source.get(span.start..span.end).unwrap_or("").to_string(),
            });
        }
        queue.extend(ast_children(node));
    }
    nodes
}

struct CodeInput {
    source: String,
    arguments: Option<CodeValue>,
    cases: Vec<(CodeValue, CodeValue)>,
    max_steps: usize,
}

fn field<'a>(entries: &'a BTreeMap<String, JsonNode>, name: &str) -> Result<&'a JsonNode> {
    entries
        .get(name)
        .ok_or_else(|| input_error(format!("required field `{name}` is missing")))
}

fn exact_fields(
    entries: &BTreeMap<String, JsonNode>,
    required: &[&str],
    optional: &[&str],
) -> Result<()> {
    for name in required {
        if !entries.contains_key(*name) {
            return Err(input_error(format!("required field `{name}` is missing")));
        }
    }
    for name in entries.keys() {
        if !required.contains(&name.as_str()) && !optional.contains(&name.as_str()) {
            return Err(input_error(format!("unknown input field `{name}`")));
        }
    }
    Ok(())
}

fn string_field(source: &str, node: &JsonNode) -> Result<String> {
    if !matches!(node.kind, JsonKind::Scalar) || !source[node.span.clone()].starts_with('"') {
        return Err(input_error("source field must be a JSON string"));
    }
    serde_json::from_str(&source[node.span.clone()]).map_err(Into::into)
}

fn positive_integer_field(source: &str, node: &JsonNode) -> Result<usize> {
    if !matches!(node.kind, JsonKind::Number) {
        return Err(input_error("max_steps must be an integer"));
    }
    let value = parse_integer(&source[node.span.clone()])?;
    let value = value
        .to_usize()
        .ok_or_else(|| input_error("max_steps is outside the supported range"))?;
    if !(1..=200_000).contains(&value) {
        return Err(input_error("max_steps must be in 1..=200000"));
    }
    Ok(value)
}

fn parse_input(document: &str, operation: CodeOperation) -> Result<CodeInput> {
    if document.len() > 256_000 {
        return Err(input_error("request document exceeds 256000 bytes"));
    }
    let root = strict_json_document(document)?;
    let JsonKind::Object(entries) = &root.kind else {
        return Err(input_error("request document root must be an object"));
    };
    let (required, optional): (&[&str], &[&str]) = match operation {
        CodeOperation::Read => (&["情報源"], &[]),
        CodeOperation::Evaluate => (&["情報源", "引数"], &["最大手数"]),
        CodeOperation::Verify => (&["情報源", "試験"], &["最大手数"]),
    };
    exact_fields(entries, required, optional)?;
    let source = string_field(document, field(entries, "情報源")?)?;
    if source.is_empty() || source.len() > 32_768 {
        return Err(input_error("source must contain 1..32768 UTF-8 bytes"));
    }
    let max_steps = entries
        .get("最大手数")
        .map(|node| positive_integer_field(document, node))
        .transpose()?
        .unwrap_or(50_000);
    let mut arguments = None;
    let mut cases = Vec::new();
    if operation == CodeOperation::Evaluate {
        let node = field(entries, "引数")?;
        if !matches!(node.kind, JsonKind::Object(_)) {
            return Err(input_error("引数 must be an object"));
        }
        ValueBudget::default().visit(document, node, 0)?;
        arguments = Some(value_from_node(document, node, 0)?);
    }
    if operation == CodeOperation::Verify {
        let node = field(entries, "試験")?;
        let JsonKind::Array(rows) = &node.kind else {
            return Err(input_error("試験 must be an array"));
        };
        if !(1..=32).contains(&rows.len()) {
            return Err(input_error("試験数は1..32"));
        }
        for row in rows {
            let JsonKind::Object(fields) = &row.kind else {
                return Err(input_error("each case must be an object"));
            };
            exact_fields(fields, &["引数", "期待値"], &[])?;
            ValueBudget::default().visit(document, row, 0)?;
            let args_node = field(fields, "引数")?;
            if !matches!(args_node.kind, JsonKind::Object(_)) {
                return Err(input_error("case 引数 must be an object"));
            }
            cases.push((
                value_from_node(document, args_node, 1)?,
                value_from_node(document, field(fields, "期待値")?, 1)?,
            ));
        }
    }
    Ok(CodeInput {
        source,
        arguments,
        cases,
        max_steps,
    })
}

pub fn run(
    document: &str,
    operation: CodeOperation,
    deadline: Option<Instant>,
) -> Result<CodeCapabilityReport> {
    let input = parse_input(document, operation)?;
    let function = PythonParser::parse(&input.source)?;
    let (ast_nodes, ast_depth) = ast_metrics(AstRef::Module(&function));
    let structural = structure(&input.source, &function);
    let source_sha256 = digest(input.source.as_bytes());
    let parameters = function
        .args
        .iter()
        .map(|arg| arg.name.clone())
        .collect::<Vec<_>>();
    let (summary, evaluation, verification) = match operation {
        CodeOperation::Read => {
            let branches = structural.iter().filter(|node| node.syntax == "If").count();
            let loops = structural
                .iter()
                .filter(|node| node.syntax == "For")
                .count();
            (
                format!(
                    "関数{}。引数は{}個。分岐{}、反復{}。型・値・仕様適合は未検証です。",
                    function.name,
                    parameters.len(),
                    branches,
                    loops
                ),
                None,
                None,
            )
        }
        CodeOperation::Evaluate => {
            let arguments = input
                .arguments
                .as_ref()
                .ok_or_else(|| input_error("arguments are missing"))?;
            let report = evaluate(
                &function,
                &input.source,
                arguments,
                input.max_steps,
                deadline,
            )?;
            let text = if report.status == "COMPUTED" {
                format!(
                    "限定評価結果: {} ({}手)",
                    report.actual_json.as_deref().unwrap_or("null"),
                    report.steps
                )
            } else {
                format!(
                    "限定評価{}: {}",
                    report.status,
                    report.diagnostic.as_deref().unwrap_or("unknown")
                )
            };
            (text, Some(report), None)
        }
        CodeOperation::Verify => {
            let report = verify(
                &function,
                &input.source,
                &input.cases,
                input.max_steps,
                deadline,
            )?;
            let matches = report.cases.iter().filter(|case| case.matched).count();
            let text = format!(
                "指定した有限ケース: {}件中{}件一致。全入力での正しさは示しません。",
                report.case_count, matches
            );
            (text, None, Some(report))
        }
    };
    Ok(CodeCapabilityReport {
        version: VERSION.into(),
        operation: match operation { CodeOperation::Read => "READ", CodeOperation::Evaluate => "EVALUATE", CodeOperation::Verify => "VERIFY" }.into(),
        source: input.source,
        source_sha256,
        function_name: function.name,
        parameters,
        ast_nodes,
        ast_depth,
        structure: structural,
        summary,
        evaluation,
        verification,
        assurance: "限定Python AST読解・ネイティブ評価。ホストPython、exec、eval、コンパイラ、外部作用は使用していない".into(),
    })
}

#[derive(Clone, Debug)]
struct EvalError {
    kind: String,
    message: String,
    line: Option<usize>,
}
impl EvalError {
    fn new(kind: &str, message: impl Into<String>, line: Option<usize>) -> Self {
        Self {
            kind: kind.into(),
            message: message.into(),
            line,
        }
    }
    fn with_span(kind: &str, message: impl Into<String>, span: Span) -> Self {
        Self::new(kind, message, Some(span.line))
    }
    fn diagnostic(&self) -> String {
        format!("{}: {}", self.kind, self.message)
    }
}

struct Evaluator {
    environment: BTreeMap<String, CodeValue>,
    locals: std::collections::BTreeSet<String>,
    max_steps: usize,
    steps: usize,
    current_line: Option<usize>,
    branches: Vec<BranchObservation>,
    deadline: Option<Instant>,
}

impl Evaluator {
    fn tick(&mut self, source_span: Option<Span>) -> std::result::Result<(), EvalError> {
        self.steps = self.steps.saturating_add(1);
        if let Some(span) = source_span {
            self.current_line = Some(span.line);
        }
        if self.steps > self.max_steps {
            return Err(EvalError::new(
                "STEP_LIMIT",
                "evaluation step budget exceeded",
                self.current_line,
            ));
        }
        if self
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            return Err(EvalError::new(
                "DEADLINE",
                "request deadline exceeded",
                self.current_line,
            ));
        }
        Ok(())
    }

    fn eval(&mut self, expression: &Expr) -> std::result::Result<CodeValue, EvalError> {
        self.tick(Some(expression.span))?;
        let value = match &expression.kind {
            ExprKind::Constant(value) => value.clone(),
            ExprKind::Name(name) => self.environment.get(name).cloned().ok_or_else(|| {
                if self.locals.contains(name) {
                    EvalError::with_span(
                        "UnboundLocalError",
                        format!("local variable `{name}` is unbound"),
                        expression.span,
                    )
                } else {
                    EvalError::with_span(
                        "NameError",
                        format!("name `{name}` is not defined"),
                        expression.span,
                    )
                }
            })?,
            ExprKind::List(values) => {
                let mut output = Vec::with_capacity(values.len());
                for value in values {
                    output.push(self.eval(value)?);
                }
                CodeValue::List(output)
            }
            ExprKind::Dict(values) => {
                let mut output: Vec<(String, CodeValue)> = Vec::with_capacity(values.len());
                for (key, value) in values {
                    let key = self.eval(key)?;
                    let CodeValue::String(key) = key else {
                        return Err(EvalError::with_span(
                            "CODE_VALUE_LIMIT",
                            "dictionary keys must be strings",
                            expression.span,
                        ));
                    };
                    let value = self.eval(value)?;
                    if let Some((_, old_value)) =
                        output.iter_mut().find(|(old_key, _)| old_key == &key)
                    {
                        *old_value = value;
                    } else {
                        output.push((key, value));
                    }
                }
                CodeValue::Dict(output)
            }
            ExprKind::Subscript(value, index) => {
                let target = self.eval(value)?;
                let position = self.eval(index)?;
                self.subscript(target, position, expression.span)?
            }
            ExprKind::Binary(operator, left, right) => {
                let left = self.eval(left)?;
                let right = self.eval(right)?;
                self.binary(*operator, left, right, expression.span)?
            }
            ExprKind::Unary(operator, operand) => {
                let value = self.eval(operand)?;
                match operator {
                    UnaryOperator::Not => CodeValue::Bool(!value.truthy()),
                    UnaryOperator::Plus => {
                        CodeValue::Integer(as_integer(&value).ok_or_else(|| {
                            EvalError::with_span(
                                "TypeError",
                                "unary plus requires an integer",
                                expression.span,
                            )
                        })?)
                    }
                    UnaryOperator::Minus => {
                        CodeValue::Integer(-as_integer(&value).ok_or_else(|| {
                            EvalError::with_span(
                                "TypeError",
                                "unary minus requires an integer",
                                expression.span,
                            )
                        })?)
                    }
                }
            }
            ExprKind::Boolean(operator, values) => {
                let mut result = self.eval(values.first().ok_or_else(|| {
                    EvalError::with_span(
                        "SyntaxError",
                        "boolean operation has no operand",
                        expression.span,
                    )
                })?)?;
                for value in values.iter().skip(1) {
                    if (*operator == BooleanOperator::And && !result.truthy())
                        || (*operator == BooleanOperator::Or && result.truthy())
                    {
                        break;
                    }
                    result = self.eval(value)?;
                }
                result
            }
            ExprKind::Compare(left, comparisons) => {
                let mut previous = self.eval(left)?;
                let mut matched = true;
                for (operator, next_expr) in comparisons {
                    let next = self.eval(next_expr)?;
                    let current = self.compare(*operator, &previous, &next, expression.span)?;
                    if !current {
                        matched = false;
                        break;
                    }
                    previous = next;
                }
                CodeValue::Bool(matched)
            }
            ExprKind::Conditional(test, body, otherwise) => {
                if self.eval(test)?.truthy() {
                    self.eval(body)?
                } else {
                    self.eval(otherwise)?
                }
            }
            ExprKind::Call(name, arguments) => {
                let mut values = Vec::with_capacity(arguments.len());
                for argument in arguments {
                    values.push(self.eval(argument)?);
                }
                self.call(name, values, expression.span)?
            }
        };
        check_runtime_value(&value, true)
            .map_err(|error| EvalError::with_span("CODE_VALUE_LIMIT", error, expression.span))?;
        Ok(value)
    }

    fn binary(
        &mut self,
        operator: Operator,
        left: CodeValue,
        right: CodeValue,
        at: Span,
    ) -> std::result::Result<CodeValue, EvalError> {
        let int_left = as_integer(&left);
        let int_right = as_integer(&right);
        let value = match operator {
            Operator::Add => {
                if let (Some(a), Some(b)) = (&int_left, &int_right) {
                    CodeValue::Integer(a + b)
                } else {
                    match (left, right) {
                        (CodeValue::String(a), CodeValue::String(b)) => {
                            if a.chars().count().saturating_add(b.chars().count()) > 8192 {
                                return Err(EvalError::with_span(
                                    "CODE_VALUE_LIMIT",
                                    "string concatenation exceeds 8192 codepoints",
                                    at,
                                ));
                            }
                            CodeValue::String(a + &b)
                        }
                        (CodeValue::List(mut a), CodeValue::List(b)) => {
                            if a.len().saturating_add(b.len()) > 2048 {
                                return Err(EvalError::with_span(
                                    "CODE_VALUE_LIMIT",
                                    "list concatenation exceeds 2048 items",
                                    at,
                                ));
                            }
                            a.extend(b);
                            CodeValue::List(a)
                        }
                        (a, b) => {
                            return Err(EvalError::with_span(
                                "TypeError",
                                format!(
                                    "unsupported operands for +: {} and {}",
                                    a.type_name(),
                                    b.type_name()
                                ),
                                at,
                            ));
                        }
                    }
                }
            }
            Operator::Subtract | Operator::FloorDivide | Operator::Modulo => {
                let (Some(a), Some(b)) = (int_left, int_right) else {
                    return Err(EvalError::with_span(
                        "TypeError",
                        "subtraction, floor division and modulo require integers",
                        at,
                    ));
                };
                if (operator == Operator::FloorDivide || operator == Operator::Modulo)
                    && b.is_zero()
                {
                    return Err(EvalError::with_span(
                        "ZeroDivisionError",
                        "integer division or modulo by zero",
                        at,
                    ));
                }
                let result = match operator {
                    Operator::Subtract => a - b,
                    Operator::FloorDivide => floor_div(&a, &b),
                    Operator::Modulo => python_mod(&a, &b),
                    _ => unreachable!(),
                };
                CodeValue::Integer(result)
            }
            Operator::Multiply => {
                if let (Some(a), Some(b)) = (int_left.clone(), int_right.clone()) {
                    CodeValue::Integer(a * b)
                } else if let (CodeValue::String(sequence), Some(times)) =
                    (&left, int_right.as_ref())
                {
                    CodeValue::String(repeat_string(sequence, times, at)?)
                } else if let (Some(times), CodeValue::String(sequence)) =
                    (int_left.as_ref(), &right)
                {
                    CodeValue::String(repeat_string(sequence, times, at)?)
                } else if let (CodeValue::List(sequence), Some(times)) = (&left, int_right.as_ref())
                {
                    CodeValue::List(repeat_list(sequence, times, at)?)
                } else if let (Some(times), CodeValue::List(sequence)) = (int_left.as_ref(), &right)
                {
                    CodeValue::List(repeat_list(sequence, times, at)?)
                } else {
                    return Err(EvalError::with_span(
                        "TypeError",
                        format!(
                            "unsupported operands for *: {} and {}",
                            left.type_name(),
                            right.type_name()
                        ),
                        at,
                    ));
                }
            }
        };
        check_runtime_value(&value, true)
            .map_err(|error| EvalError::with_span("CODE_VALUE_LIMIT", error, at))?;
        Ok(value)
    }

    fn compare(
        &mut self,
        operator: CompareOperator,
        left: &CodeValue,
        right: &CodeValue,
        at: Span,
    ) -> std::result::Result<bool, EvalError> {
        match operator {
            CompareOperator::Equal => Ok(python_equal(left, right)),
            CompareOperator::NotEqual => Ok(!python_equal(left, right)),
            CompareOperator::In | CompareOperator::NotIn => {
                let found = contains(right, left).ok_or_else(|| {
                    EvalError::with_span(
                        "TypeError",
                        "right operand is not a supported container",
                        at,
                    )
                })?;
                Ok(if operator == CompareOperator::In {
                    found
                } else {
                    !found
                })
            }
            CompareOperator::Less
            | CompareOperator::LessEqual
            | CompareOperator::Greater
            | CompareOperator::GreaterEqual => {
                let order = python_compare(left, right)
                    .map_err(|message| EvalError::with_span("TypeError", message, at))?;
                Ok(match operator {
                    CompareOperator::Less => order.is_lt(),
                    CompareOperator::LessEqual => !order.is_gt(),
                    CompareOperator::Greater => order.is_gt(),
                    CompareOperator::GreaterEqual => !order.is_lt(),
                    _ => false,
                })
            }
        }
    }

    fn subscript(
        &self,
        value: CodeValue,
        index: CodeValue,
        at: Span,
    ) -> std::result::Result<CodeValue, EvalError> {
        match value {
            CodeValue::List(values) => {
                let position = sequence_index(&index, values.len(), "list", at)?;
                values.get(position).cloned().ok_or_else(|| {
                    EvalError::with_span("IndexError", "list index out of range", at)
                })
            }
            CodeValue::String(value) => {
                let chars = value.chars().collect::<Vec<_>>();
                let position = sequence_index(&index, chars.len(), "string", at)?;
                chars
                    .get(position)
                    .map(|ch| CodeValue::String(ch.to_string()))
                    .ok_or_else(|| {
                        EvalError::with_span("IndexError", "string index out of range", at)
                    })
            }
            CodeValue::Dict(values) => {
                let CodeValue::String(key) = index else {
                    return Err(EvalError::with_span(
                        "TypeError",
                        "dictionary key must be a string",
                        at,
                    ));
                };
                values
                    .iter()
                    .find(|(existing, _)| existing == &key)
                    .map(|(_, value)| value.clone())
                    .ok_or_else(|| {
                        EvalError::with_span("KeyError", format!("key {key:?} is missing"), at)
                    })
            }
            CodeValue::Range { start, step, len } => {
                let position = sequence_index(&index, len, "range", at)?;
                Ok(CodeValue::Integer(start + step * BigInt::from(position)))
            }
            other => Err(EvalError::with_span(
                "TypeError",
                format!("{} is not subscriptable", other.type_name()),
                at,
            )),
        }
    }

    fn call(
        &mut self,
        name: &str,
        values: Vec<CodeValue>,
        at: Span,
    ) -> std::result::Result<CodeValue, EvalError> {
        let arity =
            |expected: &str| EvalError::with_span("TypeError", format!("{name}() {expected}"), at);
        match name {
            "range" => make_range(&values)
                .map_err(|message| EvalError::with_span("TypeError", message, at)),
            "abs" => {
                if values.len() != 1 {
                    return Err(arity("takes exactly one argument"));
                }
                let integer = as_integer(&values[0]).ok_or_else(|| {
                    EvalError::with_span("TypeError", "abs() argument must be an integer", at)
                })?;
                Ok(CodeValue::Integer(integer.abs()))
            }
            "len" => {
                if values.len() != 1 {
                    return Err(arity("takes exactly one argument"));
                }
                let length = values[0].len().ok_or_else(|| {
                    EvalError::with_span("TypeError", "object has no supported length", at)
                })?;
                Ok(CodeValue::Integer(BigInt::from(length)))
            }
            "min" | "max" => {
                let items = if values.len() > 1 {
                    values
                } else if let Some(value) = values.first() {
                    value.iterable().ok_or_else(|| {
                        EvalError::with_span("TypeError", "argument is not an iterable", at)
                    })?
                } else {
                    return Err(arity("expected at least one argument"));
                };
                if items.len() > 2048 {
                    return Err(EvalError::with_span(
                        "CODE_VALUE_LIMIT",
                        "aggregate input exceeds 2048 items",
                        at,
                    ));
                }
                if items.is_empty() {
                    return Err(arity("arg is an empty sequence"));
                }
                let mut best = items[0].clone();
                for item in items.iter().skip(1) {
                    self.tick(None)?;
                    let ordering = python_compare(item, &best)
                        .map_err(|message| EvalError::with_span("TypeError", message, at))?;
                    if (name == "min" && ordering.is_lt()) || (name == "max" && ordering.is_gt()) {
                        best = item.clone();
                    }
                }
                Ok(best)
            }
            "sum" => {
                if values.len() != 1 {
                    return Err(arity("takes exactly one iterable"));
                }
                let items = values[0].iterable().ok_or_else(|| {
                    EvalError::with_span("TypeError", "sum() argument is not iterable", at)
                })?;
                if items.len() > 2048 {
                    return Err(EvalError::with_span(
                        "CODE_VALUE_LIMIT",
                        "sum input exceeds 2048 items",
                        at,
                    ));
                }
                let mut sum = BigInt::zero();
                for item in items {
                    self.tick(None)?;
                    let item = as_integer(&item).ok_or_else(|| {
                        EvalError::with_span("TypeError", "sum() supports integer items only", at)
                    })?;
                    sum += item;
                    if sum.bits() > 256 {
                        return Err(EvalError::with_span(
                            "CODE_VALUE_LIMIT",
                            "sum exceeds 256 bits",
                            at,
                        ));
                    }
                }
                Ok(CodeValue::Integer(sum))
            }
            "list" => {
                if values.len() != 1 {
                    return Err(arity("takes exactly one iterable"));
                }
                let items = values[0].iterable().ok_or_else(|| {
                    EvalError::with_span("TypeError", "list() argument is not iterable", at)
                })?;
                if items.len() > 2048 {
                    return Err(EvalError::with_span(
                        "CODE_VALUE_LIMIT",
                        "list() output exceeds 2048 items",
                        at,
                    ));
                }
                Ok(CodeValue::List(items))
            }
            "sorted" => {
                if values.len() != 1 {
                    return Err(arity("takes exactly one iterable"));
                }
                let mut items = values[0].iterable().ok_or_else(|| {
                    EvalError::with_span("TypeError", "sorted() argument is not iterable", at)
                })?;
                if items.len() > 2048 {
                    return Err(EvalError::with_span(
                        "CODE_VALUE_LIMIT",
                        "sorted() input exceeds 2048 items",
                        at,
                    ));
                }
                for _ in 0..items.len() {
                    self.tick(None)?;
                }
                let mut failure = None;
                items.sort_by(|left, right| match python_compare(left, right) {
                    Ok(value) => value,
                    Err(message) => {
                        failure = Some(message);
                        std::cmp::Ordering::Equal
                    }
                });
                if let Some(message) = failure {
                    return Err(EvalError::with_span("TypeError", message, at));
                }
                Ok(CodeValue::List(items))
            }
            "all" | "any" => {
                if values.len() != 1 {
                    return Err(arity("takes exactly one iterable"));
                }
                let items = values[0].iterable().ok_or_else(|| {
                    EvalError::with_span("TypeError", "argument is not iterable", at)
                })?;
                if items.len() > 2048 {
                    return Err(EvalError::with_span(
                        "CODE_VALUE_LIMIT",
                        "boolean aggregate input exceeds 2048 items",
                        at,
                    ));
                }
                for item in items {
                    self.tick(None)?;
                    let truth = item.truthy();
                    if (name == "all" && !truth) || (name == "any" && truth) {
                        return Ok(CodeValue::Bool(truth));
                    }
                }
                Ok(CodeValue::Bool(name == "all"))
            }
            _ => Err(EvalError::with_span(
                "CODE_CAPABILITY_INPUT",
                "builtin call is not approved",
                at,
            )),
        }
    }

    fn block(&mut self, statements: &[Stmt]) -> std::result::Result<Option<Control>, EvalError> {
        for statement in statements {
            self.tick(Some(statement.span))?;
            match &statement.kind {
                StmtKind::Return(value) => {
                    return Ok(Some(Control::Return(match value {
                        Some(value) => self.eval(value)?,
                        None => CodeValue::Null,
                    })));
                }
                StmtKind::Assign(name, value) => {
                    let value = self.eval(value)?;
                    self.environment.insert(name.clone(), value);
                }
                StmtKind::AugAssign(name, operator, value) => {
                    let old = self.environment.get(name).cloned().ok_or_else(|| {
                        EvalError::with_span(
                            "UnboundLocalError",
                            format!("local variable `{name}` is unbound"),
                            statement.span,
                        )
                    })?;
                    if as_integer(&old).is_none() {
                        return Err(EvalError::with_span(
                            "TypeError",
                            "augmented assignment supports integers only",
                            statement.span,
                        ));
                    }
                    let right = self.eval(value)?;
                    if as_integer(&right).is_none() {
                        return Err(EvalError::with_span(
                            "TypeError",
                            "augmented assignment supports integers only",
                            statement.span,
                        ));
                    }
                    let updated = self.binary(*operator, old, right, statement.span)?;
                    self.environment.insert(name.clone(), updated);
                }
                StmtKind::If(test, body, otherwise) => {
                    let condition = self.eval(test)?.truthy();
                    if self.branches.len() < 128 {
                        self.branches.push(BranchObservation {
                            line: statement.span.line,
                            condition,
                        });
                    }
                    if let Some(control) = self.block(if condition { body } else { otherwise })? {
                        return Ok(Some(control));
                    }
                }
                StmtKind::For(name, iterable, body, otherwise) => {
                    let sequence = self.eval(iterable)?.iterable().ok_or_else(|| {
                        EvalError::with_span(
                            "TypeError",
                            "for-loop input is not an iterable",
                            statement.span,
                        )
                    })?;
                    if sequence.len() > 2048 {
                        return Err(EvalError::with_span(
                            "CODE_VALUE_LIMIT",
                            "for-loop input exceeds 2048 items",
                            statement.span,
                        ));
                    }
                    let mut broke = false;
                    for item in sequence {
                        self.tick(Some(statement.span))?;
                        self.environment.insert(name.clone(), item);
                        match self.block(body)? {
                            Some(Control::Return(value)) => {
                                return Ok(Some(Control::Return(value)));
                            }
                            Some(Control::Break) => {
                                broke = true;
                                break;
                            }
                            Some(Control::Continue) | None => {}
                        }
                    }
                    if !broke && let Some(control) = self.block(otherwise)? {
                        return Ok(Some(control));
                    }
                }
                StmtKind::Break => return Ok(Some(Control::Break)),
                StmtKind::Continue => return Ok(Some(Control::Continue)),
                StmtKind::Pass | StmtKind::Expression(_) => {}
            }
        }
        Ok(None)
    }
}

#[derive(Clone, Debug)]
enum Control {
    Return(CodeValue),
    Break,
    Continue,
}

fn as_integer(value: &CodeValue) -> Option<BigInt> {
    match value {
        CodeValue::Integer(value) => Some(value.clone()),
        CodeValue::Bool(value) => Some(BigInt::from(u8::from(*value))),
        _ => None,
    }
}

fn floor_div(left: &BigInt, right: &BigInt) -> BigInt {
    let quotient = left / right;
    let remainder = left % right;
    if !remainder.is_zero() && remainder.sign() != right.sign() {
        quotient - 1
    } else {
        quotient
    }
}
fn python_mod(left: &BigInt, right: &BigInt) -> BigInt {
    let remainder = left % right;
    if !remainder.is_zero() && remainder.sign() != right.sign() {
        remainder + right
    } else {
        remainder
    }
}

fn repeat_count(
    times: &BigInt,
    length: usize,
    maximum: usize,
) -> std::result::Result<usize, EvalError> {
    if times <= &BigInt::zero() || length == 0 {
        return Ok(0);
    }
    let factor = times.to_usize().ok_or_else(|| {
        EvalError::new(
            "CODE_VALUE_LIMIT",
            "sequence repetition exceeds the value envelope",
            None,
        )
    })?;
    if factor > maximum / length {
        return Err(EvalError::new(
            "CODE_VALUE_LIMIT",
            "sequence repetition exceeds the value envelope",
            None,
        ));
    }
    Ok(factor)
}
fn repeat_string(value: &str, times: &BigInt, at: Span) -> std::result::Result<String, EvalError> {
    let count = repeat_count(times, value.chars().count(), 8192).map_err(|mut error| {
        error.line = Some(at.line);
        error
    })?;
    Ok(value.repeat(count))
}
fn repeat_list(
    value: &[CodeValue],
    times: &BigInt,
    at: Span,
) -> std::result::Result<Vec<CodeValue>, EvalError> {
    let count = repeat_count(times, value.len(), 2048).map_err(|mut error| {
        error.line = Some(at.line);
        error
    })?;
    let mut output = Vec::with_capacity(value.len().saturating_mul(count));
    for _ in 0..count {
        output.extend_from_slice(value);
    }
    Ok(output)
}

fn sequence_index(
    value: &CodeValue,
    length: usize,
    sequence_type: &str,
    at: Span,
) -> std::result::Result<usize, EvalError> {
    let Some(mut index) = as_integer(value) else {
        return Err(EvalError::with_span(
            "TypeError",
            format!("{sequence_type} indices must be integers"),
            at,
        ));
    };
    if index.is_negative() {
        index += BigInt::from(length);
    }
    if index < BigInt::zero() || index >= BigInt::from(length) {
        return Err(EvalError::with_span(
            "IndexError",
            format!("{sequence_type} index out of range"),
            at,
        ));
    }
    index.to_usize().ok_or_else(|| {
        EvalError::with_span(
            "IndexError",
            format!("{sequence_type} index out of range"),
            at,
        )
    })
}

fn python_equal(left: &CodeValue, right: &CodeValue) -> bool {
    if let (Some(left), Some(right)) = (as_integer(left), as_integer(right)) {
        return left == right;
    }
    match (left, right) {
        (CodeValue::Null, CodeValue::Null) => true,
        (CodeValue::String(a), CodeValue::String(b)) => a == b,
        (CodeValue::List(a), CodeValue::List(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| python_equal(a, b))
        }
        (CodeValue::Dict(a), CodeValue::Dict(b)) => {
            a.len() == b.len()
                && a.iter().all(|(key, value)| {
                    b.iter()
                        .find(|(other, _)| key == other)
                        .is_some_and(|(_, other)| python_equal(value, other))
                })
        }
        (
            CodeValue::Range {
                start: a,
                step: astep,
                len: alen,
            },
            CodeValue::Range {
                start: b,
                step: bstep,
                len: blen,
            },
        ) => alen == blen && (alen == &0 || (a == b && (alen == &1 || astep == bstep))),
        _ => false,
    }
}

fn python_compare(
    left: &CodeValue,
    right: &CodeValue,
) -> std::result::Result<std::cmp::Ordering, String> {
    if let (Some(left), Some(right)) = (as_integer(left), as_integer(right)) {
        return Ok(left.cmp(&right));
    }
    match (left, right) {
        (CodeValue::String(a), CodeValue::String(b)) => Ok(a.cmp(b)),
        (CodeValue::List(a), CodeValue::List(b)) => {
            for (left, right) in a.iter().zip(b) {
                if python_equal(left, right) {
                    continue;
                }
                return python_compare(left, right);
            }
            Ok(a.len().cmp(&b.len()))
        }
        _ => Err(format!(
            "`<` is unsupported between {} and {}",
            left.type_name(),
            right.type_name()
        )),
    }
}

fn contains(container: &CodeValue, value: &CodeValue) -> Option<bool> {
    match container {
        CodeValue::List(values) => Some(values.iter().any(|item| python_equal(item, value))),
        CodeValue::Dict(values) => match value {
            CodeValue::String(key) => Some(values.iter().any(|(candidate, _)| candidate == key)),
            _ => Some(false),
        },
        CodeValue::String(text) => match value {
            CodeValue::String(needle) => Some(text.contains(needle)),
            _ => None,
        },
        CodeValue::Range { start, step, len } => {
            let integer = as_integer(value)?;
            if *len == 0 {
                return Some(false);
            }
            let offset = &integer - start;
            if !(&offset % step).is_zero() {
                return Some(false);
            }
            let position = offset / step;
            Some(position >= BigInt::zero() && position < BigInt::from(*len))
        }
        _ => None,
    }
}

fn make_range(values: &[CodeValue]) -> std::result::Result<CodeValue, String> {
    if !(1..=3).contains(&values.len()) {
        return Err("range expected 1 to 3 integer arguments".into());
    }
    let args = values
        .iter()
        .map(|value| {
            as_integer(value).ok_or_else(|| "range arguments must be integers".to_string())
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let (start, stop, step) = match args.as_slice() {
        [stop] => (BigInt::zero(), stop.clone(), BigInt::one()),
        [start, stop] => (start.clone(), stop.clone(), BigInt::one()),
        [start, stop, step] => (start.clone(), stop.clone(), step.clone()),
        _ => unreachable!(),
    };
    if step.is_zero() {
        return Err("range() arg 3 must not be zero".into());
    }
    let distance = if step.is_positive() {
        &stop - &start
    } else {
        &start - &stop
    };
    let stride = step.abs();
    let length = if distance <= BigInt::zero() {
        BigInt::zero()
    } else {
        (&distance + &stride - 1) / &stride
    };
    let length = length
        .to_usize()
        .filter(|length| *length <= 2048)
        .ok_or_else(|| "range length exceeds 2048".to_string())?;
    Ok(CodeValue::Range {
        start,
        step,
        len: length,
    })
}

fn check_runtime_value(value: &CodeValue, internal: bool) -> std::result::Result<(), String> {
    fn visit(
        value: &CodeValue,
        internal: bool,
        depth: usize,
        count: &mut usize,
        text_bytes: &mut usize,
    ) -> std::result::Result<(), String> {
        *count += 1;
        if *count > 4096 || depth > 16 {
            return Err("value node/depth envelope exceeded".into());
        }
        match value {
            CodeValue::Null | CodeValue::Bool(_) => Ok(()),
            CodeValue::Integer(value) if value.bits() <= 256 => Ok(()),
            CodeValue::Integer(_) => Err("integer exceeds 256 bits".into()),
            CodeValue::String(value) => {
                *text_bytes = text_bytes.saturating_add(value.len());
                if value.chars().count() > 8192 || *text_bytes > 65_536 {
                    Err("string/text envelope exceeded".into())
                } else {
                    Ok(())
                }
            }
            CodeValue::List(values) if values.len() <= 2048 => {
                for value in values {
                    visit(value, internal, depth + 1, count, text_bytes)?;
                }
                Ok(())
            }
            CodeValue::List(_) => Err("list exceeds 2048 items".into()),
            CodeValue::Dict(values) if values.len() <= 2048 => {
                for (key, value) in values {
                    *count += 1;
                    *text_bytes = text_bytes.saturating_add(key.len());
                    if *count > 4096
                        || depth + 1 > 16
                        || key.chars().count() > 8192
                        || *text_bytes > 65_536
                    {
                        return Err("dictionary key/value envelope exceeded".into());
                    }
                    visit(value, internal, depth + 1, count, text_bytes)?;
                }
                Ok(())
            }
            CodeValue::Dict(_) => Err("dictionary exceeds 2048 items".into()),
            CodeValue::Range { len, .. } if internal && *len <= 2048 => Ok(()),
            CodeValue::Range { .. } => Err("range may only be used internally".into()),
        }
    }
    let mut count = 0;
    let mut text_bytes = 0;
    visit(value, internal, 0, &mut count, &mut text_bytes)
}

fn collect_locals(statements: &[Stmt], names: &mut std::collections::BTreeSet<String>) {
    for statement in statements {
        match &statement.kind {
            StmtKind::Assign(name, _) | StmtKind::AugAssign(name, _, _) => {
                names.insert(name.clone());
            }
            StmtKind::For(name, _, body, otherwise) => {
                names.insert(name.clone());
                collect_locals(body, names);
                collect_locals(otherwise, names);
            }
            StmtKind::If(_, body, otherwise) => {
                collect_locals(body, names);
                collect_locals(otherwise, names);
            }
            _ => {}
        }
    }
}

fn evaluate(
    function: &Function,
    _source: &str,
    arguments: &CodeValue,
    max_steps: usize,
    deadline: Option<Instant>,
) -> Result<CodeEvaluationReport> {
    let CodeValue::Dict(arguments) = arguments else {
        return Err(input_error("arguments must be an object"));
    };
    let names = function
        .args
        .iter()
        .map(|argument| argument.name.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let supplied = arguments
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let arguments_sha256 = digest(CodeValue::Dict(arguments.clone()).to_json(true)?.as_bytes());
    if names != supplied {
        return Ok(CodeEvaluationReport {
            status: "FAILED".into(),
            actual_json: None,
            result_type: None,
            arguments_sha256,
            steps: 0,
            max_steps,
            branches: Vec::new(),
            diagnostic: Some(
                "CODE_CAPABILITY_INPUT: argument names must exactly match the function signature"
                    .into(),
            ),
            line: None,
            canonical_json: None,
        });
    }
    let mut environment = BTreeMap::new();
    for (name, value) in arguments {
        environment.insert(name.clone(), value.clone());
    }
    let mut locals = std::collections::BTreeSet::new();
    for argument in &function.args {
        locals.insert(argument.name.clone());
    }
    collect_locals(&function.body, &mut locals);
    let mut machine = Evaluator {
        environment,
        locals,
        max_steps,
        steps: 0,
        current_line: None,
        branches: Vec::new(),
        deadline,
    };
    let result = machine.block(&function.body);
    let (status, value, diagnostic, line) = match result {
        Ok(Some(Control::Return(value))) => ("COMPUTED", Some(value), None, None),
        Ok(Some(Control::Break | Control::Continue)) => (
            "FAILED",
            None,
            Some("SyntaxError: loop control escaped its loop".into()),
            machine.current_line,
        ),
        Ok(None) => ("COMPUTED", Some(CodeValue::Null), None, None),
        Err(error) => {
            let status = if error.kind == "DEADLINE" || error.kind == "STEP_LIMIT" {
                "STOPPED"
            } else {
                "FAILED"
            };
            (
                status,
                None,
                Some(error.diagnostic()),
                error.line.or(machine.current_line),
            )
        }
    };
    let actual_json = if let Some(value) = &value {
        match check_runtime_value(value, false) {
            Ok(()) => Some(value.to_json(false)?),
            Err(message) => {
                return Ok(CodeEvaluationReport {
                    status: "FAILED".into(),
                    actual_json: None,
                    result_type: Some(value.type_name().into()),
                    arguments_sha256,
                    steps: machine.steps,
                    max_steps,
                    branches: machine.branches,
                    diagnostic: Some(format!("CODE_VALUE_LIMIT: {message}")),
                    line: machine.current_line,
                    canonical_json: None,
                });
            }
        }
    } else {
        None
    };
    Ok(CodeEvaluationReport {
        status: status.into(),
        result_type: value.as_ref().map(|value| value.type_name().to_string()),
        actual_json,
        arguments_sha256,
        steps: machine.steps,
        max_steps,
        branches: machine.branches,
        diagnostic,
        line,
        canonical_json: value.as_ref().and_then(|value| value.to_json(true).ok()),
    })
}

fn verify(
    function: &Function,
    source: &str,
    cases: &[(CodeValue, CodeValue)],
    max_steps: usize,
    deadline: Option<Instant>,
) -> Result<CodeVerificationReport> {
    let case_value = CodeValue::List(
        cases
            .iter()
            .map(|(arguments, expected)| {
                CodeValue::Dict(vec![
                    ("引数".into(), arguments.clone()),
                    ("期待値".into(), expected.clone()),
                ])
            })
            .collect(),
    );
    let cases_sha256 = digest(case_value.to_json(true)?.as_bytes());
    let mut observations = Vec::with_capacity(cases.len());
    let mut stopped = false;
    for (index, (arguments, expected)) in cases.iter().enumerate() {
        if stopped {
            observations.push(CodeCaseObservation {
                number: index,
                matched: false,
                evaluation_status: "NOT_RUN".into(),
                actual_json: None,
                expected_json: expected.to_json(false)?,
                steps: 0,
                diagnostic: Some("A prior case reached a finite stop".into()),
                line: None,
            });
            continue;
        }
        let evaluated = evaluate(function, source, arguments, max_steps, deadline)?;
        let expected_json = expected.to_json(false)?;
        let expected_canonical = expected.to_json(true)?;
        let actual = evaluated.actual_json.clone();
        let matched = evaluated.status == "COMPUTED"
            && evaluated.canonical_json.as_deref() == Some(expected_canonical.as_str())
            && evaluated.result_type.as_deref() == Some(expected.type_name());
        stopped = evaluated.status == "STOPPED";
        observations.push(CodeCaseObservation {
            number: index,
            matched,
            evaluation_status: evaluated.status,
            actual_json: actual,
            expected_json,
            steps: evaluated.steps,
            diagnostic: evaluated.diagnostic,
            line: evaluated.line,
        });
    }
    let all_matched = observations.iter().all(|case| case.matched);
    Ok(CodeVerificationReport {
        cases_sha256,
        case_count: observations.len(),
        all_matched,
        cases: observations,
        assurance: "一致は指定された有限入力に限る。全入力での正しさ、型安全性、一般Python互換性を証明しない".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(value: serde_json::Value) -> String {
        serde_json::to_string(&value).unwrap()
    }

    #[test]
    fn bounded_reader_retains_utf8_source_segments_and_ast_bounds() {
        let source = "def 合計(xs):\n    \"合計を返す\"\n    total = 0\n    for x in xs:\n        if x > 0:\n            total += x\n    return total\n";
        let document = request(serde_json::json!({"情報源":source}));
        let report = run(&document, CodeOperation::Read, None).unwrap();
        assert_eq!(report.function_name, "合計");
        assert_eq!(report.parameters, ["xs"]);
        assert!(report.ast_nodes > report.structure.len());
        assert!(report.ast_depth <= 40);
        assert!(
            report
                .structure
                .iter()
                .any(|node| node.syntax == "For" && node.source_segment.contains("for x in xs"))
        );
        assert!(
            report
                .structure
                .iter()
                .any(|node| node.syntax == "If" && node.line == 5 && node.utf8_column == 8)
        );
    }

    #[test]
    fn parenthesized_operands_remain_inside_parent_source_segments() {
        let source = "def f(x):\n    return (False and missing) or (x if x else True)\n";
        let document = request(serde_json::json!({"情報源":source}));
        let report = run(&document, CodeOperation::Read, None).unwrap();
        assert_eq!(report.structure[0].source_segment, source.trim_end());
        assert_eq!(
            report.structure[1].source_segment,
            "return (False and missing) or (x if x else True)"
        );
    }

    #[test]
    fn evaluator_preserves_python_floor_mod_bool_and_dictionary_order() {
        let source = "def calc(d):\n    return [(-7 // 3), (-7 % 3), (True + True), (False or 4), list(d)]\n";
        let document = format!(
            "{{\"情報源\":{},\"引数\":{{\"d\":{{\"z\":1,\"a\":2}}}}}}",
            serde_json::to_string(source).unwrap()
        );
        let report = run(&document, CodeOperation::Evaluate, None).unwrap();
        let evaluation = report.evaluation.unwrap();
        assert_eq!(evaluation.status, "COMPUTED");
        assert_eq!(
            evaluation.actual_json.as_deref(),
            Some("[-3,2,2,4,[\"z\",\"a\"]]")
        );
    }

    #[test]
    fn verifier_distinguishes_bool_from_integer_and_enforces_step_budget() {
        let bool_source = "def answer():\n    return True\n";
        let document = request(serde_json::json!({"情報源":bool_source,"試験":[
            {"引数":{},"期待値":true}, {"引数":{},"期待値":1}
        ]}));
        let report = run(&document, CodeOperation::Verify, None).unwrap();
        let verification = report.verification.unwrap();
        assert_eq!(verification.case_count, 2);
        assert!(!verification.all_matched);
        assert!(verification.cases[0].matched);
        assert!(!verification.cases[1].matched);

        let source = "def loop(xs):\n    total = 0\n    for x in xs:\n        total += x\n    return total\n";
        let document =
            request(serde_json::json!({"情報源":source,"引数":{"xs":[1,2,3]},"最大手数":2}));
        let report = run(&document, CodeOperation::Evaluate, None).unwrap();
        assert_eq!(report.evaluation.unwrap().status, "STOPPED");
    }

    #[test]
    fn sequence_index_reports_type_and_out_of_range_errors_separately() {
        let source = "def item(xs, i):\n    return xs[i]\n";
        let huge_index =
            "1000000000000000000000000000000000000000000000000000000000000000000000000";
        let document = format!(
            "{{\"情報源\":{},\"引数\":{{\"xs\":[1],\"i\":{huge_index}}}}}",
            serde_json::to_string(source).unwrap()
        );
        let report = run(&document, CodeOperation::Evaluate, None).unwrap();
        assert!(
            report
                .evaluation
                .unwrap()
                .diagnostic
                .unwrap()
                .starts_with("IndexError:")
        );

        let document = request(serde_json::json!({
            "情報源": source,
            "引数": {"xs": [1], "i": "0"}
        }));
        let report = run(&document, CodeOperation::Evaluate, None).unwrap();
        assert!(
            report
                .evaluation
                .unwrap()
                .diagnostic
                .unwrap()
                .starts_with("TypeError:")
        );
    }

    #[test]
    fn whole_source_rejects_import_attributes_and_break_in_for_else() {
        for source in [
            "import os\ndef f():\n    return 1\n",
            "def f(x):\n    return x.__class__\n",
            "def f(xs):\n    for x in xs:\n        pass\n    else:\n        break\n    return 0\n",
        ] {
            let document = request(serde_json::json!({"情報源":source}));
            assert!(run(&document, CodeOperation::Read, None).is_err());
        }
    }
}

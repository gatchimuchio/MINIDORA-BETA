//! Exact arithmetic and an inherited non-neural character sequence model.
//! Model probabilities rank surface candidates; they never establish facts.
use minidora_domain::{Calculation, Result, failure, identity};
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, Signed, Zero};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use unicode_normalization::UnicodeNormalization;

const EOS: &str = "<EOS>";
const UNK: &str = "<UNK>";
const BOS: &str = "<BOS>";
const MAX_EXPRESSION_BYTES: usize = 8192;
const MAX_NUMBER_DIGITS: usize = 1024;
const MAX_NESTING: usize = 64;
const MAX_OPERATIONS: usize = 2048;
const MAX_INTEGER_BITS: u64 = 32768;
// Bounded exact-model admission, distinct from a machine-wide resource claim.
const MAX_MODEL_BYTES: usize = 16_384;
const MAX_MODEL_TOKENS: usize = 16_384;
const MAX_MODEL_CONTEXTS: usize = 65_536;
const MAX_MODEL_TRANSITIONS: usize = 262_144;
const MAX_MODEL_VOCABULARY: usize = 4096;
const MAX_PROBABILITY_BITS: u64 = 8192;
fn model_deadline() -> Instant {
    Instant::now() + Duration::from_secs(30)
}
fn poll_model(deadline: Instant) -> Result<()> {
    if Instant::now() >= deadline {
        return Err(failure(
            "MODEL_DEADLINE",
            "the task's absolute deadline elapsed during exact language computation",
        ));
    }
    Ok(())
}
fn model_bytes(length: usize) -> Result<()> {
    if length > MAX_MODEL_BYTES {
        return Err(failure(
            "MODEL_RESOURCE_LIMIT",
            "model source/sequence exceeds the explicit 16384-byte envelope",
        ));
    }
    Ok(())
}
fn probability_product(
    left: BigRational,
    right: BigRational,
    deadline: Instant,
) -> Result<BigRational> {
    poll_model(deadline)?;
    // Bound the unreduced multiplication before allocating. Rejecting an
    // expensive exact product is explicit; no digits or probability are faked.
    if left.numer().bits().saturating_add(right.numer().bits()) > MAX_PROBABILITY_BITS
        || left.denom().bits().saturating_add(right.denom().bits()) > MAX_PROBABILITY_BITS
    {
        return Err(failure(
            "MODEL_RESOURCE_LIMIT",
            "exact probability multiplication exceeds the 8192-bit operation envelope",
        ));
    }
    let product = left * right;
    if product.numer().bits() > MAX_PROBABILITY_BITS
        || product.denom().bits() > MAX_PROBABILITY_BITS
    {
        return Err(failure(
            "MODEL_RESOURCE_LIMIT",
            "exact probability exceeds the 8192-bit result envelope",
        ));
    }
    poll_model(deadline)?;
    Ok(product)
}

/// Exact rational arithmetic, without eval, floating point, names or calls.
/// Limits are this parser's explicit admission envelope, not machine-wide limits.
pub fn calculate(expression: &str) -> Result<Calculation> {
    if expression.len() > MAX_EXPRESSION_BYTES {
        return Err(failure("EXPRESSION_LIMIT", "expression exceeds 8192 bytes"));
    }
    let normalized: String = expression
        .nfkc()
        .collect::<String>()
        .replace('×', "*")
        .replace('÷', "/")
        .replace('−', "-");
    if let Some(calculation) = calculate_unit_addition(expression, &normalized)? {
        return Ok(calculation);
    }
    let mut parser = ExpressionParser {
        text: normalized.as_bytes(),
        at: 0,
        operations: 0,
        steps: Vec::new(),
    };
    let value = parser.expression(0)?;
    parser.space();
    if parser.at != parser.text.len() {
        return Err(failure(
            "INVALID_EXPRESSION",
            format!("unexpected input at byte {}", parser.at),
        ));
    }
    Ok(Calculation {
        expression: expression.into(),
        exact_value: rational_text(&value),
        steps: parser.steps,
    })
}

fn calculate_unit_addition(original: &str, normalized: &str) -> Result<Option<Calculation>> {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    let pattern = PATTERN.get_or_init(|| {
        Regex::new(r"^\s*([+-]?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][+-]?[0-9]+)?)\s*([A-Za-zμµΩ°%][A-Za-z0-9μµΩ°%*/^._-]{0,63})\s*([+-])\s*([+-]?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][+-]?[0-9]+)?)\s*([A-Za-zμµΩ°%][A-Za-z0-9μµΩ°%*/^._-]{0,63})\s*$")
            .expect("unit arithmetic expression pattern is static")
    });
    let Some(captures) = pattern.captures(normalized) else {
        return Ok(None);
    };
    let left = exact_rational(captures.get(1).expect("left value exists").as_str())?;
    let left_unit = captures.get(2).expect("left unit exists").as_str();
    let operation = captures.get(3).expect("operator exists").as_str();
    let right = exact_rational(captures.get(4).expect("right value exists").as_str())?;
    let right_unit = captures.get(5).expect("right unit exists").as_str();
    ExpressionParser::check(&left)?;
    ExpressionParser::check(&right)?;

    let (left_value, right_value, unit) = if left_unit == right_unit {
        (left.clone(), right.clone(), left_unit.to_string())
    } else {
        let left_scale = crate::math::exact_length_scale_to_meters(left_unit);
        let right_scale = crate::math::exact_length_scale_to_meters(right_unit);
        let (Some(left_scale), Some(right_scale)) = (left_scale, right_scale) else {
            return Err(failure(
                "UNIT_MISMATCH",
                "mixed addition or subtraction requires one verified conversion family; supported mixed units are mm, cm, m, and km",
            ));
        };
        (&left * &left_scale, &right * &right_scale, "m".to_string())
    };
    ExpressionParser::check(&left_value)?;
    ExpressionParser::check(&right_value)?;
    let result = if operation == "+" {
        &left_value + &right_value
    } else {
        &left_value - &right_value
    };
    ExpressionParser::check(&result)?;

    let mut steps = Vec::new();
    if left_unit != right_unit {
        steps.push(format!(
            "{} {} = {} m",
            rational_text(&left),
            left_unit,
            rational_text(&left_value)
        ));
        steps.push(format!(
            "{} {} = {} m",
            rational_text(&right),
            right_unit,
            rational_text(&right_value)
        ));
    }
    steps.push(format!(
        "{} {} {} {} {} = {} {}",
        rational_text(&left_value),
        unit,
        operation,
        rational_text(&right_value),
        unit,
        rational_text(&result),
        unit
    ));
    Ok(Some(Calculation {
        expression: original.into(),
        exact_value: format!("{} {}", rational_text(&result), unit),
        steps,
    }))
}

fn exact_rational(expression: &str) -> Result<BigRational> {
    let calculation = calculate(expression)?;
    BigRational::from_str(&calculation.exact_value).map_err(|_| {
        failure(
            "ARITHMETIC_INTERNAL",
            "exact scalar result could not be read as a rational number",
        )
    })
}

/// Return a new exact result after replacing one literal operand in a saved
/// binary calculation. Unit-bearing corrections are restricted to addition
/// and subtraction; the caller retains the prior result as a dependency.
pub fn correct_binary_operand(
    prior: &Calculation,
    operand_index: usize,
    replacement: &str,
) -> Result<Calculation> {
    static OPERANDS: OnceLock<Regex> = OnceLock::new();
    static REPLACEMENT: OnceLock<Regex> = OnceLock::new();
    let operands = OPERANDS.get_or_init(|| {
        Regex::new(r"^\s*([+-]?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][+-]?[0-9]+)?)\s*([A-Za-zμµΩ°%][A-Za-z0-9μµΩ°%*/^._-]{0,63})?\s*(\*\*|[-+*/%^])\s*([+-]?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][+-]?[0-9]+)?)\s*([A-Za-zμµΩ°%][A-Za-z0-9μµΩ°%*/^._-]{0,63})?\s*$")
            .expect("binary operand expression pattern is static")
    });
    let replacement_pattern = REPLACEMENT.get_or_init(|| {
        Regex::new(r"^\s*([+-]?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][+-]?[0-9]+)?)\s*([A-Za-zμµΩ°%][A-Za-z0-9μµΩ°%*/^._-]{0,63})?\s*$")
            .expect("operand replacement pattern is static")
    });
    if operand_index > 1 {
        return Err(failure(
            "CORRECTION_TARGET_INVALID",
            "a binary calculation has only operand 1 and operand 2",
        ));
    }
    let verified_prior = calculate(&prior.expression)?;
    if verified_prior.exact_value != prior.exact_value {
        return Err(failure(
            "CORRECTION_SOURCE_MISMATCH",
            "saved result value does not match its original expression",
        ));
    }
    let normalized_expression: String = prior.expression.nfkc().collect();
    let Some(captures) = operands.captures(&normalized_expression) else {
        return Err(failure(
            "CORRECTION_UNSUPPORTED",
            "only a saved binary literal arithmetic expression can be corrected",
        ));
    };
    let normalized_replacement: String = replacement.nfkc().collect();
    let Some(new) = replacement_pattern.captures(&normalized_replacement) else {
        return Err(failure(
            "CORRECTION_REPLACEMENT_INVALID",
            "replacement must be one exact numeric literal with an optional explicit unit",
        ));
    };
    let prior_left = captures[1].to_string();
    let prior_left_unit = captures.get(2).map(|value| value.as_str());
    let operation = captures[3].to_string();
    let prior_right = captures[4].to_string();
    let prior_right_unit = captures.get(5).map(|value| value.as_str());
    let new_value = new[1].to_string();
    let supplied_unit = new.get(2).map(|value| value.as_str());
    let target_prior_unit = if operand_index == 0 {
        prior_left_unit
    } else {
        prior_right_unit
    };
    let new_unit = if operation == "+" || operation == "-" {
        match (target_prior_unit, supplied_unit) {
            (None, Some(_)) => {
                return Err(failure(
                    "UNIT_MISMATCH",
                    "a unit cannot be added to a previously unitless operand during correction",
                ));
            }
            (Some(prior_unit), None) => Some(prior_unit.to_string()),
            (_, Some(supplied)) => Some(supplied.to_string()),
            (None, None) => None,
        }
    } else {
        if prior_left_unit.is_some() || prior_right_unit.is_some() || supplied_unit.is_some() {
            return Err(failure(
                "UNIT_OPERATION_UNSUPPORTED",
                "unit-bearing multiplication, division, remainder and power corrections are unsupported",
            ));
        }
        None
    };
    let left = if operand_index == 0 {
        (new_value.clone(), new_unit.clone())
    } else {
        (prior_left, prior_left_unit.map(str::to_string))
    };
    let right = if operand_index == 1 {
        (new_value, new_unit)
    } else {
        (prior_right, prior_right_unit.map(str::to_string))
    };
    let format_quantity = |value: &str, unit: Option<String>| match unit {
        Some(unit) => format!("{value} {unit}"),
        None => value.to_string(),
    };
    let expression = format!(
        "{} {} {}",
        format_quantity(&left.0, left.1),
        operation,
        format_quantity(&right.0, right.1)
    );
    calculate(&expression)
}

pub fn rational_text(value: &BigRational) -> String {
    if value.is_integer() {
        value.to_integer().to_string()
    } else {
        format!("{}/{}", value.numer(), value.denom())
    }
}

struct ExpressionParser<'a> {
    text: &'a [u8],
    at: usize,
    operations: usize,
    steps: Vec<String>,
}
impl ExpressionParser<'_> {
    fn space(&mut self) {
        while self.text.get(self.at).is_some_and(u8::is_ascii_whitespace) {
            self.at += 1;
        }
    }
    fn take(&mut self, ch: u8) -> bool {
        self.space();
        if self.text.get(self.at) == Some(&ch) {
            self.at += 1;
            true
        } else {
            false
        }
    }
    fn check(value: &BigRational) -> Result<()> {
        if value.numer().bits() > MAX_INTEGER_BITS || value.denom().bits() > MAX_INTEGER_BITS {
            return Err(failure(
                "ARITHMETIC_LIMIT",
                "rational exceeds the 32768-bit admission envelope",
            ));
        }
        Ok(())
    }
    fn record(
        &mut self,
        lhs: &BigRational,
        op: &str,
        rhs: &BigRational,
        value: &BigRational,
    ) -> Result<()> {
        self.operations += 1;
        if self.operations > MAX_OPERATIONS {
            return Err(failure("ARITHMETIC_LIMIT", "operation budget exceeded"));
        }
        Self::check(value)?;
        self.steps.push(format!(
            "{} {} {} = {}",
            rational_text(lhs),
            op,
            rational_text(rhs),
            rational_text(value)
        ));
        Ok(())
    }
    fn expression(&mut self, depth: usize) -> Result<BigRational> {
        let mut value = self.term(depth)?;
        loop {
            let op = if self.take(b'+') {
                "+"
            } else if self.take(b'-') {
                "-"
            } else {
                break;
            };
            let rhs = self.term(depth)?;
            let next = if op == "+" {
                &value + &rhs
            } else {
                &value - &rhs
            };
            self.record(&value, op, &rhs, &next)?;
            value = next;
        }
        Ok(value)
    }
    fn term(&mut self, depth: usize) -> Result<BigRational> {
        let mut value = self.unary(depth)?;
        loop {
            self.space();
            let op = if self.text.get(self.at..self.at + 2) == Some(b"**") {
                break;
            } else if self.take(b'*') {
                "*"
            } else if self.take(b'/') {
                "/"
            } else if self.take(b'%') {
                "%"
            } else {
                break;
            };
            let rhs = self.unary(depth)?;
            if (op == "/" || op == "%") && rhs.is_zero() {
                return Err(failure("DIVISION_BY_ZERO", "zero divisor"));
            }
            let next = match op {
                "*" => &value * &rhs,
                "/" => &value / &rhs,
                _ => {
                    if !value.is_integer() || !rhs.is_integer() {
                        return Err(failure(
                            "INTEGER_REQUIRED",
                            "remainder requires integer operands",
                        ));
                    }
                    BigRational::from_integer(value.to_integer() % rhs.to_integer())
                }
            };
            self.record(&value, op, &rhs, &next)?;
            value = next;
        }
        Ok(value)
    }
    fn unary(&mut self, depth: usize) -> Result<BigRational> {
        if depth >= MAX_NESTING {
            return Err(failure("EXPRESSION_DEPTH", "expression nesting exceeds 64"));
        }
        if self.take(b'+') {
            self.unary(depth + 1)
        } else if self.take(b'-') {
            Ok(-self.unary(depth + 1)?)
        } else {
            self.power(depth + 1)
        }
    }
    fn power(&mut self, depth: usize) -> Result<BigRational> {
        let value = self.atom(depth)?;
        self.space();
        let has_power = if self.text.get(self.at..self.at + 2) == Some(b"**") {
            self.at += 2;
            true
        } else {
            self.take(b'^')
        };
        if !has_power {
            return Ok(value);
        }
        let rhs = self.unary(depth + 1)?;
        if !rhs.is_integer() || rhs.abs() > BigRational::from_integer(BigInt::from(1024)) {
            return Err(failure(
                "EXPONENT_LIMIT",
                "exponent must be an integer in -1024..1024",
            ));
        }
        if value.is_zero() && rhs <= BigRational::zero() {
            return Err(failure(
                "UNDEFINED_POWER",
                "zero to a nonpositive power is undefined",
            ));
        }
        let exponent: i32 = rhs
            .to_integer()
            .to_string()
            .parse()
            .map_err(|_| failure("EXPONENT_LIMIT", "invalid exponent"))?;
        let next = value.pow(exponent);
        self.record(&value, "^", &rhs, &next)?;
        Ok(next)
    }
    fn atom(&mut self, depth: usize) -> Result<BigRational> {
        if self.take(b'(') {
            let value = self.expression(depth + 1)?;
            if !self.take(b')') {
                return Err(failure("INVALID_EXPRESSION", "missing closing parenthesis"));
            }
            return Ok(value);
        }
        self.space();
        let start = self.at;
        let mut digits = 0usize;
        while self.text.get(self.at).is_some_and(u8::is_ascii_digit) {
            self.at += 1;
            digits += 1;
        }
        let mut decimals = 0usize;
        if self.text.get(self.at) == Some(&b'.') {
            self.at += 1;
            while self.text.get(self.at).is_some_and(u8::is_ascii_digit) {
                self.at += 1;
                digits += 1;
                decimals += 1;
            }
        }
        if digits == 0 {
            return Err(failure(
                "INVALID_EXPRESSION",
                format!("number expected at byte {start}"),
            ));
        }
        if digits > MAX_NUMBER_DIGITS {
            return Err(failure("NUMBER_LIMIT", "number exceeds 1024 digits"));
        }
        let body = std::str::from_utf8(&self.text[start..self.at])
            .map_err(|_| failure("INVALID_EXPRESSION", "invalid number encoding"))?;
        let numerator = BigInt::parse_bytes(body.replace('.', "").as_bytes(), 10)
            .ok_or_else(|| failure("INVALID_EXPRESSION", "invalid number"))?;
        let mut value = BigRational::new(numerator, BigInt::from(10).pow(decimals as u32));
        if matches!(self.text.get(self.at), Some(b'e' | b'E')) {
            self.at += 1;
            let exp_start = self.at;
            if matches!(self.text.get(self.at), Some(b'+' | b'-')) {
                self.at += 1;
            }
            let exp_digits = self.at;
            while self.text.get(self.at).is_some_and(u8::is_ascii_digit) {
                self.at += 1;
            }
            if self.at == exp_digits || self.at - exp_digits > 4 {
                return Err(failure("EXPONENT_LIMIT", "invalid decimal exponent"));
            }
            let exponent: i32 = std::str::from_utf8(&self.text[exp_start..self.at])
                .unwrap_or("")
                .parse()
                .map_err(|_| failure("EXPONENT_LIMIT", "invalid decimal exponent"))?;
            if exponent.unsigned_abs() > 1024 {
                return Err(failure("EXPONENT_LIMIT", "decimal exponent exceeds 1024"));
            }
            let factor = BigRational::from_integer(BigInt::from(10).pow(exponent.unsigned_abs()));
            value = if exponent >= 0 {
                value * factor
            } else {
                value / factor
            };
        }
        Self::check(&value)?;
        Ok(value)
    }
}

/// The inherited character n-gram implementation choice is explicit. Source
/// text is preserved by the input bundle; NFKC applies only to this LM view.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SequenceModel {
    pub version: String,
    pub order: usize,
    pub smoothing: u64,
    pub vocabulary: BTreeSet<String>,
    pub counts: BTreeMap<String, BTreeMap<String, u64>>,
    pub document_count: u64,
}
impl Default for SequenceModel {
    fn default() -> Self {
        Self {
            version: "inherited-char-ngram-v1".into(),
            order: 3,
            smoothing: 1,
            vocabulary: BTreeSet::from([EOS.into(), UNK.into()]),
            counts: BTreeMap::new(),
            document_count: 0,
        }
    }
}

pub const LOSSLESS_TOKEN_ADAPTER: &str = "literal-raw-byte-fallback-v1";
/// Disjoint literal bytes and known Unicode scalar tokens make the projection
/// reversible even for invalid UTF-8. EOS is a model symbol, never a raw byte.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LosslessToken {
    KnownChar { symbol: String },
    LiteralByte { value: u8 },
    End,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LosslessTokenProjection {
    pub version: String,
    pub raw_sha256: String,
    pub tokens: Vec<LosslessToken>,
}
impl LosslessTokenProjection {
    pub fn recover_bytes(&self) -> Result<Vec<u8>> {
        if self.version != LOSSLESS_TOKEN_ADAPTER {
            return Err(failure(
                "TOKEN_ADAPTER_VERSION",
                "unsupported lossless token projection",
            ));
        }
        let mut bytes = Vec::new();
        for token in &self.tokens {
            match token {
                LosslessToken::KnownChar { symbol } if symbol.chars().count() == 1 => {
                    bytes.extend_from_slice(symbol.as_bytes())
                }
                LosslessToken::LiteralByte { value } => bytes.push(*value),
                _ => {
                    return Err(failure(
                        "INVALID_LITERAL_TOKEN",
                        "raw projection contains an invalid character or EOS",
                    ));
                }
            }
        }
        if minidora_domain::digest(&bytes) != self.raw_sha256 {
            return Err(failure(
                "TOKEN_PROJECTION_INTEGRITY",
                "literal token reconstruction digest mismatch",
            ));
        }
        Ok(bytes)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelEmission {
    pub text: String,
    pub model_revision: String,
    pub sequence_probability: String,
    pub token_count: usize,
    pub objective: String,
    pub decoder_version: String,
    pub context_hashes: Vec<String>,
    pub lossless_projection: LosslessTokenProjection,
}
impl SequenceModel {
    /// A versioned successor projection. The inherited NFKC/UNK methods remain
    /// unchanged and their existing comparison vectors still apply only there.
    pub fn project_bytes(&self, bytes: &[u8]) -> Result<LosslessTokenProjection> {
        self.project_bytes_until(bytes, model_deadline())
    }
    fn project_bytes_until(
        &self,
        bytes: &[u8],
        deadline: Instant,
    ) -> Result<LosslessTokenProjection> {
        poll_model(deadline)?;
        model_bytes(bytes.len())?;
        let mut tokens = Vec::new();
        let mut at = 0;
        while at < bytes.len() {
            poll_model(deadline)?;
            let (valid_length, invalid_length) = match std::str::from_utf8(&bytes[at..]) {
                Ok(value) => (value.len(), 0),
                Err(error) => (
                    error.valid_up_to(),
                    error
                        .error_len()
                        .unwrap_or(bytes.len() - at - error.valid_up_to()),
                ),
            };
            let valid = std::str::from_utf8(&bytes[at..at + valid_length]).map_err(|_| {
                failure(
                    "TOKEN_PROJECTION_INTEGRITY",
                    "invalid validated UTF-8 prefix",
                )
            })?;
            for ch in valid.chars() {
                poll_model(deadline)?;
                let symbol = ch.to_string();
                if self.vocabulary.contains(&symbol) {
                    tokens.push(LosslessToken::KnownChar { symbol });
                } else {
                    let mut buffer = [0u8; 4];
                    for value in ch.encode_utf8(&mut buffer).as_bytes() {
                        tokens.push(LosslessToken::LiteralByte { value: *value });
                    }
                }
            }
            at += valid_length;
            for value in &bytes[at..at + invalid_length] {
                tokens.push(LosslessToken::LiteralByte { value: *value });
            }
            at += invalid_length;
        }
        poll_model(deadline)?;
        Ok(LosslessTokenProjection {
            version: LOSSLESS_TOKEN_ADAPTER.into(),
            raw_sha256: minidora_domain::digest(bytes),
            tokens,
        })
    }
    fn literal_distribution(
        &self,
        history: &[String],
    ) -> Result<Vec<(LosslessToken, BigRational)>> {
        let inherited = self.distribution(history)?;
        let unknown = inherited
            .get(UNK)
            .ok_or_else(|| failure("INVALID_MODEL", "missing unknown-token probability"))?;
        let byte_probability = unknown / BigRational::from_integer(BigInt::from(256));
        let mut distribution = Vec::new();
        for (symbol, probability) in inherited {
            if symbol == UNK {
                continue;
            }
            let token = if symbol == EOS {
                LosslessToken::End
            } else {
                LosslessToken::KnownChar { symbol }
            };
            distribution.push((token, probability));
        }
        for value in 0..=255 {
            distribution.push((
                LosslessToken::LiteralByte { value },
                byte_probability.clone(),
            ));
        }
        Ok(distribution)
    }
    /// The exact inherited UNK mass is divided among the 256 literal-byte
    /// tokens. Known-character probabilities stay equal; unknown byte histories
    /// condition as UNK steps. This is an explicit successor implementation
    /// choice, not asserted equivalence to old character-level unknown scoring.
    pub fn next_lossless_distribution(
        &self,
        raw_prefix: &[u8],
    ) -> Result<Vec<(LosslessToken, BigRational)>> {
        self.validate()?;
        let projection = self.project_bytes(raw_prefix)?;
        let history = projection
            .tokens
            .iter()
            .map(|token| match token {
                LosslessToken::KnownChar { symbol } => symbol.clone(),
                _ => UNK.into(),
            })
            .collect::<Vec<_>>();
        self.literal_distribution(&history)
    }
    pub fn lossless_sequence_probability(&self, bytes: &[u8]) -> Result<BigRational> {
        self.lossless_sequence_probability_until(bytes, model_deadline())
    }
    pub fn lossless_sequence_probability_until(
        &self,
        bytes: &[u8],
        deadline: Instant,
    ) -> Result<BigRational> {
        self.validate_until(deadline)?;
        model_bytes(bytes.len())?;
        let projection = self.project_bytes_until(bytes, deadline)?;
        let mut probability = BigRational::one();
        let mut history = Vec::new();
        for token in projection
            .tokens
            .into_iter()
            .chain(std::iter::once(LosslessToken::End))
        {
            poll_model(deadline)?;
            let next = match &token {
                LosslessToken::KnownChar { symbol } => {
                    self.conditional_probability(&history, symbol, deadline)?
                }
                LosslessToken::LiteralByte { .. } => {
                    self.conditional_probability(&history, UNK, deadline)?
                        / BigRational::from_integer(BigInt::from(256))
                }
                LosslessToken::End => self.conditional_probability(&history, EOS, deadline)?,
            };
            probability = probability_product(probability, next, deadline)?;
            history.push(match token {
                LosslessToken::KnownChar { symbol } => symbol,
                _ => UNK.into(),
            });
            if history.len() > self.order - 1 {
                history.remove(0);
            }
        }
        Ok(probability)
    }
    pub fn train(documents: &[String], order: usize, smoothing: u64) -> Result<Self> {
        Self::train_until(documents, order, smoothing, model_deadline())
    }
    pub fn train_until(
        documents: &[String],
        order: usize,
        smoothing: u64,
        deadline: Instant,
    ) -> Result<Self> {
        poll_model(deadline)?;
        if !(1..=16).contains(&order) || smoothing == 0 {
            return Err(failure(
                "MODEL_CONFIGURATION",
                "order must be 1..16 and smoothing positive",
            ));
        }
        let mut model = Self {
            order,
            smoothing,
            ..Self::default()
        };
        model.learn_until(documents, deadline)?;
        Ok(model)
    }
    pub fn learn(&mut self, documents: &[String]) -> Result<()> {
        self.learn_until(documents, model_deadline())
    }
    pub fn learn_until(&mut self, documents: &[String], deadline: Instant) -> Result<()> {
        self.validate_until(deadline)?;
        let mut total = 0usize;
        for text in documents {
            poll_model(deadline)?;
            total = total
                .checked_add(text.len())
                .ok_or_else(|| failure("MODEL_RESOURCE_LIMIT", "training byte count overflow"))?;
            model_bytes(total)?;
        }
        let mut candidate = self.clone();
        candidate.learn_in_place(documents, deadline)?;
        poll_model(deadline)?;
        *self = candidate;
        Ok(())
    }
    fn learn_in_place(&mut self, documents: &[String], deadline: Instant) -> Result<()> {
        for text in documents {
            poll_model(deadline)?;
            model_bytes(text.len())?;
            let normalized: String = text
                .nfkc()
                .collect::<String>()
                .replace("\r\n", "\n")
                .replace('\r', "\n");
            poll_model(deadline)?;
            model_bytes(normalized.len())?;
            let tokens: Vec<String> = normalized.chars().map(|ch| ch.to_string()).collect();
            self.vocabulary.extend(tokens.iter().cloned());
            let mut history = vec![BOS.to_string(); self.order - 1];
            for token in tokens.into_iter().chain(std::iter::once(EOS.into())) {
                poll_model(deadline)?;
                for width in 0..self.order {
                    let context = &history[history.len().saturating_sub(width)..];
                    let key = serde_json::to_string(context)?;
                    let count = self
                        .counts
                        .entry(key)
                        .or_default()
                        .entry(token.clone())
                        .or_default();
                    *count = count.checked_add(1).ok_or_else(|| {
                        failure("MODEL_RESOURCE_LIMIT", "transition count overflow")
                    })?;
                }
                if self.counts.len() > MAX_MODEL_CONTEXTS
                    || self.vocabulary.len() > MAX_MODEL_VOCABULARY
                {
                    return Err(failure(
                        "MODEL_RESOURCE_LIMIT",
                        "training context/vocabulary admission limit exceeded",
                    ));
                }
                history.push(token);
                if history.len() > self.order - 1 {
                    history.remove(0);
                }
            }
            self.document_count = self
                .document_count
                .checked_add(1)
                .ok_or_else(|| failure("MODEL_RESOURCE_LIMIT", "document count overflow"))?;
        }
        self.validate_until(deadline)
    }
    pub fn validate(&self) -> Result<()> {
        self.validate_until(model_deadline())
    }
    fn validate_until(&self, deadline: Instant) -> Result<()> {
        poll_model(deadline)?;
        if self.counts.len() > MAX_MODEL_CONTEXTS || self.vocabulary.len() > MAX_MODEL_VOCABULARY {
            return Err(failure(
                "MODEL_RESOURCE_LIMIT",
                "model context/vocabulary admission limit exceeded",
            ));
        }
        if self.version != "inherited-char-ngram-v1"
            || !(1..=16).contains(&self.order)
            || self.smoothing == 0
            || !self.vocabulary.contains(EOS)
            || !self.vocabulary.contains(UNK)
            || self.vocabulary.contains(BOS)
            || self
                .vocabulary
                .iter()
                .any(|token| token != EOS && token != UNK && token.chars().count() != 1)
        {
            return Err(failure(
                "INVALID_MODEL",
                "invalid model configuration or vocabulary",
            ));
        }
        let mut transitions = 0usize;
        for (key, counts) in &self.counts {
            poll_model(deadline)?;
            transitions = transitions
                .checked_add(counts.len())
                .ok_or_else(|| failure("MODEL_RESOURCE_LIMIT", "transition count overflow"))?;
            if transitions > MAX_MODEL_TRANSITIONS {
                return Err(failure(
                    "MODEL_RESOURCE_LIMIT",
                    "model transition admission limit exceeded",
                ));
            }
            if key.len() > 512 || counts.len() > MAX_MODEL_VOCABULARY {
                return Err(failure(
                    "MODEL_RESOURCE_LIMIT",
                    "model context/row record exceeds its admission envelope",
                ));
            }
            let context: Vec<String> = serde_json::from_str(key)?;
            if context.len() >= self.order
                || context
                    .iter()
                    .any(|v| v != BOS && !self.vocabulary.contains(v))
                || counts
                    .iter()
                    .any(|(token, count)| *count == 0 || !self.vocabulary.contains(token))
            {
                return Err(failure("INVALID_MODEL", "invalid model transition"));
            }
        }
        Ok(())
    }
    pub fn revision(&self) -> Result<String> {
        identity(self)
    }
    fn encode(&self, text: &str) -> Vec<String> {
        text.nfkc()
            .collect::<String>()
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .chars()
            .map(|ch| {
                let token = ch.to_string();
                if self.vocabulary.contains(&token) {
                    token
                } else {
                    UNK.into()
                }
            })
            .collect()
    }
    fn context(&self, history: &[String]) -> Result<Vec<String>> {
        let width = self.order - 1;
        let tail = &history[history.len().saturating_sub(width)..];
        let mut padded = vec![BOS.into(); width.saturating_sub(tail.len())];
        padded.extend_from_slice(tail);
        for length in (0..=width).rev() {
            let context = padded[padded.len().saturating_sub(length)..].to_vec();
            if self.counts.contains_key(&serde_json::to_string(&context)?) {
                return Ok(context);
            }
        }
        Ok(Vec::new())
    }
    fn conditional_probability(
        &self,
        history: &[String],
        token: &str,
        deadline: Instant,
    ) -> Result<BigRational> {
        poll_model(deadline)?;
        if !self.vocabulary.contains(token) {
            return Err(failure(
                "EMPTY_TOKEN_SUPPORT",
                "requested token is outside the finite vocabulary",
            ));
        }
        let context = self.context(history)?;
        let counts = self.counts.get(&serde_json::to_string(&context)?);
        let mut total = BigInt::zero();
        if let Some(rows) = counts {
            for value in rows.values() {
                poll_model(deadline)?;
                total += BigInt::from(*value);
            }
        }
        let denominator =
            total + BigInt::from(self.smoothing) * BigInt::from(self.vocabulary.len());
        let count = counts
            .and_then(|rows| rows.get(token))
            .copied()
            .unwrap_or(0);
        Ok(BigRational::new(
            BigInt::from(count) + BigInt::from(self.smoothing),
            denominator,
        ))
    }
    fn distribution(&self, history: &[String]) -> Result<BTreeMap<String, BigRational>> {
        self.distribution_until(history, model_deadline())
    }
    fn distribution_until(
        &self,
        history: &[String],
        deadline: Instant,
    ) -> Result<BTreeMap<String, BigRational>> {
        poll_model(deadline)?;
        let context = self.context(history)?;
        let counts = self.counts.get(&serde_json::to_string(&context)?);
        let mut total = BigInt::zero();
        if let Some(rows) = counts {
            for value in rows.values() {
                poll_model(deadline)?;
                total += BigInt::from(*value);
            }
        }
        let denominator =
            total + BigInt::from(self.smoothing) * BigInt::from(self.vocabulary.len());
        let mut distribution = BTreeMap::new();
        for token in &self.vocabulary {
            poll_model(deadline)?;
            let count = counts
                .and_then(|rows| rows.get(token))
                .copied()
                .unwrap_or(0);
            distribution.insert(
                token.clone(),
                BigRational::new(
                    BigInt::from(count) + BigInt::from(self.smoothing),
                    denominator.clone(),
                ),
            );
        }
        Ok(distribution)
    }
    pub fn next_distribution(&self, prefix: &str) -> Result<BTreeMap<String, BigRational>> {
        let deadline = model_deadline();
        self.validate_until(deadline)?;
        model_bytes(prefix.len())?;
        self.distribution_until(&self.encode(prefix), deadline)
    }
    pub fn sequence_probability(&self, text: &str) -> Result<BigRational> {
        self.sequence_probability_until(text, model_deadline())
    }
    pub fn sequence_probability_until(&self, text: &str, deadline: Instant) -> Result<BigRational> {
        self.validate_until(deadline)?;
        model_bytes(text.len())?;
        let mut probability = BigRational::one();
        let mut history = Vec::new();
        let tokens = self.encode(text);
        if tokens.len() > MAX_MODEL_TOKENS {
            return Err(failure(
                "MODEL_RESOURCE_LIMIT",
                "normalized sequence token admission limit exceeded",
            ));
        }
        for token in tokens.into_iter().chain(std::iter::once(EOS.into())) {
            poll_model(deadline)?;
            let next = self.conditional_probability(&history, &token, deadline)?;
            probability = probability_product(probability, next, deadline)?;
            history.push(token);
            if history.len() > self.order - 1 {
                history.remove(0);
            }
        }
        Ok(probability)
    }
    /// Deterministic greedy decoding. Exhaustion is an error, not synthetic EOS.
    pub fn decode(&self, prefix: &str, max_tokens: usize) -> Result<String> {
        let deadline = model_deadline();
        self.validate_until(deadline)?;
        model_bytes(prefix.len())?;
        if max_tokens == 0 || max_tokens > MAX_MODEL_TOKENS {
            return Err(failure(
                "MODEL_RESOURCE_LIMIT",
                "invalid decoder token budget",
            ));
        }
        let mut history = self.encode(prefix);
        let mut text = String::new();
        for _ in 0..max_tokens {
            poll_model(deadline)?;
            let distribution = self.distribution_until(&history, deadline)?;
            let mut best: Option<(&String, &BigRational)> = None;
            for (token, probability) in &distribution {
                if best.is_none_or(|(_, current)| probability > current) {
                    best = Some((token, probability));
                }
            }
            let token = best
                .ok_or_else(|| failure("EMPTY_TOKEN_SUPPORT", "no admissible next token"))?
                .0;
            if token == EOS {
                return Ok(text);
            }
            if token == UNK {
                return Err(failure("UNKNOWN_TOKEN", "decoder selected unknown symbol"));
            }
            text.push_str(token);
            history.push(token.clone());
            if history.len() > self.order - 1 {
                history.remove(0);
            }
        }
        Err(failure(
            "DECODER_BUDGET",
            "decoder did not reach EOS within the explicit token budget",
        ))
    }
    /// Select only among Core-authorized semantically equivalent full surfaces.
    /// This bounded sequence objective is deliberately distinct from unrestricted
    /// conversation generation; callers must verify all alternatives beforehand.
    pub fn realize_supported(
        &self,
        alternatives: &[String],
        max_tokens: usize,
    ) -> Result<ModelEmission> {
        self.realize_supported_until(alternatives, max_tokens, model_deadline())
    }
    pub fn realize_supported_until(
        &self,
        alternatives: &[String],
        max_tokens: usize,
        deadline: Instant,
    ) -> Result<ModelEmission> {
        poll_model(deadline)?;
        self.validate_until(deadline)?;
        let max_tokens = max_tokens.min(MAX_MODEL_TOKENS);
        if max_tokens == 0 || alternatives.len() > 64 {
            return Err(failure(
                "MODEL_RESOURCE_LIMIT",
                "decoder token/candidate admission limit exceeded",
            ));
        }
        let mut total = 0usize;
        for text in alternatives {
            total = total
                .checked_add(text.len())
                .ok_or_else(|| failure("MODEL_RESOURCE_LIMIT", "candidate byte count overflow"))?;
            model_bytes(total)?;
        }
        if alternatives.is_empty() {
            return Err(failure(
                "EMPTY_TOKEN_SUPPORT",
                "no Core-authorized surface sequence",
            ));
        }
        let mut best: Option<(&String, BigRational)> = None;
        for text in alternatives {
            poll_model(deadline)?;
            if self
                .project_bytes_until(text.as_bytes(), deadline)?
                .tokens
                .len()
                > max_tokens
            {
                continue;
            }
            let probability =
                self.lossless_sequence_probability_until(text.as_bytes(), deadline)?;
            if best.as_ref().is_none_or(|(old, score)| {
                probability > *score || (probability == *score && text < *old)
            }) {
                best = Some((text, probability));
            }
        }
        let (text, probability) = best.ok_or_else(|| {
            failure(
                "DECODER_BUDGET",
                "no authorized surface satisfies the token budget",
            )
        })?;
        let mut history = Vec::new();
        let mut context_hashes = Vec::new();
        let lossless_projection = self.project_bytes_until(text.as_bytes(), deadline)?;
        if lossless_projection.recover_bytes()? != text.as_bytes() {
            return Err(failure(
                "TOKEN_PROJECTION_INTEGRITY",
                "emission bytes changed during tokenization",
            ));
        }
        for literal in &lossless_projection.tokens {
            poll_model(deadline)?;
            let token = match literal {
                LosslessToken::KnownChar { symbol } => symbol.clone(),
                _ => UNK.into(),
            };
            context_hashes.push(identity(&self.context(&history)?)?);
            history.push(token);
            if history.len() > self.order - 1 {
                history.remove(0);
            }
        }
        poll_model(deadline)?;
        let model_revision = identity(&(self.revision()?, LOSSLESS_TOKEN_ADAPTER))?;
        poll_model(deadline)?;
        Ok(ModelEmission {
            text: text.clone(),
            model_revision,
            sequence_probability: rational_text(&probability),
            token_count: lossless_projection.tokens.len(),
            objective:
                "maximum exact sequence probability over Core-authorized equivalent surfaces".into(),
            decoder_version: "bounded-finite-surface-literal-exact-v1".into(),
            context_hashes,
            lossless_projection,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_arithmetic_and_precedence() {
        for (input, expected) in [
            ("0.1+0.2", "3/10"),
            ("1/3+1/6", "1/2"),
            ("２＋３×４", "14"),
            ("-2^2", "-4"),
            ("2^3^2", "512"),
            ("2^-3", "1/8"),
            ("1.25e2/5", "25"),
        ] {
            assert_eq!(calculate(input).unwrap().exact_value, expected, "{input}");
        }
    }
    #[test]
    fn exact_addition_and_subtraction_preserve_or_convert_units() {
        let mixed = calculate("1 km + 500 m").unwrap();
        assert_eq!(mixed.expression, "1 km + 500 m");
        assert_eq!(mixed.exact_value, "1500 m");
        assert_eq!(
            mixed.steps,
            ["1 km = 1000 m", "500 m = 500 m", "1000 m + 500 m = 1500 m"]
        );

        let decimal = calculate("0.1 km - 25 m").unwrap();
        assert_eq!(decimal.exact_value, "75 m");
        assert_eq!(
            decimal.steps,
            ["1/10 km = 100 m", "25 m = 25 m", "100 m - 25 m = 75 m"]
        );

        let same_unit = calculate("1.25 kg + 0.5 kg").unwrap();
        assert_eq!(same_unit.exact_value, "7/4 kg");
        assert_eq!(same_unit.steps, ["5/4 kg + 1/2 kg = 7/4 kg"]);
    }

    #[test]
    fn incompatible_mixed_units_are_a_typed_stop() {
        let error = calculate("1 km + 2 s").unwrap_err();
        assert!(
            matches!(error, minidora_domain::Error::Contract { code, .. } if code == "UNIT_MISMATCH")
        );
    }

    #[test]
    fn operand_correction_builds_a_new_exact_binary_calculation() {
        let prior = calculate("1 km + 500 m").unwrap();
        let corrected = correct_binary_operand(&prior, 1, "600 m").unwrap();
        assert_eq!(corrected.expression, "1 km + 600 m");
        assert_eq!(corrected.exact_value, "1600 m");
        assert_eq!(prior.exact_value, "1500 m");

        let omitted_unit = correct_binary_operand(&prior, 1, "700").unwrap();
        assert_eq!(omitted_unit.expression, "1 km + 700 m");
        assert_eq!(omitted_unit.exact_value, "1700 m");

        let unitless = calculate("2+3").unwrap();
        let changed = correct_binary_operand(&unitless, 0, "4").unwrap();
        assert_eq!(changed.expression, "4 + 3");
        assert_eq!(changed.exact_value, "7");

        assert!(correct_binary_operand(&unitless, 1, "5 km").is_err());
        assert_eq!(
            correct_binary_operand(&calculate("2 * 3").unwrap(), 1, "4")
                .unwrap()
                .exact_value,
            "8"
        );
        assert_eq!(
            correct_binary_operand(&calculate("12 / 3").unwrap(), 1, "4")
                .unwrap()
                .exact_value,
            "3"
        );
        assert_eq!(
            correct_binary_operand(&calculate("10 % 3").unwrap(), 1, "4")
                .unwrap()
                .exact_value,
            "2"
        );
        assert_eq!(
            correct_binary_operand(&calculate("2 ** 3").unwrap(), 1, "4")
                .unwrap()
                .exact_value,
            "16"
        );
        assert_eq!(
            correct_binary_operand(&calculate("2 ^ 3").unwrap(), 1, "4")
                .unwrap()
                .exact_value,
            "16"
        );
        let unit_product = correct_binary_operand(&calculate("2 * 3").unwrap(), 1, "4 m");
        assert!(matches!(
            unit_product,
            Err(minidora_domain::Error::Contract { code, .. })
                if code == "UNIT_OPERATION_UNSUPPORTED"
        ));
        assert!(correct_binary_operand(&calculate("2+3*4").unwrap(), 1, "4").is_err());
        let inconsistent = Calculation {
            expression: "2+3".into(),
            exact_value: "999".into(),
            steps: Vec::new(),
        };
        assert!(correct_binary_operand(&inconsistent, 1, "4").is_err());
    }

    #[test]
    fn parser_rejects_partial_or_unbounded_execution() {
        for input in [
            "2+3;rm", "1/0", "2^99999", "sqrt(4)", "0^0", "1+", "1e99999",
        ] {
            assert!(calculate(input).is_err(), "{input}");
        }
        assert!(calculate(&format!("{}1{}", "(".repeat(80), ")".repeat(80))).is_err());
    }
    #[test]
    fn inherited_ngram_exact_vector() {
        let mut documents = vec!["猫。".to_string(); 10];
        documents.push("犬。".into());
        let model = SequenceModel::train(&documents, 2, 1).unwrap();
        let distribution = model.next_distribution("").unwrap();
        assert_eq!(
            distribution.values().cloned().sum::<BigRational>(),
            BigRational::one()
        );
        assert_eq!(rational_text(&distribution["猫"]), "11/16");
        assert_eq!(
            rational_text(&model.sequence_probability("猫。").unwrap()),
            "121/320"
        );
        assert_eq!(
            rational_text(&model.sequence_probability("犬。").unwrap()),
            "1/32"
        );
        let restored: SequenceModel =
            serde_json::from_slice(&serde_json::to_vec(&model).unwrap()).unwrap();
        assert_eq!(restored.revision().unwrap(), model.revision().unwrap());
        assert_eq!(
            restored
                .next_distribution("未知")
                .unwrap()
                .values()
                .cloned()
                .sum::<BigRational>(),
            BigRational::one()
        );
    }
    #[test]
    fn finite_supported_decoding_preserves_admitted_content() {
        let model = SequenceModel::train(&["計算結果は5です。".into()], 3, 1).unwrap();
        let emission = model
            .realize_supported(&["計算結果は5です。".into()], 100)
            .unwrap();
        assert_eq!(emission.text, "計算結果は5です。");
        assert!(!emission.context_hashes.is_empty());
        assert!(model.realize_supported(&[], 100).is_err());
        assert!(model.realize_supported(&["長い文字列".into()], 1).is_err());
    }
}

#[cfg(test)]
mod lossless_tests {
    use super::*;
    #[test]
    fn successor_projection_preserves_unknown_unicode_and_invalid_utf8() {
        let model = SequenceModel::train(&["A猫。".into()], 2, 1).unwrap();
        for bytes in ["Ａ🙂猫\r\n".as_bytes(), &[0xff, 0, 0xfe, 0xe3, 0x81][..]] {
            let projection = model.project_bytes(bytes).unwrap();
            assert_eq!(projection.recover_bytes().unwrap(), bytes);
            assert!(
                projection
                    .tokens
                    .iter()
                    .any(|token| matches!(token, LosslessToken::LiteralByte { .. }))
            );
        }
        let mut projection = model.project_bytes("🙂".as_bytes()).unwrap();
        projection.raw_sha256 = "modified".into();
        assert!(projection.recover_bytes().is_err());
    }
    #[test]
    fn successor_distribution_is_exact_and_comparison_boundary_is_explicit() {
        let model = SequenceModel::train(&["A猫。".into()], 2, 1).unwrap();
        let distribution = model.next_lossless_distribution(&[]).unwrap();
        assert_eq!(
            distribution
                .iter()
                .map(|(_, probability)| probability.clone())
                .sum::<BigRational>(),
            BigRational::one()
        );
        let inherited = model.next_distribution("").unwrap();
        let literal_a = distribution
            .iter()
            .find(
                |(token, _)| matches!(token, LosslessToken::KnownChar { symbol } if symbol == "A"),
            )
            .unwrap();
        assert_eq!(literal_a.1, inherited["A"]);
        assert_eq!(
            model.sequence_probability("A猫。").unwrap(),
            model
                .lossless_sequence_probability("A猫。".as_bytes())
                .unwrap()
        );
        assert_ne!(
            model.sequence_probability("Ａ").unwrap(),
            model
                .lossless_sequence_probability("Ａ".as_bytes())
                .unwrap()
        );
        let emission = model.realize_supported(&["Ａ🙂".into()], 20).unwrap();
        assert_eq!(
            emission.lossless_projection.recover_bytes().unwrap(),
            "Ａ🙂".as_bytes()
        );
        assert_eq!(
            emission.decoder_version,
            "bounded-finite-surface-literal-exact-v1"
        );
    }
}

#[cfg(test)]
mod model_resource_tests {
    use super::*;
    fn code(error: minidora_domain::Error) -> String {
        match error {
            minidora_domain::Error::Contract { code, .. } => code,
            other => panic!("unexpected error {other}"),
        }
    }
    #[test]
    fn deadline_is_checked_inside_training_and_scoring() {
        let documents = vec!["abcdefghijklmno".repeat(900)];
        let started = Instant::now();
        let error =
            SequenceModel::train_until(&documents, 3, 1, started + Duration::from_micros(1))
                .unwrap_err();
        assert_eq!(code(error), "MODEL_DEADLINE");
        assert!(started.elapsed() < Duration::from_secs(2));
        let model = SequenceModel::default();
        let started = Instant::now();
        let error = model
            .realize_supported_until(
                &documents,
                MAX_MODEL_TOKENS,
                started + Duration::from_micros(1),
            )
            .unwrap_err();
        assert_eq!(code(error), "MODEL_DEADLINE");
        assert!(started.elapsed() < Duration::from_secs(2));
    }
    #[test]
    fn long_exact_products_stop_before_unbounded_big_integer_growth() {
        let model = SequenceModel::default();
        let text = "untrained-symbols".repeat(700);
        let started = Instant::now();
        let error = model
            .lossless_sequence_probability_until(text.as_bytes(), started + Duration::from_secs(2))
            .unwrap_err();
        assert_eq!(code(error), "MODEL_RESOURCE_LIMIT");
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(
            code(model.sequence_probability(&text).unwrap_err()),
            "MODEL_RESOURCE_LIMIT"
        );
    }
    #[test]
    fn shared_byte_admission_and_failed_learning_preserve_model_state() {
        let mut model = SequenceModel::default();
        let original = model.revision().unwrap();
        let oversized = "x".repeat(MAX_MODEL_BYTES + 1);
        assert_eq!(
            code(model.project_bytes(oversized.as_bytes()).unwrap_err()),
            "MODEL_RESOURCE_LIMIT"
        );
        assert_eq!(
            code(model.sequence_probability(&oversized).unwrap_err()),
            "MODEL_RESOURCE_LIMIT"
        );
        assert_eq!(
            code(
                model
                    .lossless_sequence_probability(oversized.as_bytes())
                    .unwrap_err()
            ),
            "MODEL_RESOURCE_LIMIT"
        );
        assert_eq!(
            code(model.learn(&[oversized]).unwrap_err()),
            "MODEL_RESOURCE_LIMIT"
        );
        assert_eq!(model.revision().unwrap(), original);
    }
}

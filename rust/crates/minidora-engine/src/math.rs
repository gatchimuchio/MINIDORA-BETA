//! Exact bounded mathematical operations with inspectable certificates.
//! Formal results apply to the supplied coefficients/data; they do not validate
//! real-world premises or infer that observations share a physical population.
use crate::language::{calculate, rational_text};
use minidora_domain::{Result, failure};
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, Zero};
use serde::{Deserialize, Serialize};
use std::str::FromStr;

const MAX_VARIABLES: usize = 8;
const MAX_EQUATIONS: usize = 16;
const MAX_OBSERVATIONS: usize = 4096;
const MAX_SCALAR_BYTES: usize = 1024;
const MAX_RATIONAL_BITS: u64 = 4096;
const MAX_OPERATIONS: usize = 100_000;

type Matrix = Vec<Vec<BigRational>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LinearStatus {
    Unique,
    Underdetermined,
    Inconsistent,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RowOperation {
    Swap {
        first: usize,
        second: usize,
    },
    Scale {
        row: usize,
        factor: String,
    },
    Add {
        target: usize,
        source: usize,
        factor: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContradictionCertificate {
    /// Linear combination of the original equations that gives 0 = rhs != 0.
    pub equation_multipliers: Vec<String>,
    pub rhs: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinearSolution {
    pub status: LinearStatus,
    pub variable_count: usize,
    pub rank: usize,
    pub original_coefficients: Vec<Vec<String>>,
    pub original_rhs: Vec<String>,
    pub pivot_columns: Vec<usize>,
    pub free_columns: Vec<usize>,
    pub solution: Option<Vec<String>>,
    pub particular: Option<Vec<String>>,
    /// Every solution is particular + sum(t_i * nullspace_basis[i]).
    pub nullspace_basis: Vec<Vec<String>>,
    pub rref: Vec<Vec<String>>,
    pub row_transform: Vec<Vec<String>>,
    pub operations: Vec<RowOperation>,
    pub contradiction: Option<ContradictionCertificate>,
    pub verification: String,
}

#[derive(Default)]
struct Budget(usize);
impl Budget {
    fn check(&mut self, value: BigRational) -> Result<BigRational> {
        self.0 += 1;
        if self.0 > MAX_OPERATIONS {
            return Err(failure(
                "MATH_OPERATION_LIMIT",
                "mathematical operation budget exceeded",
            ));
        }
        if value.numer().bits() > MAX_RATIONAL_BITS || value.denom().bits() > MAX_RATIONAL_BITS {
            return Err(failure(
                "MATH_NUMBER_LIMIT",
                "exact value exceeds the 4096-bit admission envelope",
            ));
        }
        Ok(value)
    }
}

fn scalar(text: &str) -> Result<BigRational> {
    if text.is_empty() || text.len() > MAX_SCALAR_BYTES {
        return Err(failure(
            "MATH_INPUT_LIMIT",
            "scalar is empty or exceeds 1024 bytes",
        ));
    }
    // Reuse the native exact constant-expression parser; no eval or names.
    let exact = calculate(text)?.exact_value;
    let value = BigRational::from_str(&exact)
        .map_err(|_| failure("INVALID_EXACT_NUMBER", "invalid exact calculation result"))?;
    Budget::default().check(value)
}

pub(crate) fn json_number_equal(left: &str, right: &str) -> Result<bool> {
    Ok(scalar(left)? == scalar(right)?)
}

fn certificate_number(text: &str) -> Result<BigRational> {
    if text.is_empty() || text.len() > 8192 {
        return Err(failure(
            "INVALID_LINEAR_CERTIFICATE",
            "certificate scalar size limit",
        ));
    }
    let value = BigRational::from_str(text)
        .map_err(|_| failure("INVALID_LINEAR_CERTIFICATE", "invalid certificate rational"))?;
    Budget::default().check(value)
}
fn text_matrix(matrix: &Matrix) -> Vec<Vec<String>> {
    matrix
        .iter()
        .map(|row| row.iter().map(rational_text).collect())
        .collect()
}
fn parse_matrix(matrix: &[Vec<String>]) -> Result<Matrix> {
    matrix
        .iter()
        .map(|row| row.iter().map(|v| scalar(v)).collect())
        .collect()
}
fn eye(size: usize) -> Matrix {
    (0..size)
        .map(|row| {
            (0..size)
                .map(|column| BigRational::from_integer(BigInt::from(usize::from(row == column))))
                .collect()
        })
        .collect()
}

fn dimensions(coefficients: &[Vec<String>], rhs: &[String]) -> Result<(usize, usize)> {
    let rows = coefficients.len();
    let columns = coefficients.first().map_or(0, Vec::len);
    if !(1..=MAX_EQUATIONS).contains(&rows)
        || !(1..=MAX_VARIABLES).contains(&columns)
        || rhs.len() != rows
        || coefficients.iter().any(|row| row.len() != columns)
    {
        return Err(failure(
            "LINEAR_DIMENSIONS",
            "expected 1..16 rectangular equations in 1..8 variables and matching rhs",
        ));
    }
    Ok((rows, columns))
}

/// Solve exact linear equations. Inputs may contain bounded constant rational
/// expressions. Underdetermined systems retain all free parameters; inconsistent
/// systems return a checked linear-combination contradiction certificate.
pub fn solve_linear(coefficients: Vec<Vec<String>>, rhs: Vec<String>) -> Result<LinearSolution> {
    let (rows, columns) = dimensions(&coefficients, &rhs)?;
    let mut augmented = parse_matrix(&coefficients)?;
    for (row, value) in augmented.iter_mut().zip(&rhs) {
        row.push(scalar(value)?);
    }
    let mut transform = eye(rows);
    let mut operations = Vec::new();
    let mut pivot_columns = Vec::new();
    let mut budget = Budget::default();
    let mut rank = 0;
    for column in 0..columns {
        let Some(pivot) = (rank..rows).find(|&row| !augmented[row][column].is_zero()) else {
            continue;
        };
        if pivot != rank {
            augmented.swap(pivot, rank);
            transform.swap(pivot, rank);
            operations.push(RowOperation::Swap {
                first: pivot,
                second: rank,
            });
        }
        let reciprocal = budget.check(augmented[rank][column].clone().recip())?;
        for value in &mut augmented[rank] {
            *value = budget.check(value.clone() * &reciprocal)?;
        }
        for value in &mut transform[rank] {
            *value = budget.check(value.clone() * &reciprocal)?;
        }
        operations.push(RowOperation::Scale {
            row: rank,
            factor: rational_text(&reciprocal),
        });
        for target in 0..rows {
            if target == rank || augmented[target][column].is_zero() {
                continue;
            }
            let factor = -augmented[target][column].clone();
            let pivot_row = augmented[rank].clone();
            for (value, pivot_value) in augmented[target].iter_mut().zip(pivot_row) {
                *value = budget.check(value.clone() + &factor * pivot_value)?;
            }
            let transform_row = transform[rank].clone();
            for (value, pivot_value) in transform[target].iter_mut().zip(transform_row) {
                *value = budget.check(value.clone() + &factor * pivot_value)?;
            }
            operations.push(RowOperation::Add {
                target,
                source: rank,
                factor: rational_text(&factor),
            });
        }
        pivot_columns.push(column);
        rank += 1;
        if rank == rows {
            break;
        }
    }
    let contradiction_row = augmented
        .iter()
        .position(|row| row[..columns].iter().all(Zero::is_zero) && !row[columns].is_zero());
    let free_columns: Vec<usize> = (0..columns)
        .filter(|column| !pivot_columns.contains(column))
        .collect();
    let (status, solution, particular, nullspace_basis, contradiction) =
        if let Some(row) = contradiction_row {
            (
                LinearStatus::Inconsistent,
                None,
                None,
                Vec::new(),
                Some(ContradictionCertificate {
                    equation_multipliers: transform[row].iter().map(rational_text).collect(),
                    rhs: rational_text(&augmented[row][columns]),
                }),
            )
        } else {
            let mut particular = vec![BigRational::zero(); columns];
            for (row, &column) in pivot_columns.iter().enumerate() {
                particular[column] = augmented[row][columns].clone();
            }
            let mut basis = Vec::new();
            for &free_column in &free_columns {
                let mut vector = vec![BigRational::zero(); columns];
                vector[free_column] = BigRational::one();
                for (row, &column) in pivot_columns.iter().enumerate() {
                    vector[column] = -augmented[row][free_column].clone();
                }
                basis.push(vector.iter().map(rational_text).collect());
            }
            let particular: Vec<String> = particular.iter().map(rational_text).collect();
            let status = if free_columns.is_empty() {
                LinearStatus::Unique
            } else {
                LinearStatus::Underdetermined
            };
            let solution = if status == LinearStatus::Unique {
                Some(particular.clone())
            } else {
                None
            };
            (status, solution, Some(particular), basis, None)
        };
    let result = LinearSolution {
        status, variable_count: columns, rank, original_coefficients: coefficients, original_rhs: rhs,
        pivot_columns, free_columns, solution, particular, nullspace_basis,
        rref: text_matrix(&augmented), row_transform: text_matrix(&transform), operations, contradiction,
        verification: "exact invertible row-operation replay, transform*original, original-equation substitution and nullspace/contradiction certificate".into(),
    };
    verify_linear(&result)?;
    Ok(result)
}

fn apply_row_operation(
    matrix: &mut Matrix,
    operation: &RowOperation,
    budget: &mut Budget,
) -> Result<()> {
    let rows = matrix.len();
    match operation {
        RowOperation::Swap { first, second } => {
            if *first >= rows || *second >= rows {
                return Err(failure("INVALID_LINEAR_CERTIFICATE", "invalid swap row"));
            }
            matrix.swap(*first, *second);
        }
        RowOperation::Scale { row, factor } => {
            if *row >= rows {
                return Err(failure("INVALID_LINEAR_CERTIFICATE", "invalid scale row"));
            }
            let factor = certificate_number(factor)?;
            if factor.is_zero() {
                return Err(failure(
                    "INVALID_LINEAR_CERTIFICATE",
                    "singular row scaling",
                ));
            }
            for value in &mut matrix[*row] {
                *value = budget.check(value.clone() * &factor)?;
            }
        }
        RowOperation::Add {
            target,
            source,
            factor,
        } => {
            if *target >= rows || *source >= rows || target == source {
                return Err(failure(
                    "INVALID_LINEAR_CERTIFICATE",
                    "invalid additive row operation",
                ));
            }
            let factor = certificate_number(factor)?;
            let source = matrix[*source].clone();
            for (value, source) in matrix[*target].iter_mut().zip(source) {
                *value = budget.check(value.clone() + &factor * source)?;
            }
        }
    }
    Ok(())
}
fn require(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(failure("INVALID_LINEAR_CERTIFICATE", message))
    }
}

/// Independently replay invertible row operations and verify actual supplied
/// equations. A forged solution, missing source row or singular transform fails.
pub fn verify_linear(result: &LinearSolution) -> Result<()> {
    let (rows, columns) = dimensions(&result.original_coefficients, &result.original_rhs)?;
    require(
        result.variable_count == columns && result.rank == result.pivot_columns.len(),
        "incorrect dimensions/rank",
    )?;
    require(
        result.operations.len() <= MAX_OPERATIONS,
        "operation record limit exceeded",
    )?;
    let original = parse_matrix(&result.original_coefficients)?;
    let rhs: Vec<BigRational> = result
        .original_rhs
        .iter()
        .map(|x| scalar(x))
        .collect::<Result<_>>()?;
    let mut augmented = original.clone();
    for (row, value) in augmented.iter_mut().zip(&rhs) {
        row.push(value.clone());
    }
    let mut transform = eye(rows);
    let mut budget = Budget::default();
    for operation in &result.operations {
        apply_row_operation(&mut augmented, operation, &mut budget)?;
        apply_row_operation(&mut transform, operation, &mut budget)?;
    }
    require(
        text_matrix(&augmented) == result.rref && text_matrix(&transform) == result.row_transform,
        "row-operation replay mismatch",
    )?;
    // Matrix multiplication independently reconnects every transformed value to
    // all original rows, including redundant equations and contradiction rows.
    for row in 0..rows {
        for column in 0..=columns {
            let mut value = BigRational::zero();
            for k in 0..rows {
                let source = if column == columns {
                    &rhs[k]
                } else {
                    &original[k][column]
                };
                value = budget.check(value + &transform[row][k] * source)?;
            }
            require(
                value == augmented[row][column],
                "transform does not preserve originals",
            )?;
        }
    }
    let mut pivots = Vec::new();
    for row in &augmented {
        if let Some(column) = row[..columns].iter().position(|x| !x.is_zero()) {
            require(row[column].is_one(), "pivot not normalized")?;
            if let Some(prior) = pivots.last() {
                require(column > *prior, "pivot order not reduced")?;
            }
            pivots.push(column);
        }
    }
    require(pivots == result.pivot_columns, "incorrect pivot columns")?;
    for (row, &column) in pivots.iter().enumerate() {
        require(
            (0..rows).all(|other| other == row || augmented[other][column].is_zero()),
            "pivot column not reduced",
        )?;
    }
    let free_columns: Vec<usize> = (0..columns)
        .filter(|column| !pivots.contains(column))
        .collect();
    require(
        free_columns == result.free_columns,
        "incorrect free columns",
    )?;
    if let Some(contradiction) = &result.contradiction {
        require(
            result.status == LinearStatus::Inconsistent
                && result.solution.is_none()
                && result.particular.is_none()
                && result.nullspace_basis.is_empty(),
            "inconsistent status mismatch",
        )?;
        require(
            contradiction.equation_multipliers.len() == rows,
            "contradiction dimension mismatch",
        )?;
        let multipliers: Vec<BigRational> = contradiction
            .equation_multipliers
            .iter()
            .map(|x| certificate_number(x))
            .collect::<Result<_>>()?;
        let target = certificate_number(&contradiction.rhs)?;
        require(!target.is_zero(), "zero is not a contradiction")?;
        let mut lhs = vec![BigRational::zero(); columns];
        let mut derived_rhs = BigRational::zero();
        for ((row, source_rhs), factor) in original.iter().zip(&rhs).zip(&multipliers) {
            for (value, coefficient) in lhs.iter_mut().zip(row) {
                *value = budget.check(value.clone() + factor * coefficient)?;
            }
            derived_rhs = budget.check(derived_rhs + factor * source_rhs)?;
        }
        require(
            lhs.iter().all(Zero::is_zero) && derived_rhs == target,
            "invalid contradiction certificate",
        )?;
        return Ok(());
    }
    let expected_status = if free_columns.is_empty() {
        LinearStatus::Unique
    } else {
        LinearStatus::Underdetermined
    };
    require(
        result.status == expected_status,
        "incorrect solution status",
    )?;
    let particular = result
        .particular
        .as_ref()
        .ok_or_else(|| failure("INVALID_LINEAR_CERTIFICATE", "missing particular solution"))?;
    require(
        particular.len() == columns && result.nullspace_basis.len() == free_columns.len(),
        "solution-space dimensions mismatch",
    )?;
    let particular: Vec<BigRational> = particular
        .iter()
        .map(|x| certificate_number(x))
        .collect::<Result<_>>()?;
    for (row, expected) in original.iter().zip(&rhs) {
        let value = row
            .iter()
            .zip(&particular)
            .fold(BigRational::zero(), |total, (a, x)| total + a * x);
        require(
            &value == expected,
            "particular solution fails an original equation",
        )?;
    }
    if expected_status == LinearStatus::Unique {
        require(
            result.solution == result.particular,
            "unique solution mismatch",
        )?;
    } else {
        require(
            result.solution.is_none(),
            "free parameters were silently fixed",
        )?;
    }
    for (basis_index, vector) in result.nullspace_basis.iter().enumerate() {
        require(vector.len() == columns, "nullspace dimension mismatch")?;
        let vector: Vec<BigRational> = vector
            .iter()
            .map(|x| certificate_number(x))
            .collect::<Result<_>>()?;
        for (index, &free) in free_columns.iter().enumerate() {
            require(
                vector[free]
                    == if index == basis_index {
                        BigRational::one()
                    } else {
                        BigRational::zero()
                    },
                "nullspace free coordinates are not independent",
            )?;
        }
        for row in &original {
            let value = row
                .iter()
                .zip(&vector)
                .fold(BigRational::zero(), |total, (a, x)| total + a * x);
            require(
                value.is_zero(),
                "nullspace basis fails an original equation",
            )?;
        }
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Statistics {
    pub input_values: Vec<String>,
    /// One source unit per input value, or `None` when the input is unitless.
    pub input_units: Option<Vec<String>>,
    /// Exact values expressed in `unit`, in the original input order.
    pub normalized_values: Vec<String>,
    pub count: usize,
    pub unit: Option<String>,
    pub variance_unit: Option<String>,
    pub sum: String,
    pub mean: String,
    pub median: String,
    pub min: String,
    pub max: String,
    pub population_variance: String,
    /// None for one observation, because n-1 would be zero.
    pub sample_variance: Option<String>,
    pub sorted_values: Vec<String>,
    pub scope: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizedProbabilityWeights {
    pub input_weights: Vec<String>,
    pub total_weight: String,
    pub normalized_probabilities: Vec<String>,
    pub count: usize,
    pub probability_sum: String,
    pub scope: String,
}

/// Normalize explicit nonnegative weights into an exact finite distribution.
/// The values describe only the supplied weights, not empirical likelihoods.
fn normalize_weight_values(
    weights: &[String],
) -> Result<(Vec<BigRational>, BigRational, BigRational)> {
    if weights.is_empty() || weights.len() > MAX_OBSERVATIONS {
        return Err(failure(
            "PROBABILITY_INPUT_LIMIT",
            "expected 1..4096 explicit weights",
        ));
    }
    let values = weights
        .iter()
        .map(|weight| scalar(weight))
        .collect::<Result<Vec<_>>>()?;
    if values.iter().any(|value| value < &BigRational::zero()) {
        return Err(failure(
            "NEGATIVE_PROBABILITY_WEIGHT",
            "probability weights must be nonnegative",
        ));
    }
    let mut budget = Budget::default();
    let mut total = BigRational::zero();
    for value in &values {
        total = budget.check(total + value)?;
    }
    if total.is_zero() {
        return Err(failure(
            "ZERO_PROBABILITY_WEIGHT_TOTAL",
            "at least one supplied weight must be positive",
        ));
    }
    let normalized: Vec<BigRational> = values
        .iter()
        .map(|value| budget.check(value / &total))
        .collect::<Result<_>>()?;
    let mut probability_sum = BigRational::zero();
    for probability in &normalized {
        probability_sum = budget.check(probability_sum + probability)?;
    }
    if !probability_sum.is_one() {
        return Err(failure(
            "PROBABILITY_CERTIFICATE_FAILURE",
            "normalized probabilities do not sum exactly to one",
        ));
    }
    Ok((normalized, total, probability_sum))
}

pub fn normalize_probability_weights(weights: &[String]) -> Result<NormalizedProbabilityWeights> {
    let (normalized, total, probability_sum) = normalize_weight_values(weights)?;
    Ok(NormalizedProbabilityWeights {
        input_weights: weights.to_vec(),
        total_weight: rational_text(&total),
        normalized_probabilities: normalized.iter().map(rational_text).collect(),
        count: weights.len(),
        probability_sum: rational_text(&probability_sum),
        scope: "exact normalization of the explicitly supplied nonnegative weights; not an empirical or real-world probability estimate".into(),
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FiniteProbabilityStatistics {
    pub input_outcomes: Vec<String>,
    pub input_weights: Vec<String>,
    pub total_weight: String,
    pub normalized_probabilities: Vec<String>,
    pub probability_sum: String,
    pub count: usize,
    pub expected_value_contributions: Vec<String>,
    pub expected_value: String,
    pub variance_contributions: Vec<String>,
    pub variance: String,
    pub scope: String,
}

/// Compute exact moments for explicitly paired finite outcomes and weights.
/// This does not infer empirical or real-world probabilities.
pub fn finite_probability_statistics(
    outcomes: &[String],
    weights: &[String],
) -> Result<FiniteProbabilityStatistics> {
    if outcomes.is_empty()
        || outcomes.len() > MAX_OBSERVATIONS
        || weights.is_empty()
        || weights.len() > MAX_OBSERVATIONS
    {
        return Err(failure(
            "PROBABILITY_INPUT_LIMIT",
            "expected 1..4096 outcomes and weights",
        ));
    }
    if outcomes.len() != weights.len() {
        return Err(failure(
            "PROBABILITY_CARDINALITY_MISMATCH",
            "each outcome must have exactly one corresponding weight",
        ));
    }
    let parsed_outcomes = outcomes
        .iter()
        .map(|outcome| scalar(outcome))
        .collect::<Result<Vec<_>>>()?;
    let (probabilities, total, probability_sum) = normalize_weight_values(weights)?;
    let mut budget = Budget::default();
    let mut expected_value = BigRational::zero();
    let mut expected_value_contributions = Vec::with_capacity(outcomes.len());
    for (outcome, probability) in parsed_outcomes.iter().zip(&probabilities) {
        let weighted_value = budget.check(outcome * probability)?;
        expected_value = budget.check(expected_value + &weighted_value)?;
        expected_value_contributions.push(weighted_value);
    }
    let mut variance = BigRational::zero();
    let mut variance_contributions = Vec::with_capacity(outcomes.len());
    for (outcome, probability) in parsed_outcomes.iter().zip(&probabilities) {
        let deviation = budget.check(outcome - &expected_value)?;
        let squared_deviation = budget.check(&deviation * &deviation)?;
        let weighted_deviation = budget.check(probability * squared_deviation)?;
        variance = budget.check(variance + &weighted_deviation)?;
        variance_contributions.push(weighted_deviation);
    }
    Ok(FiniteProbabilityStatistics {
        input_outcomes: outcomes.to_vec(),
        input_weights: weights.to_vec(),
        total_weight: rational_text(&total),
        normalized_probabilities: probabilities.iter().map(rational_text).collect(),
        probability_sum: rational_text(&probability_sum),
        count: outcomes.len(),
        expected_value_contributions: expected_value_contributions.iter().map(rational_text).collect(),
        expected_value: rational_text(&expected_value),
        variance_contributions: variance_contributions.iter().map(rational_text).collect(),
        variance: rational_text(&variance),
        scope: "exact expectation and variance over the explicitly supplied finite outcomes and weights; not an empirical or real-world probability estimate".into(),
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventProbabilityResult {
    pub input_outcomes: Vec<String>,
    pub input_weights: Vec<String>,
    pub event_values: Vec<String>,
    pub total_weight: String,
    pub normalized_probabilities: Vec<String>,
    pub probability_sum: String,
    pub event_membership: Vec<bool>,
    pub event_probability_contributions: Vec<String>,
    pub event_probability: String,
    pub count: usize,
    pub scope: String,
}

/// Sum the exact mass of outcomes contained in the explicitly supplied event set.
/// Duplicate event values have set membership semantics and do not double-count.
pub fn finite_event_probability(
    outcomes: &[String],
    weights: &[String],
    event_values: &[String],
) -> Result<EventProbabilityResult> {
    if outcomes.is_empty()
        || outcomes.len() > MAX_OBSERVATIONS
        || weights.is_empty()
        || weights.len() > MAX_OBSERVATIONS
        || event_values.len() > MAX_OBSERVATIONS
    {
        return Err(failure(
            "PROBABILITY_INPUT_LIMIT",
            "expected 1..4096 outcomes and weights and at most 4096 event values",
        ));
    }
    if outcomes.len() != weights.len() {
        return Err(failure(
            "PROBABILITY_CARDINALITY_MISMATCH",
            "each outcome must have exactly one corresponding weight",
        ));
    }
    let parsed_outcomes = outcomes
        .iter()
        .map(|outcome| scalar(outcome))
        .collect::<Result<Vec<_>>>()?;
    let parsed_event_values = event_values
        .iter()
        .map(|value| scalar(value))
        .collect::<Result<Vec<_>>>()?;
    let event_set: std::collections::BTreeSet<BigRational> =
        parsed_event_values.into_iter().collect();
    let (probabilities, total, probability_sum) = normalize_weight_values(weights)?;
    let mut budget = Budget::default();
    let mut event_probability = BigRational::zero();
    let mut event_membership = Vec::with_capacity(outcomes.len());
    let mut event_probability_contributions = Vec::with_capacity(outcomes.len());
    for (outcome, probability) in parsed_outcomes.iter().zip(&probabilities) {
        let is_member = event_set.contains(outcome);
        let contribution = if is_member {
            probability.clone()
        } else {
            BigRational::zero()
        };
        event_probability = budget.check(event_probability + &contribution)?;
        event_membership.push(is_member);
        event_probability_contributions.push(contribution);
    }
    Ok(EventProbabilityResult {
        input_outcomes: outcomes.to_vec(),
        input_weights: weights.to_vec(),
        event_values: event_values.to_vec(),
        total_weight: rational_text(&total),
        normalized_probabilities: probabilities.iter().map(rational_text).collect(),
        probability_sum: rational_text(&probability_sum),
        event_membership,
        event_probability_contributions: event_probability_contributions
            .iter()
            .map(rational_text)
            .collect(),
        event_probability: rational_text(&event_probability),
        count: outcomes.len(),
        scope: "exact probability mass of the explicitly listed finite outcomes that match the supplied event values; duplicate event values do not double-count; not an empirical or real-world estimate".into(),
    })
}

pub(crate) fn exact_length_scale_to_meters(unit: &str) -> Option<BigRational> {
    let (numerator, denominator) = match unit {
        "km" => (1000_i64, 1_i64),
        "m" => (1, 1),
        "cm" => (1, 100),
        "mm" => (1, 1000),
        _ => return None,
    };
    Some(BigRational::new(
        BigInt::from(numerator),
        BigInt::from(denominator),
    ))
}

/// Descriptive statistics on supplied values. Every operation uses exact
/// rationals. Identical unit labels retain their original unit; mixed units
/// convert only within the explicitly defined SI length set. No assertion is
/// made that the supplied observations represent a wider population.
pub fn summarize(values: &[String], units: Option<&[String]>) -> Result<Statistics> {
    if values.is_empty() || values.len() > MAX_OBSERVATIONS {
        return Err(failure(
            "STATISTICS_INPUT_LIMIT",
            "expected 1..4096 observations",
        ));
    }
    let parsed_values: Vec<BigRational> = values
        .iter()
        .map(|value| scalar(value))
        .collect::<Result<_>>()?;
    let mut budget = Budget::default();
    let (unit, input_units, normalized_values) = if let Some(units) = units {
        if units.len() != values.len()
            || units.iter().any(|unit| {
                unit.is_empty() || unit.len() > 64 || unit.chars().any(char::is_control)
            })
        {
            return Err(failure(
                "STATISTICS_UNIT",
                "one valid explicit unit is required for every value",
            ));
        }
        let input_units = Some(units.to_vec());
        if units.iter().all(|unit| unit == &units[0]) {
            (Some(units[0].clone()), input_units, parsed_values)
        } else {
            let scales = units
                .iter()
                .map(|unit| exact_length_scale_to_meters(unit))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| {
                    failure(
                        "UNIT_MISMATCH",
                        "mixed units require one verified conversion family; supported mixed units are mm, cm, m, and km",
                    )
                })?;
            let normalized = parsed_values
                .iter()
                .zip(scales)
                .map(|(value, scale)| budget.check(value * scale))
                .collect::<Result<Vec<_>>>()?;
            (Some("m".into()), input_units, normalized)
        }
    } else {
        (None, None, parsed_values)
    };
    let normalized_value_texts: Vec<String> = normalized_values.iter().map(rational_text).collect();
    let mut ordered = normalized_values;
    let mut sum = BigRational::zero();
    for value in &ordered {
        sum = budget.check(sum + value)?;
    }
    let count = BigRational::from_integer(BigInt::from(ordered.len()));
    let mean = budget.check(&sum / &count)?;
    let mut squared_deviations = BigRational::zero();
    for value in &ordered {
        let delta = budget.check(value - &mean)?;
        squared_deviations = budget.check(squared_deviations + &delta * &delta)?;
    }
    let population_variance = budget.check(&squared_deviations / &count)?;
    let sample_variance = if values.len() > 1 {
        Some(rational_text(&budget.check(
            squared_deviations / BigRational::from_integer(BigInt::from(values.len() - 1)),
        )?))
    } else {
        None
    };
    ordered.sort();
    let middle = ordered.len() / 2;
    let median = if ordered.len().is_multiple_of(2) {
        budget.check(
            (&ordered[middle - 1] + &ordered[middle]) / BigRational::from_integer(BigInt::from(2)),
        )?
    } else {
        ordered[middle].clone()
    };
    Ok(Statistics {
        input_values: values.to_vec(), input_units, normalized_values: normalized_value_texts, count: values.len(), variance_unit: unit.as_ref().map(|unit| format!("({unit})^2")), unit,
        sum: rational_text(&sum), mean: rational_text(&mean), median: rational_text(&median),
        min: rational_text(&ordered[0]), max: rational_text(&ordered[ordered.len()-1]),
        population_variance: rational_text(&population_variance), sample_variance,
        sorted_values: ordered.iter().map(rational_text).collect(),
        scope: "descriptive exact statistics of the supplied values; no inference about population representativeness or causal effects".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).into()).collect()
    }
    fn matrix(rows: &[&[&str]]) -> Vec<Vec<String>> {
        rows.iter().map(|row| strings(row)).collect()
    }

    #[test]
    fn exact_linear_unique_and_fractional_substitution() {
        let result =
            solve_linear(matrix(&[&["2", "1"], &["1", "-1"]]), strings(&["7", "-1"])).unwrap();
        assert_eq!(result.status, LinearStatus::Unique);
        assert_eq!(result.solution, Some(strings(&["2", "3"])));
        verify_linear(&result).unwrap();
        let result = solve_linear(
            matrix(&[&["1/3", "1/2"], &["1", "-1"]]),
            strings(&["1", "0"]),
        )
        .unwrap();
        assert_eq!(result.solution, Some(strings(&["6/5", "6/5"])));
    }
    #[test]
    fn large_valid_certificates_do_not_reuse_the_smaller_input_text_limit() {
        let a = BigInt::from(10).pow(500) + BigInt::from(7);
        let b = BigInt::from(10).pow(500) + BigInt::from(9);
        let coefficients = vec![
            vec![a.to_string(), "1".into()],
            vec!["1".into(), b.to_string()],
        ];
        let result = solve_linear(coefficients, strings(&["1", "1"])).unwrap();
        let determinant = &a * &b - BigInt::one();
        let expected = vec![
            rational_text(&BigRational::new(&b - BigInt::one(), determinant.clone())),
            rational_text(&BigRational::new(&a - BigInt::one(), determinant)),
        ];
        assert!(expected[0].len() > MAX_SCALAR_BYTES);
        assert_eq!(result.solution, Some(expected));
        verify_linear(&result).unwrap();
    }
    #[test]
    fn free_parameters_and_inconsistent_certificates_are_distinct() {
        let free = solve_linear(
            matrix(&[&["1", "2", "3"], &["2", "4", "6"]]),
            strings(&["4", "8"]),
        )
        .unwrap();
        assert_eq!(free.status, LinearStatus::Underdetermined);
        assert!(free.solution.is_none());
        assert_eq!(free.free_columns, vec![1, 2]);
        assert_eq!(
            free.nullspace_basis,
            matrix(&[&["-2", "1", "0"], &["-3", "0", "1"]])
        );
        let inconsistent =
            solve_linear(matrix(&[&["1", "2"], &["2", "4"]]), strings(&["4", "9"])).unwrap();
        assert_eq!(inconsistent.status, LinearStatus::Inconsistent);
        assert!(inconsistent.contradiction.is_some());
        assert!(inconsistent.particular.is_none());
        verify_linear(&inconsistent).unwrap();
    }
    #[test]
    fn independent_known_solutions_and_row_order_perturbation() {
        // Generate invertible triangular systems from independent known x,y,z.
        // Expected values do not come from elimination or the returned proof.
        for seed in -7..=7 {
            let x = seed;
            let y = seed * 2 - 3;
            let z = 5 - seed;
            let result = solve_linear(
                matrix(&[&["2", "3", "1"], &["0", "-1", "4"], &["0", "0", "3"]]),
                vec![
                    (2 * x + 3 * y + z).to_string(),
                    (-y + 4 * z).to_string(),
                    (3 * z).to_string(),
                ],
            )
            .unwrap();
            let expected = vec![x.to_string(), y.to_string(), z.to_string()];
            assert_eq!(result.solution, Some(expected.clone()));
            let reordered = solve_linear(
                matrix(&[&["0", "0", "3"], &["2", "3", "1"], &["0", "-1", "4"]]),
                vec![
                    (3 * z).to_string(),
                    (2 * x + 3 * y + z).to_string(),
                    (-y + 4 * z).to_string(),
                ],
            )
            .unwrap();
            assert_eq!(reordered.solution, Some(expected));
        }
    }
    #[test]
    fn certificate_tampering_and_missing_rows_fail() {
        let original =
            solve_linear(matrix(&[&["1", "1"], &["1", "-1"]]), strings(&["5", "1"])).unwrap();
        let mut altered = original.clone();
        altered.solution = Some(strings(&["99", "2"]));
        assert!(verify_linear(&altered).is_err());
        let mut altered = original.clone();
        altered.operations.push(RowOperation::Scale {
            row: 0,
            factor: "0".into(),
        });
        assert!(verify_linear(&altered).is_err());
        let mut altered = original;
        altered.original_rhs[1] = "2".into();
        assert!(verify_linear(&altered).is_err());
    }
    #[test]
    fn dimensions_zero_equations_and_numeric_limits() {
        assert!(solve_linear(vec![], vec![]).is_err());
        assert!(solve_linear(matrix(&[&["1"], &["2", "3"]]), strings(&["1", "2"])).is_err());
        let all_free = solve_linear(matrix(&[&["0", "0"]]), strings(&["0"])).unwrap();
        assert_eq!(all_free.rank, 0);
        assert_eq!(
            all_free.nullspace_basis,
            matrix(&[&["1", "0"], &["0", "1"]])
        );
        let contradiction = solve_linear(matrix(&[&["0"]]), strings(&["1"])).unwrap();
        assert_eq!(contradiction.status, LinearStatus::Inconsistent);
        assert!(solve_linear(matrix(&[&["1/0"]]), strings(&["1"])).is_err());
        assert!(solve_linear(vec![vec!["9".repeat(1025)]], strings(&["1"])).is_err());
    }
    #[test]
    fn exact_statistics_even_median_and_variances() {
        let result = summarize(&strings(&["0.1", "0.2", "0.3", "0.4"]), None).unwrap();
        assert_eq!(result.sum, "1");
        assert_eq!(result.mean, "1/4");
        assert_eq!(result.median, "1/4");
        assert_eq!(result.population_variance, "1/80");
        assert_eq!(result.sample_variance, Some("1/60".into()));
        assert_eq!(result.min, "1/10");
        assert_eq!(result.max, "2/5");
        let reversed = summarize(&strings(&["0.4", "0.3", "0.2", "0.1"]), None).unwrap();
        assert_eq!(reversed.mean, result.mean);
        assert_eq!(reversed.population_variance, result.population_variance);
    }
    #[test]
    fn units_empty_population_and_single_observation() {
        assert!(summarize(&[], None).is_err());
        assert!(summarize(&strings(&["1", "2"]), Some(&strings(&["m", "s"]))).is_err());
        let result = summarize(&strings(&["-5"]), Some(&strings(&["m"]))).unwrap();
        assert_eq!(result.mean, "-5");
        assert_eq!(result.population_variance, "0");
        assert!(result.sample_variance.is_none());
        assert_eq!(result.unit, Some("m".into()));
        assert_eq!(result.variance_unit, Some("(m)^2".into()));
    }

    #[test]
    fn mixed_si_length_units_are_converted_exactly_before_statistics() {
        let result = summarize(&strings(&["1", "500"]), Some(&strings(&["km", "m"]))).unwrap();
        assert_eq!(result.input_values, ["1", "500"]);
        assert_eq!(result.input_units, Some(strings(&["km", "m"])));
        assert_eq!(result.normalized_values, ["1000", "500"]);
        assert_eq!(result.sorted_values, ["500", "1000"]);
        assert_eq!(result.unit, Some("m".into()));
        assert_eq!(result.variance_unit, Some("(m)^2".into()));
        assert_eq!(result.sum, "1500");
        assert_eq!(result.mean, "750");
        assert_eq!(result.median, "750");
        assert_eq!(result.population_variance, "62500");
        assert_eq!(result.sample_variance, Some("125000".into()));

        let decimal = summarize(
            &strings(&["0.1", "25", "50", "500"]),
            Some(&strings(&["km", "m", "cm", "mm"])),
        )
        .unwrap();
        assert_eq!(decimal.normalized_values, ["100", "25", "1/2", "1/2"]);
        assert_eq!(decimal.sum, "126");
    }
}

// The following sparse polynomial projection inherits public operations and
// explicit failure boundaries from the frozen symbolic component. It is not a
// general CAS: names, multiplication and division have a finite grammar.
use std::collections::{BTreeMap, BTreeSet};
use unicode_normalization::UnicodeNormalization;

const POLY_MAX_TERMS: usize = 256;
const POLY_MAX_INTERMEDIATE_TERMS: usize = 512;
const POLY_MAX_DEGREE: u16 = 64;
const POLY_MAX_NODES: usize = 512;
const POLY_MAX_DEPTH: usize = 32;
const POLY_MAX_COEFFICIENT_BITS: u64 = 1024;

type Monomial = BTreeMap<String, u16>;
type Polynomial = BTreeMap<Monomial, BigRational>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolynomialTerm {
    pub powers: BTreeMap<String, u16>,
    pub coefficient: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolicResult {
    pub original_expression: String,
    pub operation: String,
    pub variable: Option<String>,
    pub variables: Vec<String>,
    pub input_terms: Vec<PolynomialTerm>,
    pub canonical_expression: String,
    pub terms: Vec<PolynomialTerm>,
    pub operations: usize,
    pub trace: Vec<String>,
    /// An indefinite integral has an arbitrary independent additive term.
    pub integration_limit: Option<String>,
    pub verification: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolynomialEvaluationTerm {
    pub powers: BTreeMap<String, u16>,
    pub coefficient: String,
    pub monomial_value: String,
    pub contribution: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolynomialEvaluationReport {
    pub original_expression: String,
    pub variables: Vec<String>,
    pub bindings: BTreeMap<String, String>,
    pub terms: Vec<PolynomialEvaluationTerm>,
    pub value: String,
    pub operations: usize,
    pub verification: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LinearEquationKind {
    UniqueSolution,
    NoSolution,
    AllRealValues,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinearEquationSolution {
    pub equation_left: String,
    pub equation_right: String,
    pub variable: String,
    /// Coefficients are ordered [a, b] for a*x + b = 0.
    pub coefficients: [String; 2],
    pub kind: LinearEquationKind,
    pub exact_solution: Option<String>,
    pub operations: usize,
    pub verification: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum QuadraticRootKind {
    RepeatedRealRoot,
    TwoRationalRealRoots,
    TwoRadicalRealRoots,
    TwoComplexConjugates,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuadraticRoot {
    pub exact_value: String,
    pub representation: String,
    pub multiplicity: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuadraticSolution {
    pub equation_left: String,
    pub equation_right: String,
    pub variable: String,
    /// Coefficients are ordered [a, b, c] for a*x^2 + b*x + c = 0.
    pub coefficients: [String; 3],
    pub discriminant: String,
    pub root_kind: QuadraticRootKind,
    pub real_roots: Vec<QuadraticRoot>,
    pub complex_roots: Vec<String>,
    pub sum_of_roots: String,
    pub product_of_roots: String,
    pub operations: usize,
    pub verification: String,
}

#[derive(Default)]
struct PolyWork {
    operations: usize,
    trace: Vec<String>,
}
impl PolyWork {
    fn tick(&mut self) -> Result<()> {
        self.operations += 1;
        if self.operations > MAX_OPERATIONS {
            return Err(failure(
                "SYMBOLIC_OPERATION_LIMIT",
                "symbolic operation budget exceeded",
            ));
        }
        Ok(())
    }
    fn coefficient(&mut self, value: BigRational) -> Result<BigRational> {
        self.tick()?;
        if value.numer().bits() > POLY_MAX_COEFFICIENT_BITS
            || value.denom().bits() > POLY_MAX_COEFFICIENT_BITS
        {
            return Err(failure(
                "SYMBOLIC_COEFFICIENT_LIMIT",
                "polynomial coefficient exceeds 1024 bits",
            ));
        }
        Ok(value)
    }
    fn normalize(&mut self, mut value: Polynomial) -> Result<Polynomial> {
        value.retain(|_, coefficient| !coefficient.is_zero());
        if value.len() > POLY_MAX_TERMS {
            return Err(failure(
                "SYMBOLIC_TERM_LIMIT",
                "polynomial exceeds 256 terms",
            ));
        }
        for (powers, coefficient) in &value {
            self.tick()?;
            let degree: u16 = powers.values().sum();
            if degree > POLY_MAX_DEGREE || powers.len() > MAX_VARIABLES {
                return Err(failure(
                    "SYMBOLIC_DEGREE_LIMIT",
                    "polynomial total degree exceeds 64",
                ));
            }
            self.coefficient(coefficient.clone())?;
        }
        Ok(value)
    }
    fn add(&mut self, mut lhs: Polynomial, rhs: Polynomial, negative: bool) -> Result<Polynomial> {
        for (powers, coefficient) in rhs {
            let coefficient = if negative { -coefficient } else { coefficient };
            let value =
                self.coefficient(lhs.get(&powers).cloned().unwrap_or_default() + coefficient)?;
            if value.is_zero() {
                lhs.remove(&powers);
            } else {
                lhs.insert(powers, value);
            }
        }
        self.trace.push(
            if negative {
                "exact polynomial subtraction"
            } else {
                "exact polynomial addition"
            }
            .into(),
        );
        self.normalize(lhs)
    }
    fn multiply(&mut self, lhs: Polynomial, rhs: Polynomial) -> Result<Polynomial> {
        let mut out = Polynomial::new();
        for (a, x) in &lhs {
            for (b, y) in &rhs {
                self.tick()?;
                let mut powers = a.clone();
                for (name, degree) in b {
                    let exponent = powers.entry(name.clone()).or_default();
                    *exponent = exponent.checked_add(*degree).ok_or_else(|| {
                        failure("SYMBOLIC_DEGREE_LIMIT", "polynomial exponent overflow")
                    })?;
                }
                if powers.values().sum::<u16>() > POLY_MAX_DEGREE {
                    return Err(failure(
                        "SYMBOLIC_DEGREE_LIMIT",
                        "polynomial total degree exceeds 64",
                    ));
                }
                let product = self.coefficient(x * y)?;
                let coefficient =
                    self.coefficient(out.get(&powers).cloned().unwrap_or_default() + product)?;
                if coefficient.is_zero() {
                    out.remove(&powers);
                } else {
                    out.insert(powers, coefficient);
                }
                if out.len() > POLY_MAX_INTERMEDIATE_TERMS {
                    return Err(failure(
                        "SYMBOLIC_TERM_LIMIT",
                        "intermediate polynomial exceeds 512 terms",
                    ));
                }
            }
        }
        self.trace.push("exact polynomial multiplication".into());
        self.normalize(out)
    }
    fn divide_constant(&mut self, lhs: Polynomial, rhs: &Polynomial) -> Result<Polynomial> {
        let denominator = rhs.get(&Monomial::new()).cloned().unwrap_or_default();
        if denominator.is_zero() {
            return Err(failure("DIVISION_BY_ZERO", "zero polynomial denominator"));
        }
        if rhs.len() != 1 || !rhs.contains_key(&Monomial::new()) {
            return Err(failure(
                "VARIABLE_DENOMINATOR",
                "only nonzero constant denominators are supported",
            ));
        }
        let mut out = Polynomial::new();
        for (powers, coefficient) in lhs {
            out.insert(powers, self.coefficient(coefficient / &denominator)?);
        }
        self.trace.push("exact nonzero constant division".into());
        self.normalize(out)
    }
    fn power(&mut self, value: Polynomial, exponent: u16) -> Result<Polynomial> {
        let mut out = constant(BigRational::one());
        for _ in 0..exponent {
            out = self.multiply(out, value.clone())?;
        }
        self.trace
            .push(format!("nonnegative literal power {exponent}"));
        Ok(out)
    }
    fn transform(
        &mut self,
        value: &Polynomial,
        variable: &str,
        integrate: bool,
    ) -> Result<Polynomial> {
        let mut out = Polynomial::new();
        for (source_powers, coefficient) in value {
            let mut powers = source_powers.clone();
            let exponent = powers.get(variable).copied().unwrap_or(0);
            if !integrate && exponent == 0 {
                continue;
            }
            let new_exponent = if integrate {
                exponent + 1
            } else {
                exponent - 1
            };
            if new_exponent == 0 {
                powers.remove(variable);
            } else {
                powers.insert(variable.into(), new_exponent);
            }
            let multiplier = BigRational::from_integer(BigInt::from(if integrate {
                new_exponent
            } else {
                exponent
            }));
            let new_coefficient = self.coefficient(if integrate {
                coefficient / multiplier
            } else {
                coefficient * multiplier
            })?;
            if powers.values().sum::<u16>() > POLY_MAX_DEGREE {
                return Err(failure(
                    "SYMBOLIC_DEGREE_LIMIT",
                    "transformed polynomial degree exceeds 64",
                ));
            }
            out.insert(powers, new_coefficient);
        }
        self.trace.push(format!(
            "exact termwise {} with respect to {variable}",
            if integrate {
                "antiderivative"
            } else {
                "derivative"
            }
        ));
        self.normalize(out)
    }
}

fn constant(value: BigRational) -> Polynomial {
    if value.is_zero() {
        Polynomial::new()
    } else {
        BTreeMap::from([(Monomial::new(), value)])
    }
}
fn valid_variable(name: &str) -> Result<()> {
    let mut chars = name.chars();
    if name.len() > 256
        || name.chars().count() > 64
        || name.starts_with("__")
        || !chars
            .next()
            .is_some_and(|ch| ch == '_' || ch.is_alphabetic())
        || !chars.all(|ch| ch == '_' || ch.is_alphanumeric())
        || name.nfkc().collect::<String>() != name
    {
        return Err(failure(
            "SYMBOLIC_VARIABLE",
            "expected a normalized explicit variable identifier of at most 64 characters",
        ));
    }
    Ok(())
}

struct PolyValue {
    polynomial: Polynomial,
    /// Syntactic references are retained even after cancellation, so a variable
    /// denominator cannot evade its domain guard by simplifying to a constant.
    mentioned: BTreeSet<String>,
}
struct PolyParser<'a> {
    text: &'a str,
    at: usize,
    nodes: usize,
    variables: BTreeSet<String>,
    work: PolyWork,
}
impl<'a> PolyParser<'a> {
    fn new(text: &'a str) -> Result<Self> {
        if text.is_empty()
            || text.len() > 8192
            || text
                .chars()
                .any(|ch| matches!(ch, '#' | '\n' | '\r' | ';' | '\\'))
        {
            return Err(failure(
                "SYMBOLIC_INPUT_LIMIT",
                "expected one expression up to 8192 bytes without comments or statement separators",
            ));
        }
        Ok(Self {
            text,
            at: 0,
            nodes: 0,
            variables: BTreeSet::new(),
            work: PolyWork::default(),
        })
    }
    fn space(&mut self) {
        while let Some(ch) = self.peek() {
            if !ch.is_whitespace() {
                break;
            }
            self.at += ch.len_utf8();
        }
    }
    fn peek(&self) -> Option<char> {
        self.text[self.at..].chars().next()
    }
    fn take(&mut self, ch: char) -> bool {
        self.space();
        if self.peek() == Some(ch) {
            self.at += ch.len_utf8();
            true
        } else {
            false
        }
    }
    fn node(&mut self, depth: usize) -> Result<()> {
        self.nodes += 1;
        if self.nodes > POLY_MAX_NODES || depth > POLY_MAX_DEPTH {
            return Err(failure(
                "SYMBOLIC_STRUCTURE_LIMIT",
                "expression exceeds 512 nodes or depth 32",
            ));
        }
        Ok(())
    }
    fn expression(&mut self, depth: usize) -> Result<PolyValue> {
        let mut out = self.term(depth)?;
        loop {
            let negative = if self.take('+') {
                false
            } else if self.take('-') {
                true
            } else {
                break;
            };
            self.node(depth)?;
            let rhs = self.term(depth)?;
            out.polynomial = self.work.add(out.polynomial, rhs.polynomial, negative)?;
            out.mentioned.extend(rhs.mentioned);
        }
        Ok(out)
    }
    fn term(&mut self, depth: usize) -> Result<PolyValue> {
        let mut out = self.unary(depth)?;
        loop {
            self.space();
            if self.text[self.at..].starts_with("**") {
                break;
            }
            let divide = if self.take('*') {
                false
            } else if self.take('/') {
                true
            } else {
                break;
            };
            self.node(depth)?;
            let rhs = self.unary(depth)?;
            if divide {
                if !rhs.mentioned.is_empty() {
                    return Err(failure(
                        "VARIABLE_DENOMINATOR",
                        "a denominator mentioning a variable is outside the polynomial contract",
                    ));
                }
                out.polynomial = self.work.divide_constant(out.polynomial, &rhs.polynomial)?;
            } else {
                out.polynomial = self.work.multiply(out.polynomial, rhs.polynomial)?;
            }
            out.mentioned.extend(rhs.mentioned);
        }
        Ok(out)
    }
    fn unary(&mut self, depth: usize) -> Result<PolyValue> {
        self.node(depth)?;
        if self.take('+') {
            self.unary(depth + 1)
        } else if self.take('-') {
            let mut value = self.unary(depth + 1)?;
            value.polynomial = value
                .polynomial
                .into_iter()
                .map(|(powers, c)| (powers, -c))
                .collect();
            Ok(value)
        } else {
            self.power(depth + 1)
        }
    }
    fn power(&mut self, depth: usize) -> Result<PolyValue> {
        let mut value = self.atom(depth)?;
        self.space();
        let has_power = if self.text[self.at..].starts_with("**") {
            self.at += 2;
            true
        } else {
            self.take('^')
        };
        if has_power {
            self.node(depth)?;
            let exponent = self.exponent(depth + 1)?;
            value.polynomial = self.work.power(value.polynomial, exponent)?;
        }
        Ok(value)
    }
    fn exponent(&mut self, depth: usize) -> Result<u16> {
        self.node(depth)?;
        if self.take('(') {
            let exponent = self.exponent(depth + 1)?;
            if !self.take(')') {
                return Err(failure(
                    "SYMBOLIC_EXPONENT",
                    "expected closing parenthesis around integer exponent",
                ));
            }
            return Ok(exponent);
        }
        self.space();
        let start = self.at;
        while self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
            self.at += 1;
        }
        if self.at == start || self.at - start > 2 {
            return Err(failure(
                "SYMBOLIC_EXPONENT",
                "exponent must be a literal integer in 0..64",
            ));
        }
        let exponent: u16 = self.text[start..self.at]
            .parse()
            .map_err(|_| failure("SYMBOLIC_EXPONENT", "invalid exponent"))?;
        if exponent > POLY_MAX_DEGREE {
            return Err(failure("SYMBOLIC_EXPONENT", "exponent exceeds 64"));
        }
        Ok(exponent)
    }
    fn atom(&mut self, depth: usize) -> Result<PolyValue> {
        self.node(depth)?;
        if self.take('(') {
            let value = self.expression(depth + 1)?;
            if !self.take(')') {
                return Err(failure(
                    "INVALID_SYMBOLIC_EXPRESSION",
                    "missing closing parenthesis",
                ));
            }
            return Ok(value);
        }
        self.space();
        let start = self.at;
        if self
            .peek()
            .is_some_and(|ch| ch == '_' || ch.is_alphabetic())
        {
            while self
                .peek()
                .is_some_and(|ch| ch == '_' || ch.is_alphanumeric())
            {
                self.at += self.peek().expect("checked character").len_utf8();
            }
            let name = &self.text[start..self.at];
            valid_variable(name)?;
            self.variables.insert(name.into());
            if self.variables.len() > MAX_VARIABLES {
                return Err(failure(
                    "SYMBOLIC_VARIABLE_LIMIT",
                    "more than eight variables",
                ));
            }
            return Ok(PolyValue {
                polynomial: BTreeMap::from([(
                    BTreeMap::from([(name.into(), 1)]),
                    BigRational::one(),
                )]),
                mentioned: BTreeSet::from([name.into()]),
            });
        }
        let mut digits = 0usize;
        while self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
            self.at += 1;
            digits += 1;
        }
        if self.peek() == Some('.') {
            self.at += 1;
            while self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
                self.at += 1;
                digits += 1;
            }
        }
        if digits == 0 {
            return Err(failure(
                "INVALID_SYMBOLIC_EXPRESSION",
                format!("number, identifier or parenthesis expected at byte {start}"),
            ));
        }
        if matches!(self.peek(), Some('e' | 'E')) {
            self.at += 1;
            if matches!(self.peek(), Some('+' | '-')) {
                self.at += 1;
            }
            let exponent_start = self.at;
            while self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
                self.at += 1;
            }
            if self.at == exponent_start {
                return Err(failure(
                    "INVALID_SYMBOLIC_EXPRESSION",
                    "missing decimal exponent",
                ));
            }
        }
        let value = self.work.coefficient(scalar(&self.text[start..self.at])?)?;
        Ok(PolyValue {
            polynomial: constant(value),
            mentioned: BTreeSet::new(),
        })
    }
    fn finish(&mut self) -> Result<Polynomial> {
        let value = self.expression(0)?;
        self.space();
        if self.at != self.text.len() {
            return Err(failure(
                "INVALID_SYMBOLIC_EXPRESSION",
                format!("unexpected input at byte {}", self.at),
            ));
        }
        self.work.normalize(value.polynomial)
    }
}

fn polynomial_terms(polynomial: &Polynomial, variables: &[String]) -> Vec<PolynomialTerm> {
    let mut terms: Vec<_> = polynomial
        .iter()
        .map(|(powers, c)| PolynomialTerm {
            powers: powers.clone(),
            coefficient: rational_text(c),
        })
        .collect();
    terms.sort_by(|a, b| {
        let av: Vec<_> = variables
            .iter()
            .map(|name| a.powers.get(name).copied().unwrap_or(0))
            .collect();
        let bv: Vec<_> = variables
            .iter()
            .map(|name| b.powers.get(name).copied().unwrap_or(0))
            .collect();
        bv.cmp(&av)
    });
    terms
}
fn polynomial_text(polynomial: &Polynomial, variables: &[String]) -> String {
    let terms = polynomial_terms(polynomial, variables);
    let mut text = String::new();
    for term in terms {
        let coefficient =
            BigRational::from_str(&term.coefficient).expect("internally canonical rational");
        let negative = coefficient < BigRational::zero();
        let magnitude = if negative { -coefficient } else { coefficient };
        let mut factors = Vec::new();
        if magnitude != BigRational::one() || term.powers.is_empty() {
            factors.push(rational_text(&magnitude));
        }
        for (name, exponent) in term.powers {
            factors.push(if exponent == 1 {
                name
            } else {
                format!("{name}**{exponent}")
            });
        }
        if text.is_empty() {
            if negative {
                text.push('-');
            }
        } else {
            text.push_str(if negative { " - " } else { " + " });
        }
        text.push_str(&factors.join("*"));
    }
    if text.is_empty() { "0".into() } else { text }
}

/// Bounded exact polynomial normalization, formal derivative, or representative
/// antiderivative. Explicit multiplication is required. Variable denominators,
/// functions, implicit multiplication and noninteger/negative powers fail.
/// `^` is a documented Rust input alias for the inherited `**` notation.
pub fn symbolic(expression: &str, operation: &str, variable: &str) -> Result<SymbolicResult> {
    if !matches!(operation, "simplify" | "derive" | "integrate") {
        return Err(failure(
            "SYMBOLIC_OPERATION",
            "expected simplify, derive or integrate",
        ));
    }
    let mut parser = PolyParser::new(expression)?;
    let original = parser.finish()?;
    let variable = if operation == "simplify" {
        if !variable.is_empty() {
            valid_variable(variable)?;
        }
        None
    } else {
        valid_variable(variable)?;
        parser.variables.insert(variable.into());
        if parser.variables.len() > MAX_VARIABLES {
            return Err(failure(
                "SYMBOLIC_VARIABLE_LIMIT",
                "more than eight variables including transformation target",
            ));
        }
        Some(variable.to_string())
    };
    let result = match operation {
        "derive" => parser.work.transform(
            &original,
            variable.as_deref().expect("validated target"),
            false,
        )?,
        "integrate" => parser.work.transform(
            &original,
            variable.as_deref().expect("validated target"),
            true,
        )?,
        _ => original.clone(),
    };
    if operation == "integrate" {
        let reconstructed = parser.work.transform(
            &result,
            variable.as_deref().expect("validated target"),
            false,
        )?;
        if reconstructed != original {
            return Err(failure(
                "SYMBOLIC_VERIFICATION",
                "antiderivative differentiation mismatch",
            ));
        }
    }
    let variables: Vec<_> = parser.variables.into_iter().collect();
    let canonical_expression = polynomial_text(&result, &variables);
    // Independently traverse the emitted grammar rather than trusting display.
    let mut roundtrip = PolyParser::new(&canonical_expression)?;
    if roundtrip.finish()? != result {
        return Err(failure(
            "SYMBOLIC_VERIFICATION",
            "rendered polynomial does not reconstruct the result",
        ));
    }
    Ok(SymbolicResult{
        original_expression:expression.into(),operation:operation.into(),variable,
        input_terms:polynomial_terms(&original,&variables),canonical_expression,
        terms:polynomial_terms(&result,&variables),variables,
        operations:parser.work.operations,trace:parser.work.trace,
        integration_limit:(operation=="integrate").then(||"one representative antiderivative; an arbitrary term independent of the target variable may be added".into()),
        verification:"exact sparse polynomial operations; termwise formal transformation; emitted expression reparsed; antiderivative differentiated back to the supplied polynomial".into(),
    })
}

/// Evaluate the complete supported polynomial at explicit exact rational values.
/// Missing variables fail; additional bounded valid bindings do not change it.
pub fn evaluate_polynomial_report(
    expression: &str,
    bindings: &BTreeMap<String, String>,
) -> Result<PolynomialEvaluationReport> {
    let mut parser = PolyParser::new(expression)?;
    let polynomial = parser.finish()?;
    if bindings.len() > MAX_VARIABLES {
        return Err(failure(
            "SYMBOLIC_VARIABLE_LIMIT",
            "more than eight bindings",
        ));
    }
    let mut values = BTreeMap::new();
    for (name, value) in bindings {
        valid_variable(name)?;
        values.insert(name.clone(), parser.work.coefficient(scalar(value)?)?);
    }
    if parser
        .variables
        .iter()
        .any(|name| !values.contains_key(name))
    {
        return Err(failure(
            "UNBOUND_SYMBOLIC_VARIABLE",
            "every source variable must have an explicit binding",
        ));
    }
    let variables: Vec<_> = parser.variables.iter().cloned().collect();
    let mut total = BigRational::zero();
    let mut terms = Vec::new();
    for term in polynomial_terms(&polynomial, &variables) {
        let coefficient = polynomial
            .get(&term.powers)
            .expect("projected polynomial term exists")
            .clone();
        let mut monomial_value = BigRational::one();
        for (name, exponent) in &term.powers {
            let bound = values
                .get(name)
                .expect("all source polynomial variables were checked");
            let power = parser.work.coefficient(bound.pow(i32::from(*exponent)))?;
            monomial_value = parser.work.coefficient(monomial_value * power)?;
        }
        let contribution = parser.work.coefficient(&coefficient * &monomial_value)?;
        total = parser.work.coefficient(total + &contribution)?;
        terms.push(PolynomialEvaluationTerm {
            powers: term.powers,
            coefficient: rational_text(&coefficient),
            monomial_value: rational_text(&monomial_value),
            contribution: rational_text(&contribution),
        });
    }
    Ok(PolynomialEvaluationReport {
        original_expression: expression.into(),
        variables,
        bindings: bindings.clone(),
        terms,
        value: rational_text(&total),
        operations: parser.work.operations,
        verification: "complete parsed polynomial evaluated term by term with exact rational powers; reported contributions sum to the returned value".into(),
    })
}

pub fn evaluate_polynomial(
    expression: &str,
    bindings: &BTreeMap<String, String>,
) -> Result<String> {
    evaluate_polynomial_report(expression, bindings).map(|report| report.value)
}

/// Solve one bounded single-variable linear equation with exact rationals.
pub fn solve_linear_equation(left: &str, right: &str) -> Result<LinearEquationSolution> {
    let expression = format!("({left})-({right})");
    if expression.len() > 8192 {
        return Err(failure(
            "LINEAR_EQUATION_INPUT_LIMIT",
            "combined equation exceeds the 8192-byte symbolic expression limit",
        ));
    }
    let mut parser = PolyParser::new(&expression)?;
    let polynomial = parser.finish()?;
    if parser.variables.len() != 1 {
        return Err(failure(
            "LINEAR_EQUATION_VARIABLES",
            "a linear equation must mention exactly one variable",
        ));
    }
    let variable = parser
        .variables
        .iter()
        .next()
        .expect("one variable was checked")
        .clone();
    let mut coefficients = [BigRational::zero(), BigRational::zero()];
    for (monomial, coefficient) in &polynomial {
        if monomial.keys().any(|name| name != &variable) {
            return Err(failure(
                "LINEAR_EQUATION_VARIABLES",
                "all nonconstant terms must use the same variable",
            ));
        }
        let degree = monomial.get(&variable).copied().unwrap_or(0);
        if degree > 1 {
            return Err(failure(
                "LINEAR_EQUATION_DEGREE",
                "the normalized equation has degree greater than one",
            ));
        }
        coefficients[degree as usize] = coefficient.clone();
    }
    let slope = coefficients[1].clone();
    let intercept = coefficients[0].clone();
    let mut budget = Budget::default();
    let (kind, exact_solution) = if slope.is_zero() {
        if intercept.is_zero() {
            (LinearEquationKind::AllRealValues, None)
        } else {
            (LinearEquationKind::NoSolution, None)
        }
    } else {
        let solution = budget.check(-intercept.clone() / slope.clone())?;
        let linear_term = budget.check(slope.clone() * solution.clone())?;
        let residual = budget.check(linear_term + intercept.clone())?;
        if !residual.is_zero() {
            return Err(failure(
                "LINEAR_EQUATION_VERIFICATION",
                "the rational solution failed exact substitution",
            ));
        }
        (
            LinearEquationKind::UniqueSolution,
            Some(rational_text(&solution)),
        )
    };

    Ok(LinearEquationSolution {
        equation_left: left.into(),
        equation_right: right.into(),
        variable,
        coefficients: [rational_text(&slope), rational_text(&intercept)],
        kind,
        exact_solution,
        operations: parser.work.operations + budget.0,
        verification: "both sides were reduced by exact sparse-polynomial arithmetic; a unique rational solution was substituted into the normalized difference exactly; identity and contradiction cases are classified from exact coefficients; no decimal approximation".into(),
    })
}

fn integer_square_root(value: &BigInt) -> BigInt {
    if value <= &BigInt::zero() {
        return BigInt::zero();
    }
    let mut estimate = BigInt::one() << value.bits().div_ceil(2) as usize;
    loop {
        let next = (&estimate + value / &estimate) >> 1;
        if next >= estimate {
            return estimate;
        }
        estimate = next;
    }
}

fn exact_rational_square_root(value: &BigRational) -> Option<BigRational> {
    if value < &BigRational::zero() {
        return None;
    }
    let numerator = integer_square_root(value.numer());
    let denominator = integer_square_root(value.denom());
    if numerator.clone() * &numerator != value.numer().clone()
        || denominator.clone() * &denominator != value.denom().clone()
    {
        return None;
    }
    Some(BigRational::new(numerator, denominator))
}

/// Solve one single-variable degree-two equation using the existing exact
/// polynomial grammar. Non-rational roots are retained as exact radicals.
pub fn solve_quadratic(left: &str, right: &str) -> Result<QuadraticSolution> {
    let expression = format!("({left})-({right})");
    if expression.len() > 8192 {
        return Err(failure(
            "QUADRATIC_INPUT_LIMIT",
            "combined equation exceeds the 8192-byte symbolic expression limit",
        ));
    }
    let mut parser = PolyParser::new(&expression)?;
    let polynomial = parser.finish()?;
    if parser.variables.len() != 1 {
        return Err(failure(
            "QUADRATIC_VARIABLES",
            "a quadratic equation must mention exactly one variable",
        ));
    }
    let variable = parser
        .variables
        .iter()
        .next()
        .expect("one variable was checked")
        .clone();
    let mut coefficients = vec![BigRational::zero(); 3];
    for (monomial, coefficient) in &polynomial {
        if monomial.keys().any(|name| name != &variable) {
            return Err(failure(
                "QUADRATIC_VARIABLES",
                "all nonconstant terms must use the same variable",
            ));
        }
        let degree = monomial.get(&variable).copied().unwrap_or(0);
        if degree > 2 {
            return Err(failure(
                "QUADRATIC_DEGREE",
                "the normalized equation has degree greater than two",
            ));
        }
        coefficients[degree as usize] = coefficient.clone();
    }
    let a = coefficients[2].clone();
    let b = coefficients[1].clone();
    let c = coefficients[0].clone();
    if a.is_zero() {
        return Err(failure(
            "QUADRATIC_LEADING_COEFFICIENT",
            "the normalized equation must have a nonzero degree-two coefficient",
        ));
    }

    let mut budget = Budget::default();
    let b_squared = budget.check(b.clone() * b.clone())?;
    let ac = budget.check(a.clone() * c.clone())?;
    let four_ac = budget.check(BigRational::from_integer(BigInt::from(4)) * ac)?;
    let discriminant = budget.check(b_squared - four_ac)?;
    let two_a = budget.check(BigRational::from_integer(BigInt::from(2)) * a.clone())?;
    let sum_of_roots = budget.check(-b.clone() / a.clone())?;
    let product_of_roots = budget.check(c.clone() / a.clone())?;
    let base_numerator = rational_text(&(-b.clone()));
    let denominator = rational_text(&two_a);

    let (root_kind, real_roots, complex_roots) = if discriminant.is_zero() {
        let root = budget.check(-b.clone() / two_a.clone())?;
        let a_root_squared = budget.check(a.clone() * root.clone() * root.clone())?;
        let b_root = budget.check(b.clone() * root.clone())?;
        let ab = budget.check(a_root_squared + b_root)?;
        let residual = budget.check(ab + c.clone())?;
        if !residual.is_zero() {
            return Err(failure(
                "QUADRATIC_VERIFICATION",
                "the repeated rational root failed exact substitution",
            ));
        }
        (
            QuadraticRootKind::RepeatedRealRoot,
            vec![QuadraticRoot {
                exact_value: rational_text(&root),
                representation: "RATIONAL".into(),
                multiplicity: 2,
            }],
            Vec::new(),
        )
    } else if discriminant > BigRational::zero() {
        if let Some(square_root) = exact_rational_square_root(&discriminant) {
            let mut roots = vec![
                budget.check((-b.clone() - square_root.clone()) / two_a.clone())?,
                budget.check((-b.clone() + square_root) / two_a.clone())?,
            ];
            roots.sort();
            for root in &roots {
                let a_root_squared = budget.check(a.clone() * root.clone() * root.clone())?;
                let b_root = budget.check(b.clone() * root.clone())?;
                let ab = budget.check(a_root_squared + b_root)?;
                let residual = budget.check(ab + c.clone())?;
                if !residual.is_zero() {
                    return Err(failure(
                        "QUADRATIC_VERIFICATION",
                        "a rational root failed exact substitution",
                    ));
                }
            }
            (
                QuadraticRootKind::TwoRationalRealRoots,
                roots
                    .iter()
                    .map(|root| QuadraticRoot {
                        exact_value: rational_text(root),
                        representation: "RATIONAL".into(),
                        multiplicity: 1,
                    })
                    .collect(),
                Vec::new(),
            )
        } else {
            let radical = rational_text(&discriminant);
            let minus = format!("(({base_numerator}) - sqrt({radical})) / ({denominator})");
            let plus = format!("(({base_numerator}) + sqrt({radical})) / ({denominator})");
            (
                QuadraticRootKind::TwoRadicalRealRoots,
                vec![
                    QuadraticRoot {
                        exact_value: minus,
                        representation: "EXACT_RADICAL".into(),
                        multiplicity: 1,
                    },
                    QuadraticRoot {
                        exact_value: plus,
                        representation: "EXACT_RADICAL".into(),
                        multiplicity: 1,
                    },
                ],
                Vec::new(),
            )
        }
    } else {
        let radical = rational_text(&(-discriminant.clone()));
        let minus = format!("(({base_numerator}) - i*sqrt({radical})) / ({denominator})");
        let plus = format!("(({base_numerator}) + i*sqrt({radical})) / ({denominator})");
        (
            QuadraticRootKind::TwoComplexConjugates,
            Vec::new(),
            vec![minus, plus],
        )
    };

    Ok(QuadraticSolution {
        equation_left: left.into(),
        equation_right: right.into(),
        variable,
        coefficients: [rational_text(&a), rational_text(&b), rational_text(&c)],
        discriminant: rational_text(&discriminant),
        root_kind,
        real_roots,
        complex_roots,
        sum_of_roots: rational_text(&sum_of_roots),
        product_of_roots: rational_text(&product_of_roots),
        operations: parser.work.operations + budget.0,
        verification: "both sides were reduced by exact sparse-polynomial arithmetic; rational real roots were substituted exactly; radical forms follow the exact quadratic formula; root sum and product identities use exact rational arithmetic; no decimal approximation".into(),
    })
}

#[cfg(test)]
mod polynomial_tests {
    use super::*;
    fn values(bindings: &[(&str, &str)]) -> BTreeMap<String, String> {
        bindings
            .iter()
            .map(|(name, value)| ((*name).into(), (*value).into()))
            .collect()
    }
    #[test]
    fn exact_linear_equations_classify_unique_identity_and_contradiction() {
        let unique = solve_linear_equation("2*x + 1", "0").unwrap();
        assert_eq!(unique.variable, "x");
        assert_eq!(unique.coefficients, ["2", "1"]);
        assert_eq!(unique.kind, LinearEquationKind::UniqueSolution);
        assert_eq!(unique.exact_solution.as_deref(), Some("-1/2"));

        let identity = solve_linear_equation("x", "x").unwrap();
        assert_eq!(identity.coefficients, ["0", "0"]);
        assert_eq!(identity.kind, LinearEquationKind::AllRealValues);
        assert_eq!(identity.exact_solution, None);

        let contradiction = solve_linear_equation("x + 1", "x + 2").unwrap();
        assert_eq!(contradiction.coefficients, ["0", "-1"]);
        assert_eq!(contradiction.kind, LinearEquationKind::NoSolution);
        assert_eq!(contradiction.exact_solution, None);
    }
    #[test]
    fn exact_linear_equations_reject_unsupported_variables_and_degree() {
        for (left, right, code) in [
            ("x + y", "2", "LINEAR_EQUATION_VARIABLES"),
            ("x**2", "1", "LINEAR_EQUATION_DEGREE"),
            ("2", "1", "LINEAR_EQUATION_VARIABLES"),
        ] {
            let error = solve_linear_equation(left, right).unwrap_err();
            assert!(error.to_string().starts_with(code), "{error}");
        }
    }
    #[test]
    fn exact_quadratic_equations_cover_rational_repeated_radical_and_complex_roots() {
        let rational = solve_quadratic("2*x^2 - 3*x + 1", "0").unwrap();
        assert_eq!(rational.coefficients, ["2", "-3", "1"]);
        assert_eq!(rational.discriminant, "1");
        assert_eq!(rational.root_kind, QuadraticRootKind::TwoRationalRealRoots);
        assert_eq!(
            rational
                .real_roots
                .iter()
                .map(|root| root.exact_value.as_str())
                .collect::<Vec<_>>(),
            ["1/2", "1"]
        );

        let repeated = solve_quadratic("x^2 + 2*x + 1", "0").unwrap();
        assert_eq!(repeated.root_kind, QuadraticRootKind::RepeatedRealRoot);
        assert_eq!(repeated.real_roots[0].exact_value, "-1");
        assert_eq!(repeated.real_roots[0].multiplicity, 2);

        let radical = solve_quadratic("x^2", "2").unwrap();
        assert_eq!(radical.root_kind, QuadraticRootKind::TwoRadicalRealRoots);
        assert!(radical.real_roots.iter().all(|root| {
            root.representation == "EXACT_RADICAL" && root.exact_value.contains("sqrt(8)")
        }));

        let complex = solve_quadratic("x^2 + 1", "0").unwrap();
        assert_eq!(complex.root_kind, QuadraticRootKind::TwoComplexConjugates);
        assert!(complex.real_roots.is_empty());
        assert_eq!(complex.complex_roots.len(), 2);
        assert!(
            complex
                .complex_roots
                .iter()
                .all(|root| root.contains("i*sqrt(4)"))
        );
    }
    #[test]
    fn exact_quadratic_equations_reject_unsupported_variables_and_degrees() {
        for (left, right) in [("x^2 + y", "0"), ("x**3", "0"), ("x^2", "x^2 + 1")] {
            assert!(solve_quadratic(left, right).is_err(), "{left}={right}");
        }
    }
    #[test]
    fn normalization_and_full_roundtrip() {
        let result = symbolic("(x + 1)*(x - 1) + 1/2*y - y/2", "simplify", "").unwrap();
        assert_eq!(result.canonical_expression, "x**2 - 1");
        assert_eq!(result.variables, vec!["x", "y"]);
        assert_eq!(
            evaluate_polynomial(&result.canonical_expression, &values(&[("x", "3")])).unwrap(),
            "8"
        );
        assert!(symbolic("x + 2 trailing", "simplify", "").is_err());
        assert!(symbolic("2x", "simplify", "").is_err());
        assert!(symbolic("sin(x)", "simplify", "").is_err());
    }
    #[test]
    fn exact_derivative_and_antiderivative() {
        let derivative = symbolic("3*x**3 + x*y - 0.5*x + 7", "derive", "x").unwrap();
        assert_eq!(
            evaluate_polynomial(
                &derivative.canonical_expression,
                &values(&[("x", "2"), ("y", "5")])
            )
            .unwrap(),
            "81/2"
        );
        let integral = symbolic("x**2 + 1/2*y", "integrate", "x").unwrap();
        assert!(integral.integration_limit.is_some());
        let recovered = symbolic(&integral.canonical_expression, "derive", "x").unwrap();
        assert_eq!(
            recovered.terms,
            symbolic("x**2 + y/2", "simplify", "").unwrap().terms
        );
        assert_eq!(
            symbolic("7", "derive", "x").unwrap().canonical_expression,
            "0"
        );
    }
    #[test]
    fn independent_quadratic_difference_identity() {
        // For p=a*x²+b*x+c, p(x+1)-p(x-1)=2*p'(x).
        // This checks derivatives by exact evaluations, rather than reusing
        // the derivative implementation to compute expected coefficients.
        for a in -3..=3 {
            for b in -2..=2 {
                let expression = format!("{a}*x**2 + {b}*x + 7/3");
                let derivative = symbolic(&expression, "derive", "x").unwrap();
                for x in -3..=3 {
                    let plus = scalar(
                        &evaluate_polynomial(
                            &expression,
                            &BTreeMap::from([("x".into(), (x + 1).to_string())]),
                        )
                        .unwrap(),
                    )
                    .unwrap();
                    let minus = scalar(
                        &evaluate_polynomial(
                            &expression,
                            &BTreeMap::from([("x".into(), (x - 1).to_string())]),
                        )
                        .unwrap(),
                    )
                    .unwrap();
                    let actual = scalar(
                        &evaluate_polynomial(
                            &derivative.canonical_expression,
                            &BTreeMap::from([("x".into(), x.to_string())]),
                        )
                        .unwrap(),
                    )
                    .unwrap();
                    assert_eq!(
                        plus - minus,
                        actual * BigRational::from_integer(BigInt::from(2))
                    );
                }
            }
        }
    }
    #[test]
    fn domain_guards_do_not_cancel_away_variable_denominators() {
        for expr in [
            "x/x",
            "1/(x-x+1)",
            "x**-1",
            "x**(1+1)",
            "x**2.0",
            "x**65",
            "x; y",
            "x\n+y",
        ] {
            assert!(symbolic(expr, "simplify", "").is_err(), "{expr}");
        }
        assert!(symbolic("x/(1-1)", "simplify", "").is_err());
        assert!(symbolic("x**64", "integrate", "x").is_err());
        assert!(
            symbolic(
                "((((((((((((((((((((((((((((((((((((x))))))))))))))))))))))))))))))))))))",
                "simplify",
                ""
            )
            .is_err()
        );
        assert!(evaluate_polynomial("x+y", &values(&[("x", "2")])).is_err());
    }
    #[test]
    fn unicode_names_rational_coefficients_and_limits() {
        let derivative = symbolic("質量^2 + 0.1*質量", "derive", "質量").unwrap();
        assert_eq!(
            evaluate_polynomial(
                &derivative.canonical_expression,
                &values(&[("質量", "1/3")])
            )
            .unwrap(),
            "23/30"
        );
        assert!(symbolic("ｘ+1", "simplify", "").is_err());
        assert!(symbolic("a+b+c+d+e+f+g+h+i", "simplify", "").is_err());
        assert!(symbolic("1e1000*x", "simplify", "").is_err());
        assert!(symbolic("x", "derive", "__bad").is_err());
        assert!(symbolic("x", "unknown", "x").is_err());
    }
}

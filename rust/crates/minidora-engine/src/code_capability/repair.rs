//! Evidence-bound correction of simple integer boundary conditions.

use super::{
    BranchObservation, CodeOperation, CodeValue, CodeVerificationReport, CompareOperator, Expr,
    ExprKind, Function, PythonParser, Span, Stmt, StmtKind, UnaryOperator, VerificationTrace,
    collect_locals, parse_input, verify_observing,
};
use minidora_domain::{Result, digest, failure};
use num_bigint::BigInt;
use num_traits::One;
use serde::Serialize;
use std::collections::BTreeSet;
use std::time::Instant;

const VERSION: &str = "bounded-python-comparison-repair-v1";
const MAX_CANDIDATES: usize = 16;
const MAX_TOTAL_STEPS: usize = 300_000;
const ASSURANCE: &str = "同じ指定試験を、ミニドラの有限評価器で修正前後に確認しました。修正案の適用範囲は、観測した境界条件と試験入力です。";

#[derive(Debug, Serialize)]
pub(crate) struct CodeRepairReport {
    pub version: &'static str,
    pub status: &'static str,
    pub function_name: String,
    pub original_source: String,
    pub original_source_sha256: String,
    pub corrected_source: String,
    pub corrected_source_sha256: String,
    pub edit: CodeRepairEdit,
    pub before: CodeVerificationReport,
    pub after: CodeVerificationReport,
    pub witnesses: Vec<CodeRepairWitness>,
    pub assurance: &'static str,
}

#[derive(Debug, Serialize)]
pub(crate) struct CodeRepairEdit {
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub utf8_column: usize,
    pub before: &'static str,
    pub after: &'static str,
    pub parameter: String,
    pub threshold: String,
}

/// Case numbers use the same zero-based index as CodeCaseObservation.
#[derive(Debug, Serialize)]
pub(crate) struct CodeRepairWitness {
    pub below_case: usize,
    pub boundary_case: usize,
    pub above_case: usize,
    pub below_condition: bool,
    pub branch_before: bool,
    pub branch_after: bool,
    pub above_condition: bool,
}

struct BoundaryComparison<'a> {
    parameter: &'a str,
    threshold: BigInt,
    span: Span,
    before: &'static str,
    after: &'static str,
    below_condition: bool,
    boundary_condition: bool,
}

fn budget_error() -> minidora_domain::Error {
    failure(
        "CODE_REPAIR_BUDGET",
        "修正候補の絞り込みと再評価を、指定された時間・手数の範囲で完了できませんでした。",
    )
}

fn deadline_check(deadline: Option<Instant>) -> Result<()> {
    if deadline.is_some_and(|limit| Instant::now() >= limit) {
        return Err(budget_error());
    }
    Ok(())
}

fn signed_literal(expr: &Expr) -> Option<BigInt> {
    match &expr.kind {
        ExprKind::Constant(CodeValue::Integer(value)) => Some(value.clone()),
        ExprKind::Unary(operation @ (UnaryOperator::Plus | UnaryOperator::Minus), value) => {
            let ExprKind::Constant(CodeValue::Integer(integer)) = &value.kind else {
                return None;
            };
            Some(if *operation == UnaryOperator::Minus {
                -integer
            } else {
                integer.clone()
            })
        }
        _ => None,
    }
}

fn comparison<'a>(
    source: &str,
    test: &'a Expr,
    parameters: &BTreeSet<&str>,
) -> Option<BoundaryComparison<'a>> {
    let ExprKind::Compare(left, rest) = &test.kind else {
        return None;
    };
    let [(operator, right)] = rest.as_slice() else {
        return None;
    };
    let (before, after, less) = match operator {
        CompareOperator::Less => ("<", "<=", true),
        CompareOperator::LessEqual => ("<=", "<", true),
        CompareOperator::Greater => (">", ">=", false),
        CompareOperator::GreaterEqual => (">=", ">", false),
        _ => return None,
    };
    let (parameter, threshold, parameter_on_left) = match (&left.kind, &right.kind) {
        (ExprKind::Name(parameter), _) => (parameter.as_str(), signed_literal(right)?, true),
        (_, ExprKind::Name(parameter)) => (parameter.as_str(), signed_literal(left)?, false),
        _ => return None,
    };
    if !parameters.contains(parameter) {
        return None;
    }
    let left_end = left.surface_span;
    let gap = source.get(left_end.end..right.surface_span.start)?;
    let trimmed = gap.trim_start();
    if trimmed.trim_end() != before || gap.contains(['\r', '\n']) {
        return None;
    }
    let leading = gap.len() - trimmed.len();
    let start = left_end.end + leading;
    let column = left_end.end_column + leading;
    Some(BoundaryComparison {
        parameter,
        threshold,
        span: Span {
            start,
            end: start + before.len(),
            line: left_end.end_line,
            column,
            end_line: left_end.end_line,
            end_column: column + before.len(),
        },
        before,
        after,
        below_condition: less == parameter_on_left,
        boundary_condition: before.len() == 2,
    })
}

fn comparisons<'a>(
    source: &str,
    function: &'a Function,
    deadline: Option<Instant>,
) -> Result<Vec<BoundaryComparison<'a>>> {
    let mut written = BTreeSet::new();
    collect_locals(&function.body, &mut written);
    let parameters = function
        .args
        .iter()
        .map(|argument| argument.name.as_str())
        .filter(|name| !written.contains(*name))
        .collect();
    let mut statements: Vec<&Stmt> = function.body.iter().rev().collect();
    let mut guards = Vec::new();
    while let Some(statement) = statements.pop() {
        deadline_check(deadline)?;
        if let StmtKind::If(test, body, otherwise) = &statement.kind {
            if let Some(guard) = comparison(source, test, &parameters) {
                guards.push(guard);
            }
            statements.extend(otherwise.iter().rev());
            statements.extend(body.iter().rev());
        }
    }
    Ok(guards)
}

fn branch(trace: &[Vec<BranchObservation>], case: usize, line: usize) -> Option<bool> {
    let mut observations = trace.get(case)?.iter().filter(|item| item.line == line);
    let selected = observations.next()?;
    observations.next().is_none().then_some(selected.condition)
}

struct ComparisonInput<'a> {
    argument: &'a BigInt,
    other_arguments: String,
}

fn comparison_inputs<'a>(
    cases: &'a [(CodeValue, CodeValue)],
    parameter: &str,
) -> Result<Option<Vec<ComparisonInput<'a>>>> {
    let mut inputs = Vec::with_capacity(cases.len());
    for (arguments, _) in cases {
        let CodeValue::Dict(fields) = arguments else {
            return Ok(None);
        };
        let Some((_, CodeValue::Integer(argument))) =
            fields.iter().find(|(name, _)| name == parameter)
        else {
            return Ok(None);
        };
        let mut other_arguments: Vec<_> = fields
            .iter()
            .filter(|(name, _)| name != parameter)
            .cloned()
            .collect();
        // Keyword order does not affect binding. The program can observe
        // the insertion order inside a dictionary passed as an argument.
        other_arguments.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));
        let other_arguments = CodeValue::Dict(other_arguments).to_json(false)?;
        inputs.push(ComparisonInput {
            argument,
            other_arguments,
        });
    }
    Ok(Some(inputs))
}

fn before_witnesses(
    guard: &BoundaryComparison,
    inputs: &[ComparisonInput],
    before: &CodeVerificationReport,
    trace: &[Vec<BranchObservation>],
) -> Option<Vec<CodeRepairWitness>> {
    let lower = &guard.threshold - BigInt::one();
    let upper = &guard.threshold + BigInt::one();
    let observe = |index| branch(trace, index, guard.span.line);
    let mut witnesses = Vec::new();
    for mismatch in before.cases.iter().filter(|case| !case.matched) {
        let input = &inputs[mismatch.number];
        if *input.argument != guard.threshold {
            return None;
        }
        let neighbor = |expected, condition| {
            before
                .cases
                .iter()
                .zip(inputs)
                .find(|(row, candidate)| {
                    row.matched
                        && candidate.argument == expected
                        && candidate.other_arguments == input.other_arguments
                        && observe(row.number) == Some(condition)
                })
                .map(|(row, _)| row.number)
        };
        witnesses.push(CodeRepairWitness {
            below_case: neighbor(&lower, guard.below_condition)?,
            boundary_case: mismatch.number,
            above_case: neighbor(&upper, !guard.below_condition)?,
            below_condition: guard.below_condition,
            branch_before: guard.boundary_condition,
            branch_after: !guard.boundary_condition,
            above_condition: !guard.below_condition,
        });
    }
    Some(witnesses)
}

fn observe(
    function: &Function,
    source: &str,
    cases: &[(CodeValue, CodeValue)],
    max_steps: usize,
    deadline: Option<Instant>,
    remaining: &mut usize,
) -> Result<(CodeVerificationReport, Vec<Vec<BranchObservation>>)> {
    deadline_check(deadline)?;
    let mut trace = VerificationTrace {
        remaining_steps: *remaining,
        branches: Vec::with_capacity(cases.len()),
    };
    let report = verify_observing(
        function,
        source,
        cases,
        max_steps,
        deadline,
        Some(&mut trace),
    )?;
    *remaining = trace.remaining_steps;
    if report
        .cases
        .iter()
        .any(|row| matches!(row.evaluation_status.as_str(), "STOPPED" | "NOT_RUN"))
    {
        return Err(budget_error());
    }
    Ok((report, trace.branches))
}

pub(crate) fn repair_source(document: &str, deadline: Option<Instant>) -> Result<CodeRepairReport> {
    deadline_check(deadline)?;
    let input = parse_input(document, CodeOperation::Verify)?;
    let function = PythonParser::parse(&input.source)?;
    let mut remaining = MAX_TOTAL_STEPS;
    let (before, initial_trace) = observe(
        &function,
        &input.source,
        &input.cases,
        input.max_steps,
        deadline,
        &mut remaining,
    )?;
    if before
        .cases
        .iter()
        .any(|row| row.evaluation_status != "COMPUTED")
    {
        return Err(failure(
            "CODE_REPAIR_BASELINE_FAILED",
            "元のコードの試験で値を得られなかったため、比較演算子の境界誤りを確定できませんでした。",
        ));
    }
    if before.all_matched {
        return Err(failure(
            "CODE_REPAIR_NO_MISMATCH",
            "元のコードで指定試験がすべて期待値と一致しました。",
        ));
    }
    let mut accepted = None;
    let mut attempts = 0;
    for guard in comparisons(&input.source, &function, deadline)? {
        deadline_check(deadline)?;
        if before
            .cases
            .iter()
            .filter(|case| !case.matched)
            .any(|case| {
                branch(&initial_trace, case.number, guard.span.line)
                    != Some(guard.boundary_condition)
            })
        {
            continue;
        }
        let Some(inputs) = comparison_inputs(&input.cases, guard.parameter)? else {
            continue;
        };
        let Some(witnesses) = before_witnesses(&guard, &inputs, &before, &initial_trace) else {
            continue;
        };
        attempts += 1;
        if attempts > MAX_CANDIDATES {
            return Err(budget_error());
        }

        let mut corrected = input.source.clone();
        corrected.replace_range(guard.span.start..guard.span.end, guard.after);
        let corrected_function = PythonParser::parse(&corrected)?;
        let (after, corrected_trace) = observe(
            &corrected_function,
            &corrected,
            &input.cases,
            input.max_steps,
            deadline,
            &mut remaining,
        )?;
        if !after.all_matched
            || !witnesses.iter().all(|item| {
                let looked = |index| branch(&corrected_trace, index, guard.span.line);
                looked(item.boundary_case) == Some(item.branch_after)
                    && looked(item.below_case) == Some(item.below_condition)
                    && looked(item.above_case) == Some(item.above_condition)
            })
        {
            continue;
        }
        if accepted.is_some() {
            return Err(failure(
                "CODE_REPAIR_AMBIGUOUS",
                "指定試験を満たす境界の変更箇所が複数あり、一つに特定できませんでした。",
            ));
        }
        accepted = Some((
            CodeRepairEdit {
                start: guard.span.start,
                end: guard.span.end,
                line: guard.span.line,
                utf8_column: guard.span.column,
                before: guard.before,
                after: guard.after,
                parameter: guard.parameter.into(),
                threshold: guard.threshold.to_string(),
            },
            witnesses,
            corrected,
            after,
        ));
    }
    let (edit, witnesses, corrected_source, after) = accepted.ok_or_else(|| {
        failure(
            "CODE_REPAIR_NO_JUSTIFIED_EDIT",
            "不一致を説明できる変更を特定できませんでした。対象の整数引数について、ほかの引数を揃えた境界値とその前後の試験が手掛かりになります。",
        )
    })?;
    deadline_check(deadline)?;
    Ok(CodeRepairReport {
        version: VERSION,
        status: "REVERIFIED",
        function_name: function.name,
        original_source_sha256: digest(input.source.as_bytes()),
        original_source: input.source,
        corrected_source_sha256: digest(corrected_source.as_bytes()),
        corrected_source,
        edit,
        before,
        after,
        witnesses,
        assurance: ASSURANCE,
    })
}

pub(crate) fn repair_lines(report: &CodeRepairReport) -> Result<Vec<String>> {
    let edit = &report.edit;
    let source_line = |source: &str| {
        source
            .lines()
            .nth(edit.line - 1)
            .map(str::to_owned)
            .ok_or_else(|| {
                failure(
                    "CODE_REPAIR_REPORT_INVALID",
                    "差分の行を確認できませんでした。",
                )
            })
    };
    let mut lines = vec![
        format!("関数「{}」の修正案です。", report.function_name),
        format!(
            "{}行目で「{}」が {} に等しいとき、境界を含めるかどうかの判定が指定された期待値と一致していません。",
            edit.line, edit.parameter, edit.threshold
        ),
        format!(
            "比較演算子を「{}」から「{}」に変更します。",
            edit.before, edit.after
        ),
        format!(
            "差分（{}行目）\n-{}\n+{}",
            edit.line,
            source_line(&report.original_source)?,
            source_line(&report.corrected_source)?
        ),
    ];
    for item in &report.witnesses {
        let before = &report.before.cases[item.boundary_case];
        let after = &report.after.cases[item.boundary_case];
        let observed = |condition| if condition { "真" } else { "偽" };
        lines.push(format!(
            "境界の試験{}では条件が{}から{}へ変わり、実際値が {} から {}（期待値 {}）になりました。前後の試験{}と{}も期待値に一致しました。",
            item.boundary_case + 1,
            observed(item.branch_before),
            observed(item.branch_after),
            before.actual_json.as_deref().unwrap_or("値なし"),
            after.actual_json.as_deref().unwrap_or("値なし"),
            before.expected_json,
            item.below_case + 1,
            item.above_case + 1
        ));
    }
    lines.push(format!(
        "修正後の再検証は全{}件が一致しました。",
        report.after.case_count
    ));
    lines.push(report.assurance.into());
    lines.push(format!("修正後のコード\n{}", report.corrected_source));
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn repair_requires_adjacent_observations_with_the_other_arguments_equal() {
        let source = "def price(quantity, rate):\n    if quantity > 10:\n        return rate\n    return 0\n";
        let boundary = json!({"引数":{"quantity":10,"rate":5},"期待値":5});
        let upper = json!({"引数":{"quantity":11,"rate":5},"期待値":5});
        let wrong_context = json!({"引数":{"quantity":9,"rate":4},"期待値":0});
        let distant = json!({"引数":{"quantity":8,"rate":5},"期待値":0});
        for cases in [
            vec![boundary.clone(), upper.clone()],
            vec![wrong_context, boundary.clone(), upper.clone()],
            vec![distant, boundary, upper],
        ] {
            let document = json!({"情報源": source, "試験": cases}).to_string();
            let error = repair_source(&document, None).unwrap_err();
            assert!(
                matches!(
                    &error,
                    minidora_domain::Error::Contract { code, .. }
                        if code == "CODE_REPAIR_NO_JUSTIFIED_EDIT"
                ),
                "{error:?}"
            );
        }
    }

    #[test]
    fn repair_preserves_dictionary_argument_order_in_witnesses() {
        let document = r#"{"情報源":"def fee(x, data):\n    if x > 3:\n        return list(data)\n    return []\n","試験":[{"引数":{"x":2,"data":{"b":2,"a":1}},"期待値":[]},{"引数":{"data":{"a":1,"b":2},"x":3},"期待値":["a","b"]},{"引数":{"data":{"b":2,"a":1},"x":4},"期待値":["b","a"]}]}"#;
        let error = repair_source(document, None).unwrap_err();
        assert!(
            matches!(
                error,
                minidora_domain::Error::Contract { ref code, .. }
                    if code == "CODE_REPAIR_NO_JUSTIFIED_EDIT"
            ),
            "{error:?}"
        );

        let same_dictionary_order = document
            .replace(r#"{"b":2,"a":1}"#, r#"{"a":1,"b":2}"#)
            .replace(r#"["b","a"]"#, r#"["a","b"]"#);
        let report = repair_source(&same_dictionary_order, None).unwrap();
        assert!(report.after.all_matched);
    }

    #[test]
    fn repair_preserves_source_bytes_at_a_reversed_negative_boundary() {
        let source = "def 判定(残高):\r\n    if (-3) < 残高:  # 手数料\r\n        return 1\r\n    return 0\r\n";
        let document = json!({
            "情報源": source,
            "試験": [
                {"引数":{"残高":-4},"期待値":0},
                {"引数":{"残高":-3},"期待値":1},
                {"引数":{"残高":-2},"期待値":1},
            ]
        })
        .to_string();
        let report = repair_source(&document, None).unwrap();
        assert_eq!(report.edit.threshold, "-3");
        assert_eq!(report.original_source, source);
        assert_eq!(report.corrected_source, source.replace("<", "<="));
        assert_eq!(source.get(report.edit.start..report.edit.end), Some("<"));
        assert_eq!(report.edit.line, 2);
        assert_eq!(report.before.cases_sha256, report.after.cases_sha256);
        assert_eq!(report.witnesses.len(), 1);
        assert!(!report.witnesses[0].branch_before);
        assert!(report.witnesses[0].branch_after);
        assert!(report.after.all_matched);
    }
}

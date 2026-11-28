//! Bind bounded Japanese function requirements to their original source spans.

use std::sync::LazyLock;

use minidora_domain::{
    FunctionalPythonBody, FunctionalPythonSpec, NumericCase, NumericComparison, NumericListOutput,
    NumericListSelection, NumericOperand, NumericOperation, NumericValue, Result, SourceSpan,
};
use regex::{Captures, Regex};

const MAX_PARAMETERS: usize = 8;
const MAX_CASES: usize = 6;

static LEADING_SIGNATURE: LazyLock<Regex> = LazyLock::new(|| {
    syntax(concat!(
        r"\APythonで[、, 　]*(?P<body>[^\r\n]+?)(?P<verb>を返す|を判定する)関数[ 　]+",
        r"(?P<name>[_\p{XID_Start}][_\p{XID_Continue}]*)[ 　]*\((?P<parameters>[^()\r\n]*)\)",
        r"[ 　]*を(?:作って|作ってください|生成して|生成してください)\z"
    ))
});
static TRAILING_SIGNATURE: LazyLock<Regex> = LazyLock::new(|| {
    syntax(concat!(
        r"\A(?P<body>[^\r\n]+?)(?P<verb>を返す|を判定する)Python関数を",
        r"(?:作って|作ってください|生成して|生成してください)\s*。\s*",
        r"関数名は[ 　]*(?P<name>[_\p{XID_Start}][_\p{XID_Continue}]*)[ 　]*[、,，]",
        r"[ 　]*引数は[ 　]*(?P<parameters>[^()\r\n]+)\z"
    ))
});
static DEFAULT_SIGNATURE: LazyLock<Regex> = LazyLock::new(|| {
    syntax(concat!(
        r"\A(?:Pythonで[、, 　]*(?P<leading>[^\r\n]+?)を判定する関数|",
        r"(?P<trailing>[^\r\n]+?)を判定するPython関数)",
        r"[ 　]*を(?:作って|作ってください|生成して|生成してください)\z"
    ))
});
static DEFAULT_LIST_SIGNATURE: LazyLock<Regex> = LazyLock::new(|| {
    syntax(concat!(
        r"\A(?:Pythonで[、, 　]*(?P<leading>[^\r\n]+?)を返す関数|",
        r"(?P<trailing>[^\r\n]+?)を返すPython関数)",
        r"[ 　]*を(?:作って|作ってください|生成して|生成してください)\z"
    ))
});
static LIST_BOUNDARY: LazyLock<Regex> = LazyLock::new(|| {
    syntax(concat!(
        r"\A(?P<threshold>.+?)[ 　]*(?P<operator>未満|以下|以上|超|より小さい|より大きい|を超える|",
        r"と等しくない|と等しい|に等しくない|に等しい|と同じ|と異なる)\z"
    ))
});
static UNIQUE_LIST: LazyLock<Regex> = LazyLock::new(|| {
    syntax(concat!(
        r"\A(?:重複を(?:除き|除いて|取り除き|取り除いて|削除し)[、,， 　]*",
        r"(?:最初に(?:現れた|出た)順(?:番)?|初出順|入力順|元の順序?)を(?:保った|維持した)",
        r"|(?:初出順|入力順|最初に(?:現れた|出た)順(?:番)?)(?:で|を保って)[、,， 　]*",
        r"重複を(?:除いた|取り除いた|削除した))リスト\z"
    ))
});
static CASE: LazyLock<Regex> = LazyLock::new(|| {
    syntax(concat!(
        r"\A(?:(?P<left>.+?)が[ 　]*)?(?P<right>.+?)[ 　]*",
        r"(?P<operator>未満|以下|以上|超|より小さい|より大きい|を超える|",
        r"と等しくない|と等しい|に等しくない|に等しい|と同じ|と異なる)",
        r"(?:なら(?:ば)?|の(?:場合|とき)(?:は)?)[ 　]*(?P<result>.+)\z"
    ))
});
static CONDITIONAL_CASE: LazyLock<Regex> = LazyLock::new(|| {
    syntax(concat!(
        r"\A(?:(?P<left>.+?)が[ 　]*)?(?P<right>.+?)[ 　]*",
        r"(?P<operator>より小さければ|より大きければ|を超えれば|と等しければ)",
        r"[ 　]*(?P<result>.+)\z"
    ))
});
static OTHERWISE: LazyLock<Regex> = LazyLock::new(|| {
    syntax(r"\A(?:それ以外|これ以外|そのほか|その他)(?:なら(?:ば)?|は)[ 　]*(?P<result>.+)\z")
});
static SCALE: LazyLock<Regex> =
    LazyLock::new(|| syntax(r"\A(?P<left>.+?)の[ 　]*(?P<right>-?[0-9]+)倍\z"));
static ADD_OR_SUBTRACT: LazyLock<Regex> = LazyLock::new(|| {
    syntax(concat!(
        r"\A(?P<left>.+?)(?P<particle>に|から)[ 　]*(?P<right>.+?)を",
        r"(?P<operator>足した|加えた|引いた)(?:数|値|結果)?\z"
    ))
});
static COMBINE: LazyLock<Regex> =
    LazyLock::new(|| syntax(r"\A(?P<left>.+?)と(?P<right>.+?)の(?P<operator>合計|和|積)\z"));

fn syntax(pattern: &str) -> Regex {
    Regex::new(pattern).expect("constant Japanese function grammar")
}

pub(crate) fn parse(text: &str) -> Result<Option<FunctionalPythonSpec>> {
    Ok(parse_specification(text))
}

fn parse_specification(text: &str) -> Option<FunctionalPythonSpec> {
    if text.len() > 4096 || text.contains(['「', '」', '『', '』', '\n', '\r']) {
        return None;
    }
    let statement = text.trim();
    let statement = statement
        .strip_suffix(['。', '.', '！', '!'])
        .unwrap_or(statement);
    let Some(parsed) = LEADING_SIGNATURE
        .captures(statement)
        .or_else(|| TRAILING_SIGNATURE.captures(statement))
    else {
        if let Some(parsed) = DEFAULT_SIGNATURE.captures(statement) {
            let requirement = field(&parsed, "leading").or_else(|| field(&parsed, "trailing"))?;
            return palindrome_requirement(requirement).then_some(FunctionalPythonSpec {
                name: None,
                parameters: Vec::new(),
                body: FunctionalPythonBody::Palindrome {
                    requirement: source_span(text, requirement)?,
                },
            });
        }
        let parsed = DEFAULT_LIST_SIGNATURE.captures(statement)?;
        let requirement = field(&parsed, "leading").or_else(|| field(&parsed, "trailing"))?;
        return Some(FunctionalPythonSpec {
            name: None,
            parameters: Vec::new(),
            body: list_requirement(text, requirement, &[])?,
        });
    };
    let name = source_span(text, field(&parsed, "name")?)?;
    crate::code_generation::validate_python_identifier(&name.text).ok()?;
    let raw_parameters = field(&parsed, "parameters")?;
    let mut parameters = Vec::new();
    for parameter in raw_parameters.split([',', '、', '，']).map(str::trim) {
        if crate::code_generation::validate_python_identifier(parameter).is_err()
            || parameters.len() == MAX_PARAMETERS
            || parameters
                .iter()
                .any(|known: &SourceSpan| known.text == parameter)
        {
            return None;
        }
        parameters.push(source_span(text, parameter)?);
    }

    let requirement = field(&parsed, "body")?;
    let body = match field(&parsed, "verb")? {
        "を判定する" if parameters.len() == 1 && palindrome_requirement(requirement) => {
            FunctionalPythonBody::Palindrome {
                requirement: source_span(text, requirement)?,
            }
        }
        "を返す" => list_requirement(text, requirement, &parameters)
            .or_else(|| piecewise(text, requirement, &parameters))?,
        _ => return None,
    };
    Some(FunctionalPythonSpec {
        name: Some(name),
        parameters,
        body,
    })
}

const LIST_NOUNS: &[&str] = &[
    "整数のリスト",
    "整数リスト",
    "整数の配列",
    "整数配列",
    "数値のリスト",
    "数値リスト",
    "数のリスト",
    "数値の配列",
    "数値配列",
    "リスト",
    "配列",
];

fn list_parameter(subject: &str, parameters: &[SourceSpan]) -> Option<usize> {
    let subject = subject.trim();
    let subject = ["与えられた", "入力された", "入力の", "引数の"]
        .into_iter()
        .find_map(|prefix| subject.strip_prefix(prefix))
        .unwrap_or(subject)
        .trim_start();
    if let Some(index) = parameters
        .iter()
        .position(|parameter| parameter.text == subject)
    {
        return Some(index);
    }
    if LIST_NOUNS.contains(&subject) {
        return (parameters.len() <= 1).then_some(0);
    }
    LIST_NOUNS.iter().find_map(|noun| {
        let name = subject
            .strip_prefix(*noun)
            .or_else(|| subject.strip_suffix(*noun)?.strip_suffix("という"))?
            .trim();
        parameters
            .iter()
            .position(|parameter| parameter.text == name)
    })
}

fn list_output(task: &str) -> Option<(&str, NumericListOutput)> {
    use NumericListOutput::{Count, Keep, Sum};
    [
        ("要素だけを元の順に並べたリスト", Keep),
        ("要素を元の順に並べたリスト", Keep),
        ("要素を元の順序で並べたリスト", Keep),
        ("要素を入力順に並べたリスト", Keep),
        ("要素を元の順序で残したリスト", Keep),
        ("要素を順番を保って集めたリスト", Keep),
        ("だけを元の順に並べたリスト", Keep),
        ("だけを元の順で並べたリスト", Keep),
        ("だけを入力順で残したリスト", Keep),
        ("の要素だけのリスト", Keep),
        ("要素だけのリスト", Keep),
        ("要素のリスト", Keep),
        ("だけのリスト", Keep),
        ("要素の個数", Count),
        ("値の個数", Count),
        ("ものの数", Count),
        ("要素数", Count),
        ("個数", Count),
        ("件数", Count),
        ("要素の合計", Sum),
        ("値の合計", Sum),
        ("合計", Sum),
        ("総和", Sum),
    ]
    .into_iter()
    .find_map(|(suffix, output)| {
        let stem = task.strip_suffix(suffix)?.trim_end();
        let selector = stem.strip_suffix('の').unwrap_or(stem).trim_end();
        Some((selector, output))
    })
}

fn list_selection(
    text: &str,
    selector: &str,
    parameters: &[SourceSpan],
    list_index: usize,
) -> Option<NumericListSelection> {
    let selector = selector.strip_suffix("だけ").unwrap_or(selector).trim();
    let selector = selector.strip_suffix("である").unwrap_or(selector).trim();
    match selector {
        "" | "すべて" | "全て" | "全要素" => return Some(NumericListSelection::All),
        "偶数" => return Some(NumericListSelection::Parity { even: true }),
        "奇数" => return Some(NumericListSelection::Parity { even: false }),
        _ => {}
    }
    let parsed = LIST_BOUNDARY.captures(selector)?;
    let threshold = operand(text, field(&parsed, "threshold")?, parameters)?;
    if matches!(threshold, NumericOperand::Parameter { index, .. } if index == list_index) {
        return None;
    }
    Some(NumericListSelection::Boundary {
        comparison: comparison(field(&parsed, "operator")?)?,
        threshold,
    })
}

fn list_requirement(
    text: &str,
    requirement: &str,
    parameters: &[SourceSpan],
) -> Option<FunctionalPythonBody> {
    for marker in ["のうち", "に含まれる", "から", "の"] {
        for (boundary, _) in requirement.match_indices(marker) {
            let Some(list_index) = list_parameter(&requirement[..boundary], parameters) else {
                continue;
            };
            let tail = requirement[boundary + marker.len()..]
                .trim_start_matches(['、', ',', '，', ' ', '　']);
            let original_requirement = source_span(text, requirement)?;
            if UNIQUE_LIST.is_match(tail) {
                return Some(FunctionalPythonBody::StableUniqueList {
                    requirement: original_requirement,
                    list_index,
                });
            }
            if let Some((selector, output)) = list_output(tail)
                && let Some(selection) = list_selection(text, selector, parameters, list_index)
            {
                return Some(FunctionalPythonBody::NumericList {
                    requirement: original_requirement,
                    list_index,
                    selection,
                    output,
                });
            }
        }
    }
    None
}

fn palindrome_requirement(requirement: &str) -> bool {
    let Some((subject, condition)) = requirement.split_once('が') else {
        return false;
    };
    matches!(
        subject,
        "文字列" | "入力文字列" | "入力の文字列" | "引数の文字列" | "与えられた文字列"
    ) && matches!(
        condition,
        "回文かどうか"
            | "前から読んでも後ろから読んでも同じかどうか"
            | "後ろから読んでも前から読んでも同じかどうか"
            | "逆から読んでも同じかどうか"
    )
}

fn piecewise(
    text: &str,
    requirement: &str,
    parameters: &[SourceSpan],
) -> Option<FunctionalPythonBody> {
    let mut clauses = requirement
        .split(['、', ',', '，'])
        .map(str::trim)
        .peekable();
    let first = clauses.next()?;
    let mut cases = Vec::new();
    if clauses.peek().is_none() {
        return Some(FunctionalPythonBody::Piecewise {
            cases,
            otherwise: numeric_value(text, first, parameters)?,
        });
    }
    cases.push(numeric_case(text, first, parameters)?);
    while let Some(clause) = clauses.next() {
        if clauses.peek().is_none() {
            let parsed = OTHERWISE.captures(clause)?;
            return Some(FunctionalPythonBody::Piecewise {
                cases,
                otherwise: numeric_value(text, field(&parsed, "result")?, parameters)?,
            });
        }
        if cases.len() == MAX_CASES {
            return None;
        }
        cases.push(numeric_case(text, clause, parameters)?);
    }
    None
}

fn numeric_case(text: &str, clause: &str, parameters: &[SourceSpan]) -> Option<NumericCase> {
    let parsed = CASE
        .captures(clause)
        .or_else(|| CONDITIONAL_CASE.captures(clause))?;
    let left = if let Some(left) = field(&parsed, "left") {
        operand(text, left, parameters)?
    } else {
        let index = named_role(
            parameters,
            &["value", "x", "number", "input", "num", "値", "数"],
        )?;
        NumericOperand::Parameter {
            source: parameters[index].clone(),
            index,
        }
    };
    let right = operand(text, field(&parsed, "right")?, parameters)?;
    Some(NumericCase {
        left,
        comparison: comparison(field(&parsed, "operator")?)?,
        right,
        result: numeric_value(text, field(&parsed, "result")?, parameters)?,
    })
}

fn comparison(operator: &str) -> Option<NumericComparison> {
    Some(match operator {
        "未満" | "より小さい" | "より小さければ" => NumericComparison::Less,
        "以下" => NumericComparison::LessEqual,
        "超" | "を超える" | "を超えれば" | "より大きい" | "より大きければ" => {
            NumericComparison::Greater
        }
        "以上" => NumericComparison::GreaterEqual,
        "と等しい" | "に等しい" | "と同じ" | "と等しければ" => {
            NumericComparison::Equal
        }
        "と等しくない" | "に等しくない" | "と異なる" => NumericComparison::NotEqual,
        _ => return None,
    })
}

fn numeric_value(text: &str, value: &str, parameters: &[SourceSpan]) -> Option<NumericValue> {
    if let Some(value) = operand(text, value, parameters) {
        return Some(NumericValue::Operand { value });
    }
    let (parsed, operation) = if let Some(parsed) = SCALE.captures(value) {
        (parsed, NumericOperation::Multiply)
    } else if let Some(parsed) = ADD_OR_SUBTRACT.captures(value) {
        let operation = match (field(&parsed, "particle")?, field(&parsed, "operator")?) {
            ("に", "足した" | "加えた") => NumericOperation::Add,
            ("から", "引いた") => NumericOperation::Subtract,
            _ => return None,
        };
        (parsed, operation)
    } else {
        let parsed = COMBINE.captures(value)?;
        let operation = match field(&parsed, "operator")? {
            "合計" | "和" => NumericOperation::Add,
            "積" => NumericOperation::Multiply,
            _ => return None,
        };
        (parsed, operation)
    };
    Some(NumericValue::Arithmetic {
        left: operand(text, field(&parsed, "left")?, parameters)?,
        operation,
        right: operand(text, field(&parsed, "right")?, parameters)?,
    })
}

fn operand(text: &str, candidate: &str, parameters: &[SourceSpan]) -> Option<NumericOperand> {
    let candidate = candidate.trim();
    let source = source_span(text, candidate)?;
    if let Ok(value) = candidate.parse::<i64>() {
        return Some(NumericOperand::Integer { source, value });
    }
    let index = if let Some(index) = parameters.iter().position(|param| param.text == candidate) {
        index
    } else {
        match candidate {
            "下限" => named_role(parameters, &["low", "lower", "min_value", "lower_bound"]),
            "上限" => named_role(parameters, &["high", "upper", "max_value", "upper_bound"]),
            "元の数" | "元の値" | "入力値" | "値" | "数" if parameters.len() == 1 => {
                Some(0)
            }
            "元の数" | "元の値" | "入力値" | "値" | "数" => named_role(
                parameters,
                &["value", "x", "number", "input", "num", "値", "数"],
            ),
            _ => None,
        }?
    };
    Some(NumericOperand::Parameter { source, index })
}

fn named_role(parameters: &[SourceSpan], names: &[&str]) -> Option<usize> {
    let mut matches = parameters
        .iter()
        .enumerate()
        .filter(|(_, param)| names.contains(&param.text.as_str()));
    let (index, _) = matches.next()?;
    matches.next().is_none().then_some(index)
}

fn field<'a>(parsed: &Captures<'a>, name: &str) -> Option<&'a str> {
    Some(parsed.name(name)?.as_str().trim())
}

fn source_span(text: &str, slice: &str) -> Option<SourceSpan> {
    let start = (slice.as_ptr() as usize).checked_sub(text.as_ptr() as usize)?;
    crate::input::source_span_at(text, start, start.checked_add(slice.len())?)
}

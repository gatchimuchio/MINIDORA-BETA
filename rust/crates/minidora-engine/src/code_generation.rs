//! Generate a bounded Python function from the inherited explicit algorithm IR.
//! No source evaluation, host process, external effect or test oracle is used.
use crate::ports::{JsonKind, JsonNode, strict_json_document};
use minidora_domain::{Result, SourceSpan, digest, failure};
use num_bigint::BigInt;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeSet;
use std::str::FromStr;
use std::sync::OnceLock;
use unicode_normalization::UnicodeNormalization;

const VERSION: &str = "structured-python-generator-v1-ucd15";
const PREAMBLE: &str = "# Python 3.12; execution and input/output tests: NOT_RUN.\n";
const BUILTINS: &[&str] = &[
    "sum", "len", "range", "abs", "min", "max", "sorted", "list", "all", "any",
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeGenerationReport {
    pub version: String,
    pub target_runtime: String,
    pub source_document: String,
    pub source_document_sha256: String,
    pub specification: SourceSpan,
    pub constants: SourceSpan,
    pub function_name: String,
    pub parameters: Vec<String>,
    pub source: String,
    pub source_sha256: String,
    pub ir_nodes: usize,
    pub constructed_ast_nodes: usize,
    pub constructed_ast_depth: usize,
    pub validation: String,
    pub python_parse: String,
    pub execution: String,
    pub input_output_tests: String,
}

fn invalid(message: impl Into<String>) -> minidora_domain::Error {
    failure("CODE_GENERATION_INPUT", message)
}

fn identifier(value: &Value) -> Result<&str> {
    static IDENTIFIER: OnceLock<Regex> = OnceLock::new();
    let name = value
        .as_str()
        .ok_or_else(|| invalid("identifier must be a string"))?;
    // Unicode 16 XID intersected with Age <=15, excluding the four continuation
    // additions after Python 3.12's Unicode 15.0 contract. No new table/dependency.
    let pattern = IDENTIFIER.get_or_init(|| Regex::new(
        r"\A(?:_|[\p{XID_Start}&&\p{Age:15.0}])[\p{XID_Continue}&&\p{Age:15.0}&&[^\u{200c}\u{200d}\u{30fb}\u{ff65}]]*\z"
    ).expect("versioned Python identifier pattern is static"));
    let keyword = matches!(
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
    );
    if name.chars().count() > 80
        || !pattern.is_match(name)
        || keyword
        || name.starts_with("__")
        || name == "定数"
        || BUILTINS.contains(&name)
        || name.nfkc().collect::<String>() != name
    {
        return Err(invalid("unsupported, reserved or noncanonical identifier"));
    }
    Ok(name)
}

struct ValueBudget {
    nodes: usize,
    text_bytes: usize,
}
impl ValueBudget {
    fn string(&mut self, text: &str, depth: usize) -> Result<()> {
        self.node(depth)?;
        self.text_bytes = self.text_bytes.saturating_add(text.len());
        if text.chars().count() > 8192 || self.text_bytes > 65_536 {
            return Err(invalid("constant text budget exceeded"));
        }
        Ok(())
    }
    fn node(&mut self, depth: usize) -> Result<()> {
        self.nodes += 1;
        if self.nodes > 4096 || depth > 16 {
            return Err(invalid("constant value node/depth budget exceeded"));
        }
        Ok(())
    }
    fn visit(&mut self, document: &str, value: &JsonNode, depth: usize) -> Result<()> {
        if matches!(value.kind, JsonKind::Scalar) && document[value.span.clone()].starts_with('"') {
            return self.string(
                &serde_json::from_str::<String>(&document[value.span.clone()])?,
                depth,
            );
        }
        self.node(depth)?;
        match &value.kind {
            JsonKind::Object(entries) => {
                if entries.len() > 2048 {
                    return Err(invalid("constant dictionary size exceeded"));
                }
                for (key, child) in entries {
                    self.string(key, depth + 1)?;
                    self.visit(document, child, depth + 1)?;
                }
            }
            JsonKind::Array(entries) => {
                if entries.len() > 2048 {
                    return Err(invalid("constant list size exceeded"));
                }
                for child in entries {
                    self.visit(document, child, depth + 1)?;
                }
            }
            JsonKind::Number => {
                let number = &document[value.span.clone()];
                if number.contains(['.', 'e', 'E'])
                    || number.trim_start_matches('-').len() > 78
                    || BigInt::from_str(number)
                        .map_err(|_| invalid("invalid integer constant"))?
                        .bits()
                        > 256
                {
                    return Err(invalid(
                        "constants require integers of at most 256 bits; floats are unsupported",
                    ));
                }
            }
            JsonKind::Scalar => {}
        }
        Ok(())
    }
}

struct Piece {
    text: String,
    nodes: usize,
    height: usize,
}
impl Piece {
    fn leaf(text: String, nodes: usize, height: usize) -> Self {
        Self {
            text,
            nodes,
            height,
        }
    }
    fn combine(text: String, own_nodes: usize, minimum_height: usize, children: &[Self]) -> Self {
        Self {
            text,
            nodes: own_nodes + children.iter().map(|p| p.nodes).sum::<usize>(),
            height: minimum_height.max(children.iter().map(|p| p.height + 1).max().unwrap_or(0)),
        }
    }
}

struct Generator<'a> {
    constants: &'a BTreeSet<String>,
    nodes: usize,
}
impl Generator<'_> {
    fn shape<'a>(&mut self, node: &'a Value, keys: &[&str]) -> Result<&'a Map<String, Value>> {
        self.nodes += 1;
        let object = node
            .as_object()
            .ok_or_else(|| invalid("IR node must be an object"))?;
        if self.nodes > 512
            || object.len() != keys.len()
            || keys.iter().any(|k| !object.contains_key(*k))
        {
            return Err(invalid("IR shape/node limit or unknown field"));
        }
        Ok(object)
    }
    fn expression(&mut self, node: &Value, depth: usize) -> Result<Piece> {
        if depth > 24 {
            return Err(invalid("IR expression depth exceeded"));
        }
        let kind = node
            .get("種別")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("missing expression kind"))?;
        match kind {
            "参照" => {
                self.shape(node, &["種別", "名前"])?;
                Ok(Piece::leaf(identifier(&node["名前"])?.into(), 2, 1))
            }
            "定数参照" => {
                self.shape(node, &["種別", "キー"])?;
                let key = node["キー"]
                    .as_str()
                    .ok_or_else(|| invalid("constant key must be a string"))?;
                if !self.constants.contains(key) {
                    return Err(invalid("required constant is missing"));
                }
                Ok(Piece::leaf(
                    format!("定数[{}]", serde_json::to_string(key)?),
                    5,
                    2,
                ))
            }
            "算術" | "比較" => {
                self.shape(node, &["種別", "演算", "左", "右"])?;
                let operation = match (kind, node["演算"].as_str()) {
                    ("算術", Some("加算")) => "+",
                    ("算術", Some("減算")) => "-",
                    ("算術", Some("乗算")) => "*",
                    ("算術", Some("整数商")) => "//",
                    ("算術", Some("剰余")) => "%",
                    ("比較", Some("等値")) => "==",
                    ("比較", Some("非等値")) => "!=",
                    ("比較", Some("未満")) => "<",
                    ("比較", Some("以下")) => "<=",
                    ("比較", Some("超")) => ">",
                    ("比較", Some("以上")) => ">=",
                    _ => return Err(invalid("unsupported operator")),
                };
                let a = self.expression(&node["左"], depth + 1)?;
                let b = self.expression(&node["右"], depth + 1)?;
                Ok(Piece::combine(
                    format!("({} {operation} {})", a.text, b.text),
                    2,
                    1,
                    &[a, b],
                ))
            }
            "選択" => {
                self.shape(node, &["種別", "条件", "真", "偽"])?;
                let condition = self.expression(&node["条件"], depth + 1)?;
                let a = self.expression(&node["真"], depth + 1)?;
                let b = self.expression(&node["偽"], depth + 1)?;
                Ok(Piece::combine(
                    format!("({} if {} else {})", a.text, condition.text, b.text),
                    1,
                    0,
                    &[condition, a, b],
                ))
            }
            "リスト" | "呼出" => {
                let (field, function, own_nodes, height) = if kind == "リスト" {
                    self.shape(node, &["種別", "要素"])?;
                    ("要素", None, 2, 1)
                } else {
                    self.shape(node, &["種別", "関数", "引数"])?;
                    let function = match node["関数"].as_str() {
                        Some("合計") => "sum",
                        Some("長さ") => "len",
                        Some("範囲") => "range",
                        Some("絶対値") => "abs",
                        Some("最小") => "min",
                        Some("最大") => "max",
                        Some("整列") => "sorted",
                        Some("リスト化") => "list",
                        Some("すべて") => "all",
                        Some("いずれか") => "any",
                        _ => return Err(invalid("unsupported function")),
                    };
                    ("引数", Some(function), 3, 2)
                };
                let children = node[field]
                    .as_array()
                    .ok_or_else(|| invalid("IR operands must be an array"))?
                    .iter()
                    .map(|n| self.expression(n, depth + 1))
                    .collect::<Result<Vec<_>>>()?;
                let args = children
                    .iter()
                    .map(|p| p.text.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                let text = function.map_or_else(|| format!("[{args}]"), |f| format!("{f}({args})"));
                Ok(Piece::combine(text, own_nodes, height, &children))
            }
            "添字" => {
                self.shape(node, &["種別", "対象", "位置"])?;
                let a = self.expression(&node["対象"], depth + 1)?;
                let b = self.expression(&node["位置"], depth + 1)?;
                Ok(Piece::combine(
                    format!("({})[{}]", a.text, b.text),
                    2,
                    1,
                    &[a, b],
                ))
            }
            _ => Err(invalid("unsupported algorithm expression")),
        }
    }
    fn block(&mut self, rows: &Value, depth: usize) -> Result<Vec<Piece>> {
        let rows = rows
            .as_array()
            .ok_or_else(|| invalid("IR block must be an array"))?;
        if depth > 16 || rows.len() > 128 {
            return Err(invalid("IR block depth/size exceeded"));
        }
        let indentation = "    ".repeat(depth + 1);
        let mut pieces = Vec::new();
        for node in rows {
            let kind = node
                .get("種別")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid("missing statement kind"))?;
            let piece = match kind {
                "代入" | "返却" => {
                    let (prefix, nodes, height) = if kind == "代入" {
                        self.shape(node, &["種別", "名前", "式"])?;
                        (format!("{} = ", identifier(&node["名前"])?), 3, 2)
                    } else {
                        self.shape(node, &["種別", "式"])?;
                        ("return ".into(), 1, 0)
                    };
                    let expression = self.expression(&node["式"], 0)?;
                    Piece::combine(
                        format!("{indentation}{prefix}{}\n", expression.text),
                        nodes,
                        height,
                        &[expression],
                    )
                }
                "分岐" | "反復" => {
                    let (head, own_nodes, mut children) = if kind == "分岐" {
                        self.shape(node, &["種別", "条件", "真", "偽"])?;
                        let expression = self.expression(&node["条件"], 0)?;
                        (format!("if {}:", expression.text), 1, vec![expression])
                    } else {
                        self.shape(node, &["種別", "変数", "対象", "手順"])?;
                        let variable = identifier(&node["変数"])?;
                        let expression = self.expression(&node["対象"], 0)?;
                        (
                            format!("for {variable} in {}:", expression.text),
                            3,
                            vec![expression],
                        )
                    };
                    let body =
                        self.block(&node[if kind == "分岐" { "真" } else { "手順" }], depth + 1)?;
                    let mut text = format!(
                        "{indentation}{head}\n{}",
                        body.iter().map(|p| p.text.as_str()).collect::<String>()
                    );
                    children.extend(body);
                    if kind == "分岐" {
                        let other = self.block(&node["偽"], depth + 1)?;
                        text.push_str(&format!(
                            "{indentation}else:\n{}",
                            other.iter().map(|p| p.text.as_str()).collect::<String>()
                        ));
                        children.extend(other);
                    }
                    Piece::combine(
                        text,
                        own_nodes,
                        if kind == "反復" { 2 } else { 0 },
                        &children,
                    )
                }
                _ => return Err(invalid("unsupported algorithm statement")),
            };
            pieces.push(piece);
        }
        if pieces.is_empty() {
            pieces.push(Piece::leaf(format!("{indentation}pass\n"), 1, 0));
        }
        Ok(pieces)
    }
}

pub fn generate(document: &str) -> Result<CodeGenerationReport> {
    if document.len() > 256_000 {
        return Err(invalid("structured code document exceeds 256000 bytes"));
    }
    let root = strict_json_document(document)?;
    let JsonKind::Object(fields) = &root.kind else {
        return Err(invalid("expected specification/constants object"));
    };
    if fields.len() != 2 || !fields.contains_key("仕様") || !fields.contains_key("定数") {
        return Err(invalid("exactly 仕様 and 定数 fields are required"));
    }
    let JsonKind::Object(constants) = &fields["定数"].kind else {
        return Err(invalid("constants must be a dictionary"));
    };
    ValueBudget {
        nodes: 0,
        text_bytes: 0,
    }
    .visit(document, &fields["定数"], 0)?;
    let specification: Value = serde_json::from_str(&document[fields["仕様"].span.clone()])?;
    let constants: BTreeSet<_> = constants.keys().cloned().collect();
    let mut generator = Generator {
        constants: &constants,
        nodes: 0,
    };
    generator.shape(&specification, &["名前", "引数", "手順"])?;
    let name = identifier(&specification["名前"])?;
    let parameters = specification["引数"]
        .as_array()
        .ok_or_else(|| invalid("parameters must be an array"))?
        .iter()
        .map(|v| identifier(v).map(str::to_string))
        .collect::<Result<Vec<_>>>()?;
    if parameters.len() > 15 || parameters.iter().collect::<BTreeSet<_>>().len() != parameters.len()
    {
        return Err(invalid("at most 15 unique user parameters are supported"));
    }
    let body = generator.block(&specification["手順"], 0)?;
    let ast_nodes = 4 + parameters.len() + body.iter().map(|p| p.nodes).sum::<usize>();
    let ast_depth = 3.max(2 + body.iter().map(|p| p.height).max().unwrap_or(0));
    if ast_nodes > 2048 || ast_depth > 40 {
        return Err(invalid("constructed AST node/depth budget exceeded"));
    }
    let mut all_parameters = parameters;
    all_parameters.push("定数".into());
    let source = format!(
        "{PREAMBLE}def {name}({}):\n{}",
        all_parameters.join(", "),
        body.iter().map(|p| p.text.as_str()).collect::<String>()
    );
    if source.len() > 32768 {
        return Err(invalid("generated source exceeds 32768 bytes"));
    }
    let span = |key: &str| {
        let range = fields[key].span.clone();
        SourceSpan {
            start: range.start,
            end: range.end,
            text: document[range].into(),
        }
    };
    Ok(CodeGenerationReport {
        version: VERSION.into(),
        target_runtime: "Python 3.12 / Unicode 15.0.0 identifiers".into(),
        source_document: document.into(),
        source_document_sha256: digest(document.as_bytes()),
        specification: span("仕様"),
        constants: span("定数"),
        function_name: name.into(),
        parameters: all_parameters,
        source_sha256: digest(source.as_bytes()),
        source,
        ir_nodes: generator.nodes,
        constructed_ast_nodes: ast_nodes,
        constructed_ast_depth: ast_depth,
        validation: "EXPLICIT_IR_STRUCTURE_AND_CONSTRUCTED_AST_BOUNDS".into(),
        python_parse: "NOT_RUN".into(),
        execution: "NOT_RUN".into(),
        input_output_tests: "NOT_RUN".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structured_code_generation_identifier_contract() {
        for name in [
            "処理",
            "データ",
            "変数_2",
            "αβ",
            "a\u{301}",
            "_",
            "case",
            "match",
        ] {
            let value = Value::String(name.into());
            let result = identifier(&value);
            if name.nfkc().collect::<String>() == name {
                assert_eq!(result.unwrap(), name);
            } else {
                assert!(result.is_err());
            }
        }
        for name in [
            "",
            "def",
            "True",
            "定数",
            "sum",
            "__x",
            "Ａ",
            "a b",
            "a\nreturn 1",
            "a\u{200c}",
            "a\u{200d}",
            "a・b",
            "a･b",
            "a\u{1c89}",
        ] {
            assert!(identifier(&Value::String(name.into())).is_err(), "{name:?}");
        }
        assert!(identifier(&Value::String("x".repeat(81))).is_err());
        assert!(identifier(&Value::String("x".repeat(80))).is_ok());
    }

    #[test]
    fn structured_code_generation_preserves_exact_constant_lexemes() {
        let spec = r#"{"名前":"処理","引数":[],"手順":[{"種別":"返却","式":{"種別":"定数参照","キー":"値"}}]}"#;
        let maximum = (BigInt::from(1u8) << 256usize) - 1u8;
        for number in [maximum.to_string(), format!("-{maximum}"), "-0".into()] {
            let constants =
                format!(r#"{{"値":{number},"資料":[true,null,"猫\n説明",{{"x":false}}]}}"#);
            let document = format!(r#"{{"仕様":{spec},"定数":{constants}}}"#);
            let report = generate(&document).unwrap();
            assert_eq!(report.constants.text, constants);
            assert_eq!(
                &document[report.constants.start..report.constants.end],
                constants
            );
            assert!(report.source.contains("return 定数[\"値\"]"));
            assert!(!report.source.contains(&number));
            assert_eq!(report.python_parse, "NOT_RUN");
            assert_eq!(report.execution, "NOT_RUN");
            assert_eq!(report.input_output_tests, "NOT_RUN");
        }
        for invalid in [
            "1.0".to_string(),
            "1e0".into(),
            (BigInt::from(1u8) << 256usize).to_string(),
        ] {
            assert!(generate(&format!(r#"{{"仕様":{spec},"定数":{{"値":{invalid}}}}}"#)).is_err());
        }
    }
}

use std::fmt::{self, Write};

use rustpython_vm::{
    PyObjectRef, PyResult, TryFromObject, VirtualMachine,
    builtins::{PyBaseExceptionRef, PyTypeRef, PyUtf8StrRef},
};
use serde_json::{Map, Number, Value};
use yaml_rust2::{
    Event, Yaml, YamlEmitter, YamlLoader,
    parser::Parser,
    scanner::{Scanner, TScalarStyle, TokenType},
};

use crate::{
    limits::{MAX_DEPTH, MAX_NODES, MAX_SAFE_INTEGER, YAML_BYTES},
    value::{py_to_safe_json, safe_json_to_py, safe_json_to_yaml},
};

#[rustpython_vm::pymodule(name = "yaml")]
pub(crate) mod yaml_module {
    use super::*;

    #[pyattr(name = "YAMLError", once)]
    fn error(vm: &VirtualMachine) -> PyTypeRef {
        vm.ctx.new_exception_type(
            "yaml",
            "YAMLError",
            Some(vec![vm.ctx.exceptions.value_error.to_owned()]),
        )
    }

    #[pyfunction]
    fn safe_load(source: PyUtf8StrRef, vm: &VirtualMachine) -> PyResult {
        load_yaml(source.as_str())
            .map(|value| safe_json_to_py(&value, vm))
            .map_err(|message| exception(vm, message))
    }

    #[pyfunction]
    fn safe_dump(value: PyObjectRef, vm: &VirtualMachine) -> PyResult<String> {
        let safe = py_to_safe_json(&value, vm)
            .map_err(|error| exception(vm, error.message().to_owned()))?;
        dump_yaml(&safe).map_err(|message| exception(vm, message))
    }
}

fn exception(vm: &VirtualMachine, message: String) -> PyBaseExceptionRef {
    let error_type = vm
        .sys_module
        .get_attr("modules", vm)
        .and_then(|modules| modules.get_item("yaml", vm))
        .and_then(|module| module.get_attr("YAMLError", vm))
        .and_then(|class| PyTypeRef::try_from_object(vm, class));
    match error_type {
        Ok(error_type) => vm.new_exception_msg(error_type, message.into()),
        Err(_error) => vm.new_value_error(message),
    }
}

pub(crate) fn load_yaml(source: &str) -> Result<Value, String> {
    if source.len() > YAML_BYTES {
        return Err(format!("YAML input exceeds {YAML_BYTES} UTF-8 bytes"));
    }
    scan_policy(source)?;
    event_policy(source)?;
    let documents = YamlLoader::load_from_str(source).map_err(|error| {
        format!(
            "invalid YAML at line {} column {}: {}",
            error.marker().line(),
            error.marker().col() + 1,
            error.info()
        )
    })?;
    if documents.len() != 1 {
        return Err("YAML input must contain exactly one document".to_owned());
    }
    let mut nodes = 0usize;
    yaml_to_json(
        documents
            .first()
            .ok_or_else(|| "YAML input must contain exactly one document".to_owned())?,
        1,
        &mut nodes,
    )
}

fn scan_policy(source: &str) -> Result<(), String> {
    let mut scanner = Scanner::new(source.chars());
    loop {
        let token = scanner.next_token().map_err(|error| {
            format!(
                "invalid YAML at line {} column {}: {}",
                error.marker().line(),
                error.marker().col() + 1,
                error.info()
            )
        })?;
        let Some(token) = token else {
            break;
        };
        match token.1 {
            TokenType::VersionDirective(..) | TokenType::TagDirective(..) => {
                return Err("YAML directives are not permitted".to_owned());
            }
            TokenType::Alias(_) => return Err("YAML aliases are not permitted".to_owned()),
            TokenType::Anchor(_) => return Err("YAML anchors are not permitted".to_owned()),
            TokenType::Tag(..) => return Err("YAML tags are not permitted".to_owned()),
            TokenType::StreamEnd => break,
            _ => {}
        }
    }
    Ok(())
}

fn event_policy(source: &str) -> Result<(), String> {
    let mut parser = Parser::new_from_str(source);
    let mut documents = 0usize;
    let mut depth = 0usize;
    let mut nodes = 0usize;
    loop {
        let (event, marker) = parser.next_token().map_err(|error| {
            format!(
                "invalid YAML at line {} column {}: {}",
                error.marker().line(),
                error.marker().col() + 1,
                error.info()
            )
        })?;
        match event {
            Event::StreamStart | Event::Nothing => {}
            Event::StreamEnd => break,
            Event::DocumentStart => {
                documents += 1;
                if documents > 1 {
                    return Err("multiple YAML documents are not permitted".to_owned());
                }
            }
            Event::DocumentEnd => {}
            Event::Alias(_) => return Err("YAML aliases are not permitted".to_owned()),
            Event::Scalar(value, style, anchor, tag) => {
                if anchor != 0 {
                    return Err("YAML anchors are not permitted".to_owned());
                }
                if tag.is_some() {
                    return Err("YAML tags are not permitted".to_owned());
                }
                visit_node(&mut nodes, depth.saturating_add(1))?;
                if style == TScalarStyle::Plain && looks_like_integer(&value) {
                    validate_integer(&value)?;
                }
            }
            Event::SequenceStart(anchor, tag) | Event::MappingStart(anchor, tag) => {
                if anchor != 0 {
                    return Err("YAML anchors are not permitted".to_owned());
                }
                if tag.is_some() {
                    return Err("YAML tags are not permitted".to_owned());
                }
                depth += 1;
                visit_node(&mut nodes, depth)?;
            }
            Event::SequenceEnd | Event::MappingEnd => {
                depth = depth.checked_sub(1).ok_or_else(|| {
                    format!(
                        "invalid YAML structure at line {} column {}",
                        marker.line(),
                        marker.col() + 1
                    )
                })?;
            }
        }
    }
    if documents != 1 {
        return Err("YAML input must contain exactly one document".to_owned());
    }
    if depth != 0 {
        return Err("invalid YAML structure".to_owned());
    }
    Ok(())
}

fn visit_node(nodes: &mut usize, depth: usize) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Err(format!("YAML exceeds maximum depth of {MAX_DEPTH}"));
    }
    *nodes = nodes
        .checked_add(1)
        .ok_or_else(|| "YAML node count overflowed".to_owned())?;
    if *nodes > MAX_NODES {
        return Err(format!("YAML exceeds maximum node count of {MAX_NODES}"));
    }
    Ok(())
}

fn looks_like_integer(value: &str) -> bool {
    let unsigned = value.strip_prefix(['+', '-']).unwrap_or(value);
    if let Some(hex) = unsigned.strip_prefix("0x") {
        return !hex.is_empty() && hex.bytes().all(|byte| byte.is_ascii_hexdigit());
    }
    if let Some(octal) = unsigned.strip_prefix("0o") {
        return !octal.is_empty() && octal.bytes().all(|byte| matches!(byte, b'0'..=b'7'));
    }
    !unsigned.is_empty() && unsigned.bytes().all(|byte| byte.is_ascii_digit())
}

fn validate_integer(value: &str) -> Result<(), String> {
    let (negative, unsigned) = value.strip_prefix('-').map_or_else(
        || (false, value.strip_prefix('+').unwrap_or(value)),
        |value| (true, value),
    );
    let magnitude = if let Some(hex) = unsigned.strip_prefix("0x") {
        u128::from_str_radix(hex, 16)
    } else if let Some(octal) = unsigned.strip_prefix("0o") {
        u128::from_str_radix(octal, 8)
    } else {
        unsigned.parse::<u128>()
    }
    .map_err(|_| "YAML integer is out of range".to_owned())?;
    let limit = MAX_SAFE_INTEGER as u128;
    if magnitude > limit {
        return Err(format!(
            "YAML integer must be between -{MAX_SAFE_INTEGER} and {MAX_SAFE_INTEGER}"
        ));
    }
    let _negative = negative;
    Ok(())
}

fn yaml_to_json(value: &Yaml, depth: usize, nodes: &mut usize) -> Result<Value, String> {
    visit_node(nodes, depth)?;
    match value {
        Yaml::Null | Yaml::BadValue => Ok(Value::Null),
        Yaml::Boolean(value) => Ok(Value::Bool(*value)),
        Yaml::Integer(value) => {
            if !(-MAX_SAFE_INTEGER..=MAX_SAFE_INTEGER).contains(value) {
                return Err(format!(
                    "YAML integer must be between -{MAX_SAFE_INTEGER} and {MAX_SAFE_INTEGER}"
                ));
            }
            Ok(Value::Number(Number::from(*value)))
        }
        Yaml::Real(value) => {
            let lowercase = value.to_ascii_lowercase();
            if lowercase.contains("nan") || lowercase.contains("inf") {
                return Err("non-finite YAML numbers are not permitted".to_owned());
            }
            let value = value
                .parse::<f64>()
                .map_err(|_| "YAML number is invalid".to_owned())?;
            let value = Number::from_f64(value)
                .ok_or_else(|| "non-finite YAML numbers are not permitted".to_owned())?;
            Ok(Value::Number(value))
        }
        Yaml::String(value) => Ok(Value::String(value.clone())),
        Yaml::Array(values) => values
            .iter()
            .map(|value| yaml_to_json(value, depth + 1, nodes))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        Yaml::Hash(values) => {
            let mut output = Map::new();
            for (key, value) in values {
                *nodes = nodes
                    .checked_add(1)
                    .ok_or_else(|| "YAML node count overflowed".to_owned())?;
                if *nodes > MAX_NODES {
                    return Err(format!("YAML exceeds maximum node count of {MAX_NODES}"));
                }
                let Yaml::String(key) = key else {
                    return Err("YAML mapping keys must be strings".to_owned());
                };
                if key == "<<" {
                    return Err("YAML merge keys are not permitted".to_owned());
                }
                if output
                    .insert(key.clone(), yaml_to_json(value, depth + 1, nodes)?)
                    .is_some()
                {
                    return Err("duplicate YAML mapping keys are not permitted".to_owned());
                }
            }
            Ok(Value::Object(output))
        }
        Yaml::Alias(_) => Err("YAML aliases are not permitted".to_owned()),
    }
}

pub(crate) fn dump_yaml(value: &Value) -> Result<String, String> {
    let yaml = safe_json_to_yaml(value);
    let mut writer = BoundedWriter::new(YAML_BYTES);
    let emitted = YamlEmitter::new(&mut writer).dump(&yaml);
    if writer.overflowed {
        return Err(format!("YAML output exceeds {YAML_BYTES} UTF-8 bytes"));
    }
    emitted.map_err(|_error| "YAML output could not be encoded".to_owned())?;
    Ok(writer.output)
}

struct BoundedWriter {
    output: String,
    limit: usize,
    overflowed: bool,
}

impl BoundedWriter {
    fn new(limit: usize) -> Self {
        Self {
            output: String::new(),
            limit,
            overflowed: false,
        }
    }
}

impl Write for BoundedWriter {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        if self
            .output
            .len()
            .checked_add(value.len())
            .is_none_or(|length| length > self.limit)
        {
            self.overflowed = true;
            return Err(fmt::Error);
        }
        self.output.push_str(value);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{dump_yaml, load_yaml};
    use crate::limits::{MAX_DEPTH, MAX_SAFE_INTEGER, YAML_BYTES};

    #[test]
    fn loads_the_constrained_value_model_and_preserves_timestamp_text() {
        let value = load_yaml("name: agent\nwhen: 2025-02-03\nitems: [true, null, 2.5]")
            .expect("safe document");
        assert_eq!(
            value,
            json!({"name":"agent","when":"2025-02-03","items":[true,null,2.5]})
        );
    }

    #[test]
    fn rejects_yaml_expansion_and_type_surfaces() {
        let cases = [
            ("%YAML 1.2\n---\na: b", "directives"),
            ("a: &x [1]\nb: *x", "anchors"),
            ("a: *x", "aliases"),
            ("a: !thing b", "tags"),
            ("a: 1\na: 2", "duplicated"),
            ("<<: value", "merge"),
            ("? [a, b]\n: value", "keys"),
            ("---\na: b\n---\nc: d", "multiple"),
            ("a: .nan", "non-finite"),
            (&format!("a: {}", MAX_SAFE_INTEGER as u128 + 1), "integer"),
        ];
        for (source, expected) in cases {
            let error = load_yaml(source).expect_err(source);
            assert!(error.contains(expected), "{source:?}: {error}");
        }
    }

    #[test]
    fn enforces_depth_and_output_bytes() {
        let mut source = String::new();
        for _ in 0..MAX_DEPTH {
            source.push_str("- ");
        }
        source.push_str("null");
        assert!(load_yaml(&source).expect_err("too deep").contains("depth"));

        let oversized = json!({"text": "x".repeat(YAML_BYTES)});
        assert!(
            dump_yaml(&oversized)
                .expect_err("too large")
                .contains("output")
        );
    }

    #[test]
    fn safe_dump_round_trips() {
        let value = json!({"alpha":[1, 2.5, true, null], "text":"hello"});
        let encoded = dump_yaml(&value).expect("dump");
        assert_eq!(load_yaml(&encoded).expect("reload"), value);
    }

    #[test]
    fn bounded_scanner_corpus_never_panics() {
        let alphabet = b"abcXYZ012:-[]{}&*!%?.,# \\n\\t\\\"'";
        let mut state = 0x5eed_u64;
        for length in 0..512 {
            let mut source = String::with_capacity(length);
            for _ in 0..length {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1);
                source.push(alphabet[(state as usize) % alphabet.len()] as char);
            }
            if let Ok(value) = load_yaml(&source) {
                let encoded = serde_json::to_vec(&value).expect("safe YAML is JSON encodable");
                assert!(encoded.len() <= crate::limits::RESULT_BYTES);
            }
        }
    }
}

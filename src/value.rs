use std::collections::HashSet;

use num_traits::ToPrimitive;
use rustpython_vm::{
    AsObject, PyObjectRef, VirtualMachine,
    builtins::{PyDict, PyFloat, PyInt, PyList, PyStr, PyTuple},
};
use serde_json::{Map, Number, Value};
use yaml_rust2::{Yaml, yaml::Hash};

use crate::limits::{MAX_DEPTH, MAX_NODES, MAX_SAFE_INTEGER, RESULT_BYTES};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ConversionError {
    message: String,
}

impl ConversionError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub(crate) fn message(&self) -> &str {
        &self.message
    }
}

struct Budget {
    nodes: usize,
    ancestors: HashSet<usize>,
}

impl Budget {
    fn visit(&mut self, depth: usize) -> Result<(), ConversionError> {
        if depth > MAX_DEPTH {
            return Err(ConversionError::new(format!(
                "value exceeds maximum depth of {MAX_DEPTH}"
            )));
        }
        self.nodes = self
            .nodes
            .checked_add(1)
            .ok_or_else(|| ConversionError::new("value node count overflowed"))?;
        if self.nodes > MAX_NODES {
            return Err(ConversionError::new(format!(
                "value exceeds maximum node count of {MAX_NODES}"
            )));
        }
        Ok(())
    }

    fn visit_key(&mut self) -> Result<(), ConversionError> {
        self.nodes = self
            .nodes
            .checked_add(1)
            .ok_or_else(|| ConversionError::new("value node count overflowed"))?;
        if self.nodes > MAX_NODES {
            return Err(ConversionError::new(format!(
                "value exceeds maximum node count of {MAX_NODES}"
            )));
        }
        Ok(())
    }

    fn enter_container(&mut self, id: usize) -> Result<(), ConversionError> {
        if !self.ancestors.insert(id) {
            return Err(ConversionError::new("cyclic values are not supported"));
        }
        Ok(())
    }

    fn leave_container(&mut self, id: usize) {
        self.ancestors.remove(&id);
    }
}

pub(crate) fn py_to_safe_json(
    object: &PyObjectRef,
    vm: &VirtualMachine,
) -> Result<Value, ConversionError> {
    let mut budget = Budget {
        nodes: 0,
        ancestors: HashSet::new(),
    };
    let value = py_to_json_inner(object, vm, 1, &mut budget)?;
    let encoded = serde_json::to_vec(&value)
        .map_err(|_| ConversionError::new("value could not be encoded as JSON"))?;
    if encoded.len() > RESULT_BYTES {
        return Err(ConversionError::new(format!(
            "encoded value exceeds {RESULT_BYTES} bytes"
        )));
    }
    Ok(value)
}

fn py_to_json_inner(
    object: &PyObjectRef,
    vm: &VirtualMachine,
    depth: usize,
    budget: &mut Budget,
) -> Result<Value, ConversionError> {
    budget.visit(depth)?;
    if vm.is_none(object) {
        return Ok(Value::Null);
    }

    let class = object.class();
    if class.is(vm.ctx.types.bool_type) {
        return Ok(Value::Bool(object.is(&vm.ctx.true_value)));
    }
    if class.is(vm.ctx.types.int_type) {
        let integer = object
            .downcast_ref::<PyInt>()
            .and_then(|value| value.as_bigint().to_i64())
            .ok_or_else(|| ConversionError::new("integer is outside the safe JSON range"))?;
        if !(-MAX_SAFE_INTEGER..=MAX_SAFE_INTEGER).contains(&integer) {
            return Err(ConversionError::new(format!(
                "integer must be between -{MAX_SAFE_INTEGER} and {MAX_SAFE_INTEGER}"
            )));
        }
        return Ok(Value::Number(Number::from(integer)));
    }
    if class.is(vm.ctx.types.float_type) {
        let value = object
            .downcast_ref::<PyFloat>()
            .map(|value| value.to_f64())
            .ok_or_else(|| ConversionError::new("invalid float payload"))?;
        let number = Number::from_f64(value)
            .ok_or_else(|| ConversionError::new("non-finite floats are not supported"))?;
        return Ok(Value::Number(number));
    }
    if class.is(vm.ctx.types.str_type) {
        let value = object
            .downcast_ref::<PyStr>()
            .and_then(|value| value.to_str())
            .ok_or_else(|| ConversionError::new("strings must contain valid UTF-8"))?;
        return Ok(Value::String(value.to_owned()));
    }
    if class.is(vm.ctx.types.list_type) {
        let list = object
            .downcast_ref::<PyList>()
            .ok_or_else(|| ConversionError::new("invalid list payload"))?;
        let id = object.get_id();
        budget.enter_container(id)?;
        let result = list
            .borrow_vec()
            .iter()
            .map(|item| py_to_json_inner(item, vm, depth + 1, budget))
            .collect::<Result<Vec<_>, _>>();
        budget.leave_container(id);
        return result.map(Value::Array);
    }
    if class.is(vm.ctx.types.tuple_type) {
        let tuple = object
            .downcast_ref::<PyTuple>()
            .ok_or_else(|| ConversionError::new("invalid tuple payload"))?;
        let id = object.get_id();
        budget.enter_container(id)?;
        let result = tuple
            .as_slice()
            .iter()
            .map(|item| py_to_json_inner(item, vm, depth + 1, budget))
            .collect::<Result<Vec<_>, _>>();
        budget.leave_container(id);
        return result.map(Value::Array);
    }
    if class.is(vm.ctx.types.dict_type) {
        let dict = object
            .downcast_ref::<PyDict>()
            .ok_or_else(|| ConversionError::new("invalid dict payload"))?;
        let id = object.get_id();
        budget.enter_container(id)?;
        let result = (|| {
            let mut output = Map::new();
            for (key, value) in dict.items_vec() {
                budget.visit_key()?;
                if !key.class().is(vm.ctx.types.str_type) {
                    return Err(ConversionError::new("dict keys must be exact strings"));
                }
                let key = key
                    .downcast_ref::<PyStr>()
                    .and_then(|value| value.to_str())
                    .ok_or_else(|| ConversionError::new("dict keys must contain valid UTF-8"))?;
                output.insert(
                    key.to_owned(),
                    py_to_json_inner(&value, vm, depth + 1, budget)?,
                );
            }
            Ok(Value::Object(output))
        })();
        budget.leave_container(id);
        return result;
    }

    Err(ConversionError::new(format!(
        "unsupported result type '{}'",
        class.name()
    )))
}

pub(crate) fn safe_json_to_py(value: &Value, vm: &VirtualMachine) -> PyObjectRef {
    match value {
        Value::Null => vm.ctx.none(),
        Value::Bool(value) => vm.ctx.new_bool(*value).into(),
        Value::Number(value) => {
            if let Some(integer) = value.as_i64() {
                vm.ctx.new_int(integer).into()
            } else if let Some(integer) = value.as_u64() {
                vm.ctx.new_int(integer).into()
            } else {
                vm.ctx
                    .new_float(value.as_f64().expect("JSON number is representable as f64"))
                    .into()
            }
        }
        Value::String(value) => vm.ctx.new_str(value.as_str()).into(),
        Value::Array(values) => vm
            .ctx
            .new_list(
                values
                    .iter()
                    .map(|value| safe_json_to_py(value, vm))
                    .collect(),
            )
            .into(),
        Value::Object(values) => {
            let dict = vm.ctx.new_dict();
            for (key, value) in values {
                dict.set_item(key.as_str(), safe_json_to_py(value, vm), vm)
                    .expect("safe string dict key");
            }
            dict.into()
        }
    }
}

pub(crate) fn safe_json_to_yaml(value: &Value) -> Yaml {
    match value {
        Value::Null => Yaml::Null,
        Value::Bool(value) => Yaml::Boolean(*value),
        Value::Number(value) => value
            .as_i64()
            .map_or_else(|| Yaml::Real(value.to_string()), Yaml::Integer),
        Value::String(value) => Yaml::String(value.clone()),
        Value::Array(values) => {
            Yaml::Array(values.iter().map(safe_json_to_yaml).collect::<Vec<_>>())
        }
        Value::Object(values) => {
            let mut hash = Hash::new();
            for (key, value) in values {
                hash.insert(Yaml::String(key.clone()), safe_json_to_yaml(value));
            }
            Yaml::Hash(hash)
        }
    }
}

//! Query-only in-memory SQL. Each call owns its batches, session and bounded projection.
use std::sync::Arc;

use datafusion::{
    arrow::{
        array::{Array, ArrayRef, BooleanArray, Float64Array, Int64Array, StringArray},
        compute::cast,
        datatypes::{DataType, Field, Schema},
        record_batch::RecordBatch,
        util::display::array_value_to_string,
    },
    execution::context::SQLOptions,
    execution::{
        disk_manager::{DiskManagerBuilder, DiskManagerMode},
        runtime_env::RuntimeEnvBuilder,
        session_state::SessionStateBuilder,
    },
    prelude::{SessionConfig, SessionContext},
    sql::sqlparser::{ast::Statement, dialect::GenericDialect, parser::Parser},
};
use rustpython_vm::{PyObjectRef, PyResult, VirtualMachine, builtins::PyUtf8StrRef};
use serde_json::{Value, json};

use crate::{
    limits::RESULT_BYTES,
    value::{py_to_safe_json, safe_json_to_py},
};

const MAX_SQL_BYTES: usize = 4096;
const MAX_TABLES: usize = 4;
const MAX_COLUMNS: usize = 8;
const MAX_INPUT_ROWS: usize = 256;
const MAX_OUTPUT_ROWS: usize = 256;
const MAX_OUTPUT_COLUMNS: usize = 16;
const MAX_INPUT_BYTES: usize = 131_072;
const MAX_CELL_BYTES: usize = 4096;

#[rustpython_vm::pymodule(name = "dekopon_tables")]
pub(crate) mod tables_module {
    use super::*;

    #[pyfunction]
    fn query(sql: PyUtf8StrRef, tables: PyObjectRef, vm: &VirtualMachine) -> PyResult<PyObjectRef> {
        let input =
            py_to_safe_json(&tables, vm).map_err(|e| vm.new_value_error(e.message().to_owned()))?;
        let result = run(sql.as_str(), &input).map_err(|e| vm.new_value_error(e))?;
        Ok(safe_json_to_py(&result, vm))
    }
}

fn run(sql: &str, tables: &Value) -> Result<Value, String> {
    if sql.is_empty() || sql.len() > MAX_SQL_BYTES {
        return Err(format!("SQL must contain 1..={MAX_SQL_BYTES} UTF-8 bytes"));
    }
    let parsed = Parser::parse_sql(&GenericDialect {}, sql).map_err(bounded_error)?;
    if parsed.len() != 1 || !matches!(parsed.first(), Some(Statement::Query(_))) {
        return Err("only one SELECT query is permitted".into());
    }
    if serde_json::to_vec(tables).map_err(bounded_error)?.len() > MAX_INPUT_BYTES {
        return Err("table data exceeds 131072 bytes".into());
    }
    let tables = tables
        .as_object()
        .ok_or("tables must be an object of named row arrays")?;
    if tables.is_empty() || tables.len() > MAX_TABLES {
        return Err("tables must contain 1..=4 named row arrays".into());
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .map_err(bounded_error)?;
    runtime.block_on(async {
        let environment = RuntimeEnvBuilder::new()
            .with_disk_manager_builder(
                DiskManagerBuilder::default().with_mode(DiskManagerMode::Disabled),
            )
            .with_memory_limit(8 * 1024 * 1024, 1.0)
            .build()
            .map_err(bounded_error)?;
        let state = SessionStateBuilder::new()
            .with_config(SessionConfig::new().with_target_partitions(1))
            .with_runtime_env(Arc::new(environment))
            .with_default_features()
            .build();
        let ctx = SessionContext::new_with_state(state);
        let mut total_rows = 0;
        for (name, rows) in tables {
            if !valid_identifier(name) {
                return Err("table names must be short ASCII identifiers".into());
            }
            let rows = rows
                .as_array()
                .ok_or("table must be an array of row objects")?;
            total_rows += rows.len();
            if rows.is_empty() || total_rows > MAX_INPUT_ROWS {
                return Err("tables require nonempty rows, at most 256 total".into());
            }
            ctx.register_batch(name, record_batch(rows)?)
                .map_err(bounded_error)?;
        }
        // This plan verifier rejects DDL/DML and statement commands even inside a query wrapper.
        // Only the just-registered in-memory tables are present in this fresh session.
        let frame = ctx
            .sql_with_options(
                sql,
                SQLOptions::new()
                    .with_allow_ddl(false)
                    .with_allow_dml(false)
                    .with_allow_statements(false),
            )
            .await
            .map_err(bounded_error)?;
        let columns: Vec<String> = frame
            .schema()
            .fields()
            .iter()
            .map(|field| field.name().clone())
            .collect();
        if columns.len() > MAX_OUTPUT_COLUMNS {
            return Err("query exceeds 16 output columns".into());
        }
        let frame = frame
            .limit(0, Some(MAX_OUTPUT_ROWS + 1))
            .map_err(bounded_error)?;
        let batches = frame.collect().await.map_err(bounded_error)?;
        project(&columns, &batches)
    })
}

fn valid_identifier(name: &str) -> bool {
    name.len() <= 32
        && !name.is_empty()
        && name.bytes().next().is_some_and(|c| c.is_ascii_alphabetic())
        && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
}

fn record_batch(rows: &[Value]) -> Result<RecordBatch, String> {
    let first = rows[0].as_object().ok_or("rows must be objects")?;
    if first.is_empty()
        || first.len() > MAX_COLUMNS
        || first.keys().any(|key| !valid_identifier(key))
    {
        return Err("rows require 1..=8 short ASCII column identifiers".into());
    }
    for row in rows {
        let row = row.as_object().ok_or("rows must be objects")?;
        if row.len() != first.len() || row.keys().ne(first.keys()) {
            return Err("all rows must have the same columns".into());
        }
    }
    let mut fields = Vec::new();
    let mut arrays: Vec<ArrayRef> = Vec::new();
    for key in first.keys() {
        let values: Vec<&Value> = rows.iter().map(|row| &row[key]).collect();
        let kind = values
            .iter()
            .find(|v| !v.is_null())
            .ok_or("all-null columns need a type")?;
        let dtype = match kind {
            Value::Bool(_) => DataType::Boolean,
            Value::Number(_) if values.iter().any(|v| v.is_f64()) => DataType::Float64,
            Value::Number(n) if n.as_i64().is_some() => DataType::Int64,
            Value::Number(n) if n.as_f64().is_some() => DataType::Float64,
            Value::String(_) => DataType::Utf8,
            _ => return Err("columns support bool, signed int, finite float or string".into()),
        };
        let array: ArrayRef = match dtype {
            DataType::Boolean => Arc::new(BooleanArray::from(
                values.iter().map(|v| v.as_bool()).collect::<Vec<_>>(),
            )),
            DataType::Int64 => Arc::new(Int64Array::from(
                values.iter().map(|v| v.as_i64()).collect::<Vec<_>>(),
            )),
            DataType::Float64 => Arc::new(Float64Array::from(
                values
                    .iter()
                    .map(|v| v.as_f64().filter(|v| v.is_finite()))
                    .collect::<Vec<_>>(),
            )),
            DataType::Utf8 => Arc::new(StringArray::from(
                values.iter().map(|v| v.as_str()).collect::<Vec<_>>(),
            )),
            _ => unreachable!(),
        };
        for (value, index) in values.iter().zip(0..) {
            if !value.is_null()
                && (array.is_null(index)
                    || value.as_str().is_some_and(|s| s.len() > MAX_CELL_BYTES))
            {
                return Err("mixed, nonfinite or oversized column cell".into());
            }
        }
        fields.push(Field::new(key, dtype, true));
        arrays.push(array);
    }
    RecordBatch::try_new(Arc::new(Schema::new(fields)), arrays).map_err(bounded_error)
}

fn project(columns: &[String], batches: &[RecordBatch]) -> Result<Value, String> {
    let mut rows = Vec::new();
    for batch in batches {
        let arrays = batch
            .columns()
            .iter()
            .map(widen)
            .collect::<Result<Vec<_>, _>>()?;
        for index in 0..batch.num_rows() {
            if rows.len() >= MAX_OUTPUT_ROWS {
                return Err("query exceeds 256 output rows".into());
            }
            let row = arrays
                .iter()
                .map(|column| cell(column.as_ref(), index))
                .collect::<Result<Vec<_>, _>>()?;
            rows.push(row);
        }
    }
    let output = json!({"columns":columns,"rows":rows});
    if serde_json::to_vec(&output).map_err(bounded_error)?.len() > RESULT_BYTES {
        return Err("SQL result exceeds safe result bytes".into());
    }
    Ok(output)
}

fn widen(array: &ArrayRef) -> Result<ArrayRef, String> {
    let target = match array.data_type() {
        DataType::Int8
        | DataType::Int16
        | DataType::Int32
        | DataType::UInt8
        | DataType::UInt16
        | DataType::UInt32 => DataType::Int64,
        DataType::Utf8View | DataType::LargeUtf8 => DataType::Utf8,
        _ => return Ok(Arc::clone(array)),
    };
    cast(array, &target).map_err(bounded_error)
}

fn cell(array: &dyn Array, index: usize) -> Result<Value, String> {
    if array.is_null(index) {
        return Ok(Value::Null);
    }
    let value = match array.data_type() {
        DataType::Boolean => json!(
            array
                .as_any()
                .downcast_ref::<BooleanArray>()
                .unwrap()
                .value(index)
        ),
        DataType::Int64 => json!(
            array
                .as_any()
                .downcast_ref::<Int64Array>()
                .unwrap()
                .value(index)
        ),
        DataType::UInt64 => {
            let n = array
                .as_any()
                .downcast_ref::<datafusion::arrow::array::UInt64Array>()
                .unwrap()
                .value(index);
            if n > crate::limits::MAX_SAFE_INTEGER as u64 {
                return Err("SQL integer exceeds safe JSON range".into());
            }
            json!(n)
        }
        DataType::Float64 => {
            let n = array
                .as_any()
                .downcast_ref::<Float64Array>()
                .unwrap()
                .value(index);
            if !n.is_finite() {
                return Err("SQL produced a nonfinite float".into());
            }
            json!(n)
        }
        DataType::Utf8 => json!(
            array
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap()
                .value(index)
        ),
        DataType::Timestamp(_, _) | DataType::Date32 | DataType::Date64 => {
            json!(array_value_to_string(array, index).map_err(bounded_error)?)
        }
        _ => return Err("SQL result type is not supported by the safe projection".into()),
    };
    if value.as_str().is_some_and(|s| s.len() > MAX_CELL_BYTES)
        || value
            .as_i64()
            .is_some_and(|n| n.unsigned_abs() > crate::limits::MAX_SAFE_INTEGER as u64)
    {
        return Err("SQL cell exceeds safe result limits".into());
    }
    Ok(value)
}

fn bounded_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_query_retains_planned_columns() {
        let output = run("SELECT id FROM t WHERE id < 0", &json!({"t":[{"id":1}]}))
            .expect("valid empty SELECT");
        assert_eq!(output, json!({"columns":["id"],"rows":[]}));
    }

    #[test]
    fn rejects_nonquery_and_bad_shapes_before_execution() {
        let rows = json!({"t":[{"id":1}]});
        for sql in [
            "SELECT * FROM t; DROP TABLE t",
            "COPY t TO '/tmp/x'",
            "CREATE EXTERNAL TABLE x STORED AS CSV LOCATION '/tmp/x'",
        ] {
            assert!(run(sql, &rows).is_err(), "{sql}");
        }
        assert!(run("SELECT * FROM t", &json!({"t":[{"id":1},{"id":"bad"}]})).is_err());
        assert!(run("SELECT * FROM t", &json!({"t":5})).is_err());
    }

    #[test]
    fn integer_then_float_column_is_float() {
        let output = run(
            "SELECT price FROM t",
            &json!({"t":[{"price":10},{"price":10.5}]}),
        )
        .expect("mixed numeric column");
        assert_eq!(output, json!({"columns":["price"],"rows":[[10.0],[10.5]]}));
    }

    #[test]
    fn narrow_integer_results_project_as_integers() {
        let output = run(
            "SELECT length(name) AS n FROM t",
            &json!({"t":[{"name":"abc"}]}),
        )
        .expect("length() result");
        assert_eq!(output, json!({"columns":["n"],"rows":[[3]]}));
    }
}

//! Invocation-only sqlite3 subset over the same connection and WAL adapter as turso.exec.
use dekopon_turso_sql_io::turso_core::{Connection, IO, Numeric, StepResult, Value};
use rustpython_vm::{
    AsObject, PyPayload, PyResult, VirtualMachine, builtins::PyUtf8StrRef, function::FuncArgs,
    pyclass,
};
use std::{
    cell::{Cell, RefCell},
    num::NonZeroUsize,
    rc::Rc,
    sync::Arc,
};

type EngineHandle = (Arc<Connection>, Arc<dyn IO>);
thread_local! {
    static CURRENT: RefCell<Option<EngineHandle>> = RefCell::new(None);
}

pub(crate) fn with_connection<T>(
    connection: &Arc<Connection>,
    engine: &Arc<dyn IO>,
    run: impl FnOnce() -> T,
) -> T {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            CURRENT.with(|cell| *cell.borrow_mut() = None);
        }
    }
    CURRENT.with(|cell| *cell.borrow_mut() = Some((Arc::clone(connection), Arc::clone(engine))));
    let _reset = Reset;
    run()
}

pub(crate) fn active() -> bool {
    CURRENT.with(|cell| cell.borrow().is_some())
}

#[rustpython_vm::pymodule(name = "sqlite3")]
pub(crate) mod sqlite_module {
    use super::*;
    use rustpython_vm::{
        TryFromObject,
        builtins::{PyTupleRef, PyTypeRef},
    };

    #[pyattr(name = "ProgrammingError", once)]
    fn programming_error(vm: &VirtualMachine) -> PyTypeRef {
        crate::exception::immutable_exception_type(
            vm,
            "sqlite3",
            "ProgrammingError",
            vm.ctx.exceptions.exception_type.to_owned(),
        )
    }

    fn closed_error(vm: &VirtualMachine) -> rustpython_vm::builtins::PyBaseExceptionRef {
        vm.new_exception_msg(programming_error(vm), "connection closed".into())
    }

    #[pyattr]
    #[pyclass(module = "sqlite3", name = "Connection")]
    #[derive(Debug, PyPayload)]
    pub(crate) struct SqlConnection {
        closed: Rc<Cell<bool>>,
        transaction: Cell<bool>,
    }

    #[pyclass]
    impl SqlConnection {
        #[pymethod]
        fn close(&self, vm: &VirtualMachine) -> PyResult<()> {
            self.rollback(vm)?;
            self.closed.set(true);
            Ok(())
        }

        #[pymethod]
        fn commit(&self, vm: &VirtualMachine) -> PyResult<()> {
            self.finish_transaction("COMMIT", vm)
        }

        #[pymethod]
        fn rollback(&self, vm: &VirtualMachine) -> PyResult<()> {
            self.finish_transaction("ROLLBACK", vm)
        }

        #[pymethod]
        fn execute(
            &self,
            sql: PyUtf8StrRef,
            parameters: FuncArgs,
            vm: &VirtualMachine,
        ) -> PyResult<Cursor> {
            self.ensure_open(vm)?;
            if sql.as_str().is_empty() || sql.as_str().len() > 4096 {
                return Err(vm.new_value_error("SQL exceeds 4096 bytes or is empty"));
            }
            if parameters.args.len() > 1
                || parameters.kwargs.keys().any(|k| k != "parameters")
                || (!parameters.args.is_empty() && !parameters.kwargs.is_empty())
            {
                return Err(
                    vm.new_type_error("execute accepts SQL and one optional tuple of parameters")
                );
            }
            let params = parameters
                .args
                .first()
                .or_else(|| parameters.kwargs.get("parameters"))
                .map(|p| PyTupleRef::try_from_object(vm, p.clone()))
                .transpose()?;
            // Refuse bad arguments before touching the broker's durable files.
            if sql_has_multiple_statements(sql.as_str()) {
                return Err(vm.new_value_error("execute accepts one statement"));
            }
            if let Some(params) = &params {
                if params.len() > 64 {
                    return Err(vm.new_value_error("too many parameters"));
                }
                for param in params.iter() {
                    let _ = parameter(param, vm)?;
                }
            }
            let connection = self.connection(vm)?.0;
            let stmt = connection
                .prepare(sql.as_str())
                .map_err(|_| vm.new_value_error("SQL prepare failed"))?;
            if !sql.as_str()[stmt.tail_offset()..]
                .trim_matches(|c: char| c.is_whitespace() || c == ';')
                .is_empty()
            {
                return Err(vm.new_value_error("execute accepts one statement"));
            }
            if stmt.parameters_count() != params.as_ref().map_or(0, |p| p.len()) {
                return Err(vm.new_value_error("parameter count mismatch"));
            }
            drop(stmt);
            if !self.transaction.get()
                && ["insert", "update", "delete", "replace"].contains(
                    &sql.as_str()
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .to_ascii_lowercase()
                        .as_str(),
                )
            {
                run_sql(&connection, &self.connection(vm)?.1, "BEGIN", vm)?;
                self.transaction.set(true);
            }
            let mut stmt = connection
                .prepare(sql.as_str())
                .map_err(|_| vm.new_value_error("SQL prepare failed"))?;
            if let Some(params) = &params {
                for (index, param) in params.iter().enumerate() {
                    stmt.bind_at(NonZeroUsize::new(index + 1).unwrap(), parameter(param, vm)?)
                        .map_err(|_| vm.new_value_error("SQL binding failed"))?;
                }
            }
            let mut rows = Vec::new();
            let mut bytes = 0;
            let engine = self.connection(vm)?.1;
            loop {
                match stmt
                    .step()
                    .map_err(|_| vm.new_value_error("SQL step failed"))?
                {
                    StepResult::Row => {
                        if rows.len() >= 256 {
                            return Err(vm.new_value_error("SQL result exceeds 256 rows"));
                        }
                        let row = stmt
                            .row()
                            .ok_or_else(|| vm.new_value_error("missing SQL row"))?;
                        let values = row.get_values().cloned().collect::<Vec<_>>();
                        bytes += values.iter().map(value_size).sum::<usize>();
                        if bytes > 131_072 || values.len() > 32 {
                            return Err(vm.new_value_error("SQL result exceeds limit"));
                        }
                        rows.push(values);
                    }
                    StepResult::IO => engine
                        .step()
                        .map_err(|_| vm.new_value_error("SQL I/O failed"))?,
                    StepResult::Done => break,
                    _ => return Err(vm.new_value_error("SQL interrupted or busy")),
                }
            }
            Ok(Cursor {
                rows: RefCell::new(rows),
                index: RefCell::new(0),
                closed: Rc::clone(&self.closed),
            })
        }
    }

    impl SqlConnection {
        fn ensure_open(&self, vm: &VirtualMachine) -> PyResult<()> {
            if self.closed.get() {
                Err(closed_error(vm))
            } else {
                self.connection(vm).map(|_| ())
            }
        }
        fn finish_transaction(&self, statement: &str, vm: &VirtualMachine) -> PyResult<()> {
            self.ensure_open(vm)?;
            if self.transaction.get() {
                let (connection, engine) = self.connection(vm)?;
                run_sql(&connection, &engine, statement, vm)?;
                self.transaction.set(false);
            }
            Ok(())
        }
        fn connection(&self, vm: &VirtualMachine) -> PyResult<EngineHandle> {
            CURRENT
                .with(|cell| cell.borrow().clone())
                .ok_or_else(|| vm.new_value_error("connection outside invocation"))
        }
    }

    #[pyattr]
    #[pyclass(module = "sqlite3", name = "Cursor")]
    #[derive(Debug, PyPayload)]
    pub(crate) struct Cursor {
        rows: RefCell<Vec<Vec<Value>>>,
        index: RefCell<usize>,
        closed: Rc<Cell<bool>>,
    }
    #[pyclass]
    impl Cursor {
        #[pymethod]
        fn fetchone(&self, vm: &VirtualMachine) -> PyResult<rustpython_vm::PyObjectRef> {
            if self.closed.get() {
                return Err(closed_error(vm));
            }
            let mut index = self.index.borrow_mut();
            let rows = self.rows.borrow();
            let value = rows.get(*index);
            if value.is_some() {
                *index += 1;
            }
            Ok(value
                .map(|v| {
                    vm.ctx
                        .new_tuple(v.iter().map(|cell| convert(cell, vm)).collect())
                        .into()
                })
                .unwrap_or_else(|| vm.ctx.none()))
        }
        #[pymethod]
        fn fetchall(&self, vm: &VirtualMachine) -> PyResult<rustpython_vm::PyObjectRef> {
            if self.closed.get() {
                return Err(closed_error(vm));
            }
            let mut index = self.index.borrow_mut();
            let rows = self.rows.borrow();
            let remainder = rows[*index..]
                .iter()
                .map(|row| {
                    vm.ctx
                        .new_tuple(row.iter().map(|cell| convert(cell, vm)).collect())
                        .into()
                })
                .collect();
            *index = rows.len();
            Ok(vm.ctx.new_list(remainder).into())
        }
    }

    #[pyfunction]
    fn connect(args: FuncArgs, vm: &VirtualMachine) -> PyResult<SqlConnection> {
        if !args.args.is_empty() || !args.kwargs.is_empty() {
            return Err(vm.new_type_error("sqlite3.connect() is pathless"));
        }
        if CURRENT.with(|cell| cell.borrow().is_none()) {
            return Err(vm.new_value_error("sqlite unavailable outside invocation"));
        }
        Ok(SqlConnection {
            closed: Rc::new(Cell::new(false)),
            transaction: Cell::new(false),
        })
    }
}

// Conservative lexical gate: semicolons within quoted SQL strings/identifiers are data;
// any nontrivial suffix after a statement separator is forbidden before storage is opened.
fn sql_has_multiple_statements(sql: &str) -> bool {
    let mut quoted = None;
    let mut chars = sql.chars().peekable();
    while let Some(ch) = chars.next() {
        if let Some(quote) = quoted {
            if ch == quote {
                if chars.peek() == Some(&quote) {
                    chars.next();
                } else {
                    quoted = None;
                }
            }
        } else if ch == '\'' || ch == '"' || ch == '`' {
            quoted = Some(ch);
        } else if ch == ';' {
            return chars.any(|rest| !rest.is_whitespace() && rest != ';');
        }
    }
    false
}

fn value_size(value: &Value) -> usize {
    match value {
        Value::Text(s) => s.as_str().len(),
        Value::Blob(b) => b.len(),
        _ => 8,
    }
}

fn convert(value: &Value, vm: &VirtualMachine) -> rustpython_vm::PyObjectRef {
    match value {
        Value::Null => vm.ctx.none(),
        Value::Numeric(Numeric::Integer(n)) => vm.ctx.new_int(*n).into(),
        Value::Numeric(Numeric::Float(n)) => vm.ctx.new_float(f64::from(*n)).into(),
        Value::Text(s) => vm.ctx.new_str(s.as_str()).into(),
        Value::Blob(b) => vm.ctx.new_bytes(b.as_slice().to_vec()).into(),
    }
}

fn run_sql(
    connection: &Arc<Connection>,
    engine: &Arc<dyn IO>,
    sql: &str,
    vm: &VirtualMachine,
) -> PyResult<()> {
    let mut stmt = connection
        .prepare(sql)
        .map_err(|_| vm.new_value_error("transaction prepare failed"))?;
    loop {
        match stmt
            .step()
            .map_err(|_| vm.new_value_error("transaction failed"))?
        {
            StepResult::Done => return Ok(()),
            StepResult::IO => engine
                .step()
                .map_err(|_| vm.new_value_error("transaction I/O failed"))?,
            _ => return Err(vm.new_value_error("transaction busy or interrupted")),
        }
    }
}

fn parameter(param: &rustpython_vm::PyObjectRef, vm: &VirtualMachine) -> PyResult<Value> {
    use rustpython_vm::{
        TryFromObject,
        builtins::{PyBytes, PyFloat, PyUtf8StrRef},
    };
    if vm.is_none(param) {
        return Ok(Value::Null);
    }
    if param.class().is(vm.ctx.types.bool_type) {
        return Ok(Value::from_i64(i64::from(param.clone().try_to_bool(vm)?)));
    }
    if param.class().is(vm.ctx.types.int_type) {
        return i64::try_from_object(vm, param.clone()).map(Value::from_i64);
    }
    if param.class().is(vm.ctx.types.float_type) {
        let number = param.downcast_ref::<PyFloat>().unwrap().to_f64();
        if !number.is_finite() {
            return Err(vm.new_value_error("nonfinite SQL parameter"));
        }
        return Ok(Value::from_f64(number));
    }
    if param.class().is(vm.ctx.types.str_type) {
        let string = PyUtf8StrRef::try_from_object(vm, param.clone())?;
        if string.as_str().len() > 4096 {
            return Err(vm.new_value_error("SQL text parameter exceeds limit"));
        }
        return Ok(Value::build_text(string.as_str().to_owned()));
    }
    if param.class().is(vm.ctx.types.bytes_type) {
        let bytes = param.downcast_ref::<PyBytes>().unwrap().as_bytes();
        if bytes.len() > 4096 {
            return Err(vm.new_value_error("SQL blob parameter exceeds limit"));
        }
        return Ok(Value::from_blob(bytes.to_vec()));
    }
    Err(vm.new_type_error("unsupported SQL parameter"))
}

#[cfg(test)]
mod tests {
    use super::sql_has_multiple_statements;
    #[test]
    fn rejects_second_sql_statement_without_rejecting_semicolons_in_literals() {
        assert!(sql_has_multiple_statements("SELECT 1; DELETE FROM note"));
        assert!(!sql_has_multiple_statements("SELECT ';'"));
        assert!(!sql_has_multiple_statements("SELECT 1;"));
    }
}

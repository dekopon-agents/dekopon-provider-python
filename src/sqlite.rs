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
) -> Result<T, String> {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            CURRENT.with(|cell| *cell.borrow_mut() = None);
        }
    }
    CURRENT.with(|cell| *cell.borrow_mut() = Some((Arc::clone(connection), Arc::clone(engine))));
    let _reset = Reset;
    let result = run();
    // The VM can exit with a live write transaction on either success or exception.
    // Discard it before the shared adapter checkpoints committed WAL frames.
    if !connection.get_auto_commit() {
        run_sql_raw(connection, engine, "ROLLBACK")?;
    }
    Ok(result)
}

pub(crate) fn active() -> bool {
    CURRENT.with(|cell| cell.borrow().is_some())
}

#[rustpython_vm::pymodule(name = "sqlite3")]
pub(crate) mod sqlite_module {
    use super::*;
    use rustpython_vm::{
        TryFromObject,
        builtins::{PyList, PyTuple, PyTupleRef, PyTypeRef},
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
    }

    #[pyclass]
    impl SqlConnection {
        #[pymethod]
        fn close(&self, vm: &VirtualMachine) -> PyResult<()> {
            if self.closed.get() {
                return Ok(());
            }
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
            check_sql(&sql, vm)?;
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
            // Validate parameters before preparing against broker-owned durable files.
            let values = params
                .as_ref()
                .map(|params| tuple_values(params, vm))
                .transpose()?;
            let (connection, engine) = self.connection(vm)?;
            check_statement(&connection, &sql, values.as_ref().map_or(0, Vec::len), vm)?;
            if connection.get_auto_commit() && is_write(&sql) {
                run_sql(&connection, &engine, "BEGIN", vm)?;
            }
            let mut stmt = connection
                .prepare(sql.as_str())
                .map_err(|_| vm.new_value_error("SQL prepare failed"))?;
            bind_all(&mut stmt, values.unwrap_or_default(), vm)?;
            let mut rows = Vec::new();
            let mut bytes = 0;
            loop {
                match stmt
                    .step()
                    .map_err(|_| vm.new_value_error("SQL step failed"))?
                {
                    StepResult::Row => {
                        if rows.len() >= MAX_ROWS {
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

        #[pymethod]
        fn executemany(
            &self,
            sql: PyUtf8StrRef,
            seq_of_parameters: rustpython_vm::PyObjectRef,
            vm: &VirtualMachine,
        ) -> PyResult<Cursor> {
            self.ensure_open(vm)?;
            check_sql(&sql, vm)?;
            if !is_write(&sql) {
                return Err(vm.new_value_error(
                    "executemany accepts INSERT, UPDATE, DELETE or REPLACE statements",
                ));
            }
            let tuples: Vec<rustpython_vm::PyObjectRef> =
                if let Some(list) = seq_of_parameters.downcast_ref::<PyList>() {
                    list.borrow_vec().to_vec()
                } else if let Some(tuple) = seq_of_parameters.downcast_ref::<PyTuple>() {
                    tuple.to_vec()
                } else {
                    return Err(vm.new_type_error(
                        "executemany accepts a list or tuple of parameter tuples",
                    ));
                };
            if tuples.len() > MAX_ROWS {
                return Err(vm.new_value_error("executemany exceeds 256 parameter tuples"));
            }
            // Validate every tuple before the first write: nothing below can refuse a row
            // for type, count or size, so only a step failure remains after BEGIN.
            let batch = tuples
                .into_iter()
                .map(|item| {
                    let tuple = PyTupleRef::try_from_object(vm, item)?;
                    tuple_values(&tuple, vm)
                })
                .collect::<PyResult<Vec<_>>>()?;
            let (connection, engine) = self.connection(vm)?;
            check_statement(&connection, &sql, batch.first().map_or(0, Vec::len), vm)?;
            if batch.iter().any(|values| values.len() != batch[0].len()) {
                return Err(vm.new_value_error("parameter count mismatch"));
            }
            if !batch.is_empty() {
                let had_pending = !connection.get_auto_commit();
                if !had_pending {
                    run_sql(&connection, &engine, "BEGIN", vm)?;
                }
                run_sql(&connection, &engine, "SAVEPOINT executemany", vm)?;
                let written = batch
                    .into_iter()
                    .try_for_each(|values| write_row(&connection, &engine, &sql, values, vm));
                match written {
                    Ok(()) => run_sql(&connection, &engine, "RELEASE executemany", vm)?,
                    Err(error) => {
                        if connection.get_auto_commit() {
                            // The engine already rolled back the whole transaction (an
                            // `OR ROLLBACK` conflict clause) and dropped the savepoint.
                            return Err(vm.new_value_error(if had_pending {
                                "executemany failed and the engine rolled back the whole \
                                 transaction: earlier pending writes were lost"
                            } else {
                                "executemany failed and the engine rolled back the transaction"
                            }));
                        }
                        // Undo only this call's rows; earlier pending writes stay pending.
                        run_sql(&connection, &engine, "ROLLBACK TO executemany", vm)?;
                        run_sql(&connection, &engine, "RELEASE executemany", vm)?;
                        return Err(error);
                    }
                }
            }
            Ok(Cursor {
                rows: RefCell::new(Vec::new()),
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
            let (connection, engine) = self.connection(vm)?;
            if !connection.get_auto_commit() {
                run_sql(&connection, &engine, statement, vm)?;
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
        })
    }
}

const MAX_ROWS: usize = 256;

fn check_sql(sql: &PyUtf8StrRef, vm: &VirtualMachine) -> PyResult<()> {
    if sql.as_str().is_empty() || sql.as_str().len() > 4096 {
        return Err(vm.new_value_error("SQL exceeds 4096 bytes or is empty"));
    }
    Ok(())
}

fn is_write(sql: &PyUtf8StrRef) -> bool {
    ["insert", "update", "delete", "replace"].contains(
        &sql.as_str()
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str(),
    )
}

fn tuple_values(
    params: &rustpython_vm::builtins::PyTupleRef,
    vm: &VirtualMachine,
) -> PyResult<Vec<Value>> {
    if params.len() > 64 {
        return Err(vm.new_value_error("too many parameters"));
    }
    params.iter().map(|param| parameter(param, vm)).collect()
}

/// Prepares once to reject multiple statements and a wrong parameter count.
fn check_statement(
    connection: &Arc<Connection>,
    sql: &PyUtf8StrRef,
    parameter_count: usize,
    vm: &VirtualMachine,
) -> PyResult<()> {
    let stmt = connection
        .prepare(sql.as_str())
        .map_err(|_| vm.new_value_error("SQL prepare failed"))?;
    if !sql.as_str()[stmt.tail_offset()..]
        .trim_matches(|c: char| c.is_whitespace() || c == ';')
        .is_empty()
    {
        return Err(vm.new_value_error("execute accepts one statement"));
    }
    if stmt.parameters_count() != parameter_count {
        return Err(vm.new_value_error("parameter count mismatch"));
    }
    Ok(())
}

fn bind_all(
    stmt: &mut dekopon_turso_sql_io::turso_core::Statement,
    values: Vec<Value>,
    vm: &VirtualMachine,
) -> PyResult<()> {
    for (index, value) in values.into_iter().enumerate() {
        stmt.bind_at(NonZeroUsize::new(index + 1).unwrap(), value)
            .map_err(|_| vm.new_value_error("SQL binding failed"))?;
    }
    Ok(())
}

/// Runs one executemany row; a statement that returns rows is not a write.
fn write_row(
    connection: &Arc<Connection>,
    engine: &Arc<dyn IO>,
    sql: &PyUtf8StrRef,
    values: Vec<Value>,
    vm: &VirtualMachine,
) -> PyResult<()> {
    let mut stmt = connection
        .prepare(sql.as_str())
        .map_err(|_| vm.new_value_error("SQL prepare failed"))?;
    bind_all(&mut stmt, values, vm)?;
    loop {
        match stmt
            .step()
            .map_err(|_| vm.new_value_error("SQL step failed"))?
        {
            StepResult::Row => {
                return Err(vm.new_value_error("executemany statement must not return rows"));
            }
            StepResult::IO => engine
                .step()
                .map_err(|_| vm.new_value_error("SQL I/O failed"))?,
            StepResult::Done => return Ok(()),
            _ => return Err(vm.new_value_error("SQL interrupted or busy")),
        }
    }
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
    run_sql_raw(connection, engine, sql)
        .map_err(|detail| vm.new_value_error(format!("transaction {sql} failed: {detail}")))
}

fn run_sql_raw(
    connection: &Arc<Connection>,
    engine: &Arc<dyn IO>,
    sql: &str,
) -> Result<(), String> {
    let mut stmt = connection.prepare(sql).map_err(|error| error.to_string())?;
    loop {
        match stmt.step().map_err(|error| error.to_string())? {
            StepResult::Done => return Ok(()),
            StepResult::IO => engine.step().map_err(|error| error.to_string())?,
            StepResult::Busy => return Err("busy".to_owned()),
            StepResult::Interrupt => return Err("interrupted".to_owned()),
            _ => return Err("unexpected transaction step".to_owned()),
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

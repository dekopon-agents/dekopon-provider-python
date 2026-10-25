//! Invocation-only clock facade; never read time while describing or proposing a command.
use std::cell::RefCell;

use dekopon_provider_sdk::provider::{Clock, Monotonic};

thread_local! {
    static CLOCKS: RefCell<Option<(Clock, Monotonic)>> = const { RefCell::new(None) };
}

struct ClearClocks;
impl Drop for ClearClocks {
    fn drop(&mut self) {
        CLOCKS.with(|slot| {
            slot.borrow_mut().take();
        });
    }
}

pub(crate) fn with_clocks<R>(clock: Clock, monotonic: Monotonic, f: impl FnOnce() -> R) -> R {
    CLOCKS.with(|slot| {
        assert!(slot.borrow().is_none(), "nested Python clock invocation");
        *slot.borrow_mut() = Some((clock, monotonic));
    });
    let _clear = ClearClocks;
    f()
}

#[rustpython_vm::pymodule(name = "_dekopon_time")]
pub(crate) mod time_module {
    use rustpython_vm::{PyResult, VirtualMachine};

    #[pyfunction]
    fn time(vm: &VirtualMachine) -> PyResult<f64> {
        super::CLOCKS.with(|slot| {
            slot.borrow()
                .as_ref()
                .map(|(clock, _)| clock.now_unix_millis() as f64 / 1000.0)
                .ok_or_else(|| vm.new_runtime_error("clock unavailable outside invocation"))
        })
    }

    #[pyfunction]
    fn monotonic(vm: &VirtualMachine) -> PyResult<f64> {
        super::CLOCKS.with(|slot| {
            slot.borrow()
                .as_ref()
                .map(|(_, clock)| clock.now_nanos() as f64 / 1_000_000_000.0)
                .ok_or_else(|| vm.new_runtime_error("clock unavailable outside invocation"))
        })
    }
    #[pyfunction]
    fn time_ns(vm: &VirtualMachine) -> PyResult<u64> {
        super::CLOCKS.with(|slot| {
            slot.borrow()
                .as_ref()
                .map(|(clock, _)| clock.now_unix_millis() * 1_000_000)
                .ok_or_else(|| vm.new_runtime_error("clock unavailable outside invocation"))
        })
    }

    #[pyfunction]
    fn _gmtime(seconds: f64, vm: &VirtualMachine) -> PyResult<rustpython_vm::builtins::PyTupleRef> {
        use chrono::{DateTime, Datelike, Timelike};
        if !seconds.is_finite()
            || seconds.floor() < i64::MIN as f64
            || seconds.floor() >= i64::MAX as f64
        {
            return Err(vm.new_overflow_error("timestamp out of range"));
        }
        let date = DateTime::from_timestamp(seconds.floor() as i64, 0)
            .ok_or_else(|| vm.new_overflow_error("timestamp out of range"))?;
        Ok(vm.new_tuple(
            vec![
                date.year(),
                date.month() as i32,
                date.day() as i32,
                date.hour() as i32,
                date.minute() as i32,
                date.second() as i32,
                date.weekday().num_days_from_monday() as i32,
                date.ordinal() as i32,
                0,
            ]
            .into_iter()
            .map(|n| vm.ctx.new_int(n).into())
            .collect::<Vec<_>>(),
        ))
    }

    #[pyfunction]
    fn _strftime(
        format: String,
        fields: rustpython_vm::builtins::PyTupleRef,
        vm: &VirtualMachine,
    ) -> PyResult<String> {
        use chrono::{NaiveDate, TimeZone, Utc};
        use std::fmt::Write;
        if fields.len() != 9 {
            return Err(vm.new_type_error("strftime requires a 9-item time tuple"));
        }
        let values = fields
            .iter()
            .map(|v| v.try_to_value::<i32>(vm))
            .collect::<PyResult<Vec<_>>>()?;
        let date = NaiveDate::from_ymd_opt(values[0], values[1] as u32, values[2] as u32)
            .and_then(|d| d.and_hms_opt(values[3] as u32, values[4] as u32, values[5] as u32))
            .ok_or_else(|| vm.new_value_error("invalid time tuple"))?;
        let date = Utc.from_utc_datetime(&date);
        let mut output = String::new();
        write!(output, "{}", date.format(&format))
            .map_err(|_| vm.new_value_error("invalid strftime format"))?;
        Ok(output)
    }
}

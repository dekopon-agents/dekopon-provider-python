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

#[rustpython_vm::pymodule(name = "time")]
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
}

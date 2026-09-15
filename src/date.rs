//! Invoke-only broker wall time; no guest clock or fallback.
use rustpython_vm::{PyResult, VirtualMachine, builtins::PyIntRef};

#[rustpython_vm::pymodule(name = "dekopon_date")]
pub(crate) mod date_module {
    use super::*;

    #[pyfunction]
    fn now_unix_millis(vm: &VirtualMachine) -> PyResult<PyIntRef> {
        // Native argument binding rejects arguments before this single host read.
        safe_millis(dekopon_provider_clock::now_unix_millis(), vm)
    }
}

fn safe_millis(millis: u64, vm: &VirtualMachine) -> PyResult<PyIntRef> {
    if millis > crate::limits::MAX_SAFE_INTEGER as u64 {
        return Err(vm.new_overflow_error("clock exceeds safe integer range"));
    }
    Ok(vm.ctx.new_int(millis))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustpython_vm::AsObject;

    #[test]
    fn date_millis_conversion_preserves_safe_boundaries_and_rejects_overflow() {
        std::thread::Builder::new()
            .stack_size(32 * 1024 * 1024)
            .spawn(|| {
                crate::eval::interpreter().enter(|vm| {
                    let maximum = crate::limits::MAX_SAFE_INTEGER as u64;
                    for millis in [0, maximum] {
                        let value = safe_millis(millis, vm).expect("safe clock value");
                        assert!(value.class().is(vm.ctx.types.int_type));
                        assert_eq!(
                            crate::value::py_to_safe_json(&value.into(), vm).unwrap(),
                            serde_json::json!(millis)
                        );
                    }
                    for millis in [maximum + 1, u64::MAX] {
                        let error = safe_millis(millis, vm).expect_err("unsafe clock value");
                        assert!(error.class().is(vm.ctx.exceptions.overflow_error));
                        assert_eq!(
                            error.args().as_slice()[0]
                                .str(vm)
                                .unwrap()
                                .to_str()
                                .unwrap(),
                            "clock exceeds safe integer range"
                        );
                    }
                });
            })
            .unwrap()
            .join()
            .unwrap();
    }
}

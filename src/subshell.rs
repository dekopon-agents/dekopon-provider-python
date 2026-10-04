//! Child Dekopon shell scripts. Only the broker-granted spawn import runs them, under this
//! invocation's person, agent and budget.
use rustpython_vm::{
    PyPayload, PyResult, VirtualMachine,
    builtins::{PyBaseExceptionRef, PyTypeRef},
};

use dekopon_provider_sdk::provider::{ChildStdin, Spawn};
use std::{cell::RefCell, io::Read};

use crate::limits::{SCRIPT_BYTES, STDOUT_BYTES};

thread_local! {
    static INVOCATION_SPAWN: RefCell<Option<Spawn>> = const { RefCell::new(None) };
}

struct ClearSpawn;
impl Drop for ClearSpawn {
    fn drop(&mut self) {
        INVOCATION_SPAWN.with(|slot| {
            slot.borrow_mut().take();
        });
    }
}

pub(crate) fn with_spawn<R>(spawn: Spawn, f: impl FnOnce() -> R) -> R {
    INVOCATION_SPAWN.with(|slot| {
        assert!(slot.borrow().is_none(), "nested Python spawn invocation");
        *slot.borrow_mut() = Some(spawn);
    });
    let _clear = ClearSpawn;
    f()
}

fn bounded_utf8(mut raw: Vec<u8>, limit: usize) -> (String, bool) {
    let mut truncated = raw.len() > limit;
    raw.truncate(limit);
    if truncated
        && let Err(error) = std::str::from_utf8(&raw)
        && error.error_len().is_none()
    {
        raw.truncate(error.valid_up_to());
    }
    let mut text = String::from_utf8_lossy(&raw).into_owned();
    if text.len() > limit {
        let mut boundary = limit;
        while !text.is_char_boundary(boundary) {
            boundary -= 1;
        }
        text.truncate(boundary);
        truncated = true;
    }
    (text, truncated)
}

#[rustpython_vm::pymodule(name = "dekopon_subshell")]
pub(crate) mod subshell_module {
    use super::*;
    use rustpython_vm::{
        FromArgs, PyObjectRef, PyRef, builtins::PyUtf8StrRef, class::PyClassImpl, pyclass,
    };

    #[pyattr(name = "SubshellError", once)]
    fn error(vm: &VirtualMachine) -> PyTypeRef {
        crate::exception::immutable_exception_type(
            vm,
            "dekopon_subshell",
            "SubshellError",
            vm.ctx.exceptions.exception_type.to_owned(),
        )
    }

    #[pyattr]
    #[pyclass(module = "dekopon_subshell", name = "Inherit")]
    #[derive(Debug, PyPayload)]
    pub(crate) struct Inherit;

    #[pyclass]
    impl Inherit {}

    #[pyattr(name = "INHERIT")]
    fn inherit(vm: &VirtualMachine) -> PyRef<Inherit> {
        Inherit::make_static_type();
        Inherit.into_ref(&vm.ctx)
    }

    #[pyattr]
    #[pyclass(module = "dekopon_subshell", name = "CompletedRun")]
    #[derive(Debug, PyPayload)]
    pub(crate) struct CompletedRun {
        returncode: u8,
        stdout: String,
        stderr: String,
        truncated: bool,
    }

    #[pyclass]
    impl CompletedRun {
        #[pygetset]
        const fn returncode(&self) -> u8 {
            self.returncode
        }

        #[pygetset]
        fn stdout(&self) -> String {
            self.stdout.clone()
        }

        #[pygetset]
        fn stderr(&self) -> String {
            self.stderr.clone()
        }

        #[pygetset]
        const fn truncated(&self) -> bool {
            self.truncated
        }
    }

    #[derive(FromArgs)]
    struct RunArgs {
        #[pyarg(any)]
        script: PyUtf8StrRef,
        #[pyarg(any, default)]
        stdin: Option<PyObjectRef>,
    }

    #[pyfunction]
    fn run(args: RunArgs, vm: &VirtualMachine) -> PyResult<CompletedRun> {
        let script = args.script.as_str();
        if script.len() > SCRIPT_BYTES {
            return Err(exception(vm, error(vm), "script exceeds 65536 UTF-8 bytes"));
        }
        let stdin = match args.stdin {
            None => ChildStdin::None,
            Some(value) if value.downcast_ref::<Inherit>().is_some() => ChildStdin::Inherit,
            Some(_) => {
                return Err(vm.new_type_error("stdin must be None or dekopon_subshell.INHERIT"));
            }
        };
        INVOCATION_SPAWN.with(|slot| {
            let handle = slot.borrow();
            let spawn = handle
                .as_ref()
                .ok_or_else(|| exception(vm, error(vm), "spawn not granted"))?;
            let mut child = spawn
                .run(script, stdin)
                .map_err(|failure| exception(vm, error(vm), &failure.to_string()))?;
            let mut raw = Vec::new();
            let read = (&mut child.stdout)
                .take(STDOUT_BYTES as u64 + 1)
                .read_to_end(&mut raw);
            let exit = child.wait();
            read.map_err(|_| exception(vm, error(vm), "child stdout could not be read"))?;
            let (stdout, truncated) = bounded_utf8(raw, STDOUT_BYTES);
            let (stderr, _) = bounded_utf8(exit.stderr.into_bytes(), STDOUT_BYTES);
            Ok(CompletedRun {
                returncode: exit.status,
                stdout,
                stderr,
                truncated,
            })
        })
    }
}

fn exception(vm: &VirtualMachine, class: PyTypeRef, message: &str) -> PyBaseExceptionRef {
    vm.new_exception_msg(class, message.to_owned().into())
}

#[cfg(test)]
mod tests {
    #[test]
    fn outside_invocation_run_has_no_spawn_handle() {
        std::thread::Builder::new()
            .stack_size(32 * 1024 * 1024)
            .spawn(|| {
                let script = "import dekopon_subshell as s\ns.run('gh pr list')";
                let before = crate::eval::evaluate(script);
                assert_eq!(before["error"]["type"], "SubshellError", "{before}");
                assert_eq!(before["error"]["message"], "spawn not granted");
                let native = dekopon_provider_sdk_testkit::Native::<crate::PythonProvider>::new()
                    .child(dekopon_provider_sdk_testkit::ChildScript {
                        script: "gh pr list".to_owned(),
                        ..Default::default()
                    });
                let inside = native.call(
                    "python.eval",
                    &serde_json::json!({"script": script}).to_string(),
                );
                assert_eq!(inside.status, 0, "{}", inside.stderr);
                assert_eq!(native.children().len(), 1);
                let after = crate::eval::evaluate(script);
                assert_eq!(after["error"]["message"], "spawn not granted", "{after}");
                assert!(super::INVOCATION_SPAWN.with(|slot| slot.borrow().is_none()));
            })
            .unwrap()
            .join()
            .unwrap();
    }
}

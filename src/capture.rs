use std::cell::RefCell;

use rustpython_vm::{VirtualMachine, builtins::PyStrRef};

use crate::limits::STDOUT_BYTES;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Captured {
    pub(crate) stdout: String,
    pub(crate) truncated: bool,
}

thread_local! {
    static CAPTURE: RefCell<Captured> = RefCell::new(Captured::default());
}

pub(crate) fn reset() {
    CAPTURE.with(|capture| *capture.borrow_mut() = Captured::default());
}

pub(crate) fn snapshot() -> Captured {
    CAPTURE.with(|capture| capture.borrow().clone())
}

fn append(value: &str) {
    CAPTURE.with(|capture| {
        let mut capture = capture.borrow_mut();
        if capture.truncated {
            return;
        }
        let remaining = STDOUT_BYTES.saturating_sub(capture.stdout.len());
        if value.len() <= remaining {
            capture.stdout.push_str(value);
            return;
        }
        let mut boundary = remaining.min(value.len());
        while boundary > 0 && !value.is_char_boundary(boundary) {
            boundary -= 1;
        }
        capture.stdout.push_str(&value[..boundary]);
        capture.truncated = true;
    });
}

#[rustpython_vm::pymodule(name = "_dekopon_stdout")]
pub(crate) mod stdout_module {
    use super::*;

    #[pyfunction]
    fn write(value: PyStrRef) -> usize {
        let value = value.to_string_lossy();
        let characters = value.chars().count();
        append(&value);
        characters
    }

    #[pyfunction]
    const fn flush() {}

    #[pyfunction]
    const fn isatty() -> bool {
        false
    }

    #[pyattr]
    const ENCODING: &str = "utf-8";
}

#[rustpython_vm::pymodule(name = "_dekopon_stderr")]
pub(crate) mod stderr_module {
    use super::*;

    #[pyfunction]
    fn write(value: PyStrRef) -> usize {
        value.to_string_lossy().chars().count()
    }

    #[pyfunction]
    const fn flush() {}

    #[pyfunction]
    const fn isatty() -> bool {
        false
    }

    #[pyattr]
    const ENCODING: &str = "utf-8";
}

pub(crate) fn install(vm: &VirtualMachine) -> rustpython_vm::PyResult<()> {
    let stdout = vm.import("_dekopon_stdout", 0)?;
    let stderr = vm.import("_dekopon_stderr", 0)?;
    vm.sys_module.set_attr("stdout", stdout.clone(), vm)?;
    vm.sys_module.set_attr("__stdout__", stdout, vm)?;
    vm.sys_module.set_attr("stderr", stderr.clone(), vm)?;
    vm.sys_module.set_attr("__stderr__", stderr, vm)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{append, reset, snapshot};
    use crate::limits::STDOUT_BYTES;

    #[test]
    fn truncates_at_a_utf8_boundary() {
        reset();
        append(&"a".repeat(STDOUT_BYTES - 1));
        append("étail");
        let captured = snapshot();
        assert_eq!(captured.stdout.len(), STDOUT_BYTES - 1);
        assert!(captured.stdout.is_char_boundary(captured.stdout.len()));
        assert!(captured.truncated);
    }

    #[test]
    fn bounded_chunk_corpus_never_exceeds_or_splits_the_limit() {
        for offset in 0..64 {
            reset();
            for index in 0..512 {
                let chunk = match (index + offset) % 5 {
                    0 => "ascii",
                    1 => "é",
                    2 => "🦀",
                    3 => "\n",
                    _ => "日本語",
                };
                append(&chunk.repeat((index % 17) + 1));
            }
            let captured = snapshot();
            assert!(captured.stdout.len() <= STDOUT_BYTES);
            assert!(captured.stdout.is_char_boundary(captured.stdout.len()));
        }
    }
}

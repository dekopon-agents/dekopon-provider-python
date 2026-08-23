use std::cell::Cell;

use rustpython_vm::{PyResult, VirtualMachine};

const ALLOWED_ROOTS: [&str; 3] = ["json", "re", "yaml"];
const REMOVED_BUILTINS: [&str; 3] = ["open", "input", "breakpoint"];
const GUARDED_BUILTINS: [&str; 3] = ["compile", "eval", "exec"];

thread_local! {
    // Non-zero only while trusted frozen/native code is resolving the private dependency closure
    // of an allowlisted public root. A top-level guest import never inherits this state.
    static TRUSTED_IMPORT_DEPTH: Cell<usize> = const { Cell::new(0) };
}

#[rustpython_vm::pymodule(name = "_dekopon_policy")]
pub(crate) mod policy_module {
    use rustpython_vm::{
        PyResult, TryFromObject, VirtualMachine, builtins::PyStrRef, function::FuncArgs,
    };

    #[pyfunction]
    fn guarded_import(args: FuncArgs, vm: &VirtualMachine) -> PyResult {
        let name_object = args
            .args
            .first()
            .cloned()
            .or_else(|| args.kwargs.get("name").cloned())
            .ok_or_else(|| vm.new_type_error("__import__() missing required argument 'name'"))?;
        let name = PyStrRef::try_from_object(vm, name_object)?;
        let name = name
            .to_str()
            .ok_or_else(|| vm.new_import_error("module name must be UTF-8", name.clone()))?;

        let level = args
            .args
            .get(4)
            .cloned()
            .or_else(|| args.kwargs.get("level").cloned())
            .map(|value| i32::try_from_object(vm, value))
            .transpose()?
            .unwrap_or(0);
        let nested = super::TRUSTED_IMPORT_DEPTH.with(|depth| depth.get() != 0);
        if !nested && (level != 0 || !super::is_allowed_root(name)) {
            return Err(vm.new_import_error(
                format!("import of '{name}' is not permitted"),
                vm.ctx.new_utf8_str(name),
            ));
        }

        let modules = vm.sys_module.get_attr("modules", vm)?;
        let policy = modules.get_item("_dekopon_policy", vm)?;
        let original = policy.get_attr("_original_import", vm)?;
        super::TRUSTED_IMPORT_DEPTH.with(|depth| {
            let previous = depth.get();
            depth.set(previous.saturating_add(1));
            let result = original.call(args, vm);
            depth.set(previous);
            result
        })
    }

    #[pyfunction]
    fn guarded_compile(args: FuncArgs, vm: &VirtualMachine) -> PyResult {
        super::call_guarded_builtin("compile", args, vm)
    }

    #[pyfunction]
    fn guarded_eval(args: FuncArgs, vm: &VirtualMachine) -> PyResult {
        super::call_guarded_builtin("eval", args, vm)
    }

    #[pyfunction]
    fn guarded_exec(args: FuncArgs, vm: &VirtualMachine) -> PyResult {
        super::call_guarded_builtin("exec", args, vm)
    }
}

fn call_guarded_builtin(
    name: &str,
    args: rustpython_vm::function::FuncArgs,
    vm: &VirtualMachine,
) -> PyResult {
    let trusted = TRUSTED_IMPORT_DEPTH.with(|depth| depth.get() != 0);
    if !trusted {
        return Err(vm.new_runtime_error(format!("builtin '{name}' is not permitted")));
    }
    let modules = vm.sys_module.get_attr("modules", vm)?;
    let policy = modules.get_item("_dekopon_policy", vm)?;
    let original = policy.get_attr(vm.ctx.intern_str(format!("_original_{name}")), vm)?;
    original.call(args, vm)
}

pub(crate) fn install(vm: &VirtualMachine) -> PyResult<()> {
    // Public imports are loaded lazily. During one allowlisted root import, its trusted frozen
    // implementation may resolve private dependencies; direct guest imports of those same names
    // remain denied once the root import returns.
    let policy = vm.import("_dekopon_policy", 0)?;
    let original = vm.builtins.get_attr("__import__", vm)?;
    policy.set_attr("_original_import", original, vm)?;
    let guarded = policy.get_attr("guarded_import", vm)?;
    vm.builtins.set_attr("__import__", guarded, vm)?;

    let builtins = vm.builtins.dict();
    for name in REMOVED_BUILTINS {
        let _removed = builtins.del_item(name, vm);
    }
    for name in GUARDED_BUILTINS {
        let original = builtins.get_item(name, vm)?;
        policy.set_attr(vm.ctx.intern_str(format!("_original_{name}")), original, vm)?;
        let guarded = policy.get_attr(vm.ctx.intern_str(format!("guarded_{name}")), vm)?;
        builtins.set_item(name, guarded, vm)?;
    }
    Ok(())
}

pub(crate) fn is_allowed_root(name: &str) -> bool {
    let root = name.split('.').next().unwrap_or_default();
    ALLOWED_ROOTS.contains(&root)
}

#[cfg(test)]
mod tests {
    use super::is_allowed_root;

    #[test]
    fn allowlist_is_root_based_and_closed() {
        assert!(is_allowed_root("json"));
        assert!(is_allowed_root("re._parser"));
        assert!(is_allowed_root("yaml"));
        for denied in [
            "sys",
            "os",
            "time",
            "random",
            "socket",
            "subprocess",
            "ctypes",
        ] {
            assert!(!is_allowed_root(denied), "{denied}");
        }
    }
}

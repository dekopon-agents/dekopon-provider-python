use rustpython_vm::{AsObject, PyResult, TryFromObject, VirtualMachine, builtins::PyDictRef};

const ALLOWED_MODULES: [&str; 3] = ["json", "re", "yaml"];
const REMOVED_BUILTINS: [&str; 6] = ["open", "input", "breakpoint", "compile", "eval", "exec"];
const DENIED_MODULES: [&str; 15] = [
    "sys",
    "os",
    "pathlib",
    "time",
    "random",
    "secrets",
    "socket",
    "ssl",
    "sqlite3",
    "subprocess",
    "threading",
    "ctypes",
    "tkinter",
    "webbrowser",
    "_dekopon_policy",
];

#[rustpython_vm::pymodule(name = "_dekopon_policy")]
pub(crate) mod policy_module {
    use rustpython_vm::{
        AsObject, PyObjectRef, PyResult, TryFromObject, VirtualMachine,
        builtins::{PyStrRef, PyTupleRef},
        function::FuncArgs,
    };

    const IMPORT_ARGUMENTS: [&str; 5] = ["name", "globals", "locals", "fromlist", "level"];

    fn import_argument(
        args: &FuncArgs,
        index: usize,
        name: &str,
        vm: &VirtualMachine,
    ) -> PyResult<Option<PyObjectRef>> {
        let positional = args.args.get(index).cloned();
        let keyword = args.kwargs.get(name).cloned();
        if positional.is_some() && keyword.is_some() {
            return Err(vm.new_type_error(format!(
                "__import__() got multiple values for argument '{name}'"
            )));
        }
        Ok(positional.or(keyword))
    }

    #[pyfunction]
    fn guarded_import(args: FuncArgs, vm: &VirtualMachine) -> PyResult {
        if args.args.len() > IMPORT_ARGUMENTS.len() {
            return Err(vm.new_type_error(format!(
                "__import__() takes at most {} arguments ({} given)",
                IMPORT_ARGUMENTS.len(),
                args.args.len()
            )));
        }
        if let Some(unexpected) = args
            .kwargs
            .keys()
            .find(|keyword| !IMPORT_ARGUMENTS.contains(&keyword.as_str()))
        {
            return Err(vm.new_type_error(format!(
                "__import__() got an unexpected keyword argument '{unexpected}'"
            )));
        }

        let name_object = import_argument(&args, 0, "name", vm)?
            .ok_or_else(|| vm.new_type_error("__import__() missing required argument 'name'"))?;
        let name = PyStrRef::try_from_object(vm, name_object)?;
        let name = name
            .to_str()
            .ok_or_else(|| vm.new_import_error("module name must be UTF-8", name.clone()))?;

        let level = import_argument(&args, 4, "level", vm)?
            .map(|value| i32::try_from_object(vm, value))
            .transpose()?
            .unwrap_or(0);
        if level != 0 || !super::is_allowed_module(name) {
            return Err(vm.new_import_error(
                format!("import of '{name}' is not permitted"),
                vm.ctx.new_utf8_str(name),
            ));
        }

        // All public modules and their private dependency closures are loaded before this guard is
        // installed. Return only an exact public module from the Rust-owned sys module rather than
        // retaining or invoking Python-visible importlib/original-import callables.
        let modules = vm.sys_module.get_attr("modules", vm)?;
        let module = modules.get_item(name, vm)?;

        // IMPORT_NAME passes a tuple of requested attributes. Permit public values such as
        // `from json import loads`, but do not let IMPORT_FROM bind a private implementation name,
        // a star-expanded namespace, or a module object such as json.decoder / re._parser.
        if let Some(fromlist) = import_argument(&args, 3, "fromlist", vm)?
            && !vm.is_none(&fromlist)
        {
            if !fromlist.class().is(vm.ctx.types.tuple_type) {
                return Err(vm.new_type_error("__import__() fromlist must be an exact tuple"));
            }
            let fromlist = PyTupleRef::try_from_object(vm, fromlist)?;
            for requested in fromlist.iter() {
                let requested_object = PyStrRef::try_from_object(vm, requested.clone())?;
                let requested = requested_object
                    .to_str()
                    .ok_or_else(|| {
                        vm.new_import_error("imported name must be UTF-8", requested_object.clone())
                    })?
                    .to_owned();
                if requested == "*" || requested.starts_with('_') {
                    return Err(vm.new_import_error(
                        format!("import of '{name}.{requested}' is not permitted"),
                        vm.ctx.new_utf8_str(format!("{name}.{requested}")),
                    ));
                }
                let value = module.get_attr(&requested_object, vm).map_err(|_| {
                    vm.new_import_error(
                        format!("cannot import name '{requested}' from '{name}'"),
                        vm.ctx.new_utf8_str(name),
                    )
                })?;
                if value.class().fast_issubclass(vm.ctx.types.module_type) {
                    return Err(vm.new_import_error(
                        format!("module-valued import of '{name}.{requested}' is not permitted"),
                        vm.ctx.new_utf8_str(format!("{name}.{requested}")),
                    ));
                }
            }
        }

        Ok(module)
    }
}

pub(crate) fn install(vm: &VirtualMachine) -> PyResult<()> {
    // Resolve the complete frozen/native dependency closure while the VM still has its pristine
    // internal import machinery. Guest imports never execute that machinery.
    let public_modules = ALLOWED_MODULES
        .iter()
        .map(|name| vm.import(*name, 0).map(|module| (*name, module)))
        .collect::<PyResult<Vec<_>>>()?;
    let policy = vm.import("_dekopon_policy", 0)?;
    let guarded_import = policy.get_attr("guarded_import", vm)?;

    let modules_object = vm.sys_module.get_attr("modules", vm)?;
    let modules = PyDictRef::try_from_object(vm, modules_object)?;

    // Capture identities before removing the builtins. If importlib or a transitive frozen module
    // aliased one of these privileged callables, remove that alias too; comparing object identity
    // avoids deleting unrelated public functions such as re.compile.
    let privileged_callables = ["__import__"]
        .into_iter()
        .chain(REMOVED_BUILTINS)
        .filter_map(|name| vm.builtins.get_attr(name, vm).ok())
        .collect::<Vec<_>>();
    let denied_modules = DENIED_MODULES
        .iter()
        .filter_map(|name| modules.get_item(*name, vm).ok())
        .collect::<Vec<_>>();

    for module in modules.values_vec() {
        // Module specs/loaders lead back into frozen importlib functions that are not part of the
        // public module contract. They are unnecessary after eager loading and would otherwise be
        // an alternate path around the exact-name guard.
        for metadata in ["__loader__", "__spec__", "__cached__", "__file__"] {
            let _removed = module.del_attr(metadata, vm);
        }
        let Ok(namespace) = module.get_attr("__dict__", vm) else {
            continue;
        };
        let Ok(namespace) = PyDictRef::try_from_object(vm, namespace) else {
            continue;
        };
        for (key, value) in namespace.items_vec() {
            let privileged = privileged_callables
                .iter()
                .any(|callable| value.is(callable));
            let denied_reference = denied_modules
                .iter()
                .any(|denied_module| value.is(denied_module));
            if privileged || denied_reference {
                let _removed = namespace.del_item(key.as_object(), vm);
            }
        }
    }

    // enum is an implementation detail of re and was the shortest path to enum.sys.modules.
    // RegexFlag has already been constructed, and normal matching/compilation does not need this
    // module-global reference after initialization.
    if let Some((_, re_module)) = public_modules.iter().find(|(name, _)| *name == "re") {
        let _removed = re_module.del_attr("enum", vm);
    }

    // RustPython's global type-subclass registry exposes frozen importlib loader classes (and its
    // 0.5.0 enumeration can itself panic on internal static types). It is not part of python.eval's
    // supported surface, so remove the Python-visible traversal after trusted initialization.
    let type_type = vm.ctx.types.type_type;
    type_type.modified();
    if type_type
        .attributes
        .write()
        .shift_remove(vm.ctx.intern_str("__subclasses__"))
        .is_none()
    {
        return Err(vm.new_runtime_error("failed to close type introspection"));
    }

    vm.builtins.set_attr("__import__", guarded_import, vm)?;
    let builtins = vm.builtins.dict();
    for name in REMOVED_BUILTINS {
        let _removed = builtins.del_item(name, vm);
    }

    // Replace the Python-visible registry with the closed public set. This removes importlib,
    // policy-module, and denied-module recovery through any residual transitive sys reference.
    let public_registry = vm.ctx.new_dict();
    for (name, module) in public_modules {
        public_registry.set_item(name, module, vm)?;
    }
    vm.sys_module.set_attr("modules", public_registry, vm)?;
    Ok(())
}

pub(crate) fn is_allowed_module(name: &str) -> bool {
    ALLOWED_MODULES.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::is_allowed_module;

    #[test]
    fn allowlist_is_exact_and_closed() {
        for allowed in ["json", "re", "yaml"] {
            assert!(is_allowed_module(allowed), "{allowed}");
        }
        for denied in [
            "re._parser",
            "json.decoder",
            "sys",
            "os",
            "time",
            "random",
            "socket",
            "subprocess",
            "ctypes",
        ] {
            assert!(!is_allowed_module(denied), "{denied}");
        }
    }
}

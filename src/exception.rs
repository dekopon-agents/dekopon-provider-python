//! Factory-owned native exception classes preserve selection and payload layout despite guest writes.
use rustpython_vm::{
    VirtualMachine,
    builtins::{PyType, PyTypeRef},
    types::{PyTypeFlags, PyTypeSlots},
};

// Like RustPython's immutable native exceptions, ordinary class writes and bases/MRO changes are
// rejected. This is not transitive immutability: RustPython's heap-type __annotations__ getter
// exposes a mutable dictionary. Its entries do not select native classes or affect payload layout.
// The caller's #[pyattr(once)] shares Context::genesis's static-cell lifecycle, so annotations can
// persist across supplemental native interpreters. Production invocations use fresh Wasm instances.
pub(crate) fn immutable_exception_type(
    vm: &VirtualMachine,
    module: &'static str,
    name: &'static str,
    base: PyTypeRef,
) -> PyTypeRef {
    let attrs = [(
        vm.ctx.intern_str("__module__"),
        vm.ctx.new_str(module).into(),
    )]
    .into_iter()
    .collect();
    PyType::new_heap(
        name,
        vec![base],
        attrs,
        PyTypeSlots::new(
            name,
            PyTypeFlags::heap_type_flags() | PyTypeFlags::HAS_DICT | PyTypeFlags::IMMUTABLETYPE,
        ),
        vm.ctx.types.type_type.to_owned(),
        &vm.ctx,
    )
    .expect("native exception base and layout")
}

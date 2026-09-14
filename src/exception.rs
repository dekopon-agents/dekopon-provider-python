//! Native exception classes must not accept guest mutations, including across fresh interpreters.
use rustpython_vm::{
    VirtualMachine,
    builtins::{PyType, PyTypeRef},
    types::{PyTypeFlags, PyTypeSlots},
};

// Like RustPython's immutable native exceptions. The caller's #[pyattr(once)] uses the same
// static-cell lifecycle as Context::genesis; only immutable class state is shared, not VM state.
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

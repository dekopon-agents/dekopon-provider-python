//! Opt-in fixed Python facade for a separately composed engine component.
use rustpython_vm::VirtualMachine;

mod bindings {
    wit_bindgen::generate!({
        path: "experiments/engine-swap/facade-wit",
        world: "engine-client",
        with: { "dekopon:engine/api@0.1.0": generate },
    });
}

#[rustpython_vm::pymodule(name = "dekopon_engine")]
pub(crate) mod engine_module {
    use super::bindings::dekopon::engine::api;
    use super::*;

    #[pyfunction]
    fn engine_name() -> String {
        api::engine_name()
    }

    #[pyfunction]
    fn burn(iterations: u32) -> u32 {
        api::burn(iterations)
    }

    #[pyfunction]
    fn allocate(bytes: u32) -> u32 {
        api::allocate(bytes)
    }

    #[pyfunction]
    fn probe(_vm: &VirtualMachine) -> (String, u32, u32, u32) {
        let before = api::live_count();
        let (value, during) = {
            let counter = api::Counter::new();
            (counter.get(), api::live_count())
        };
        (value, before, during, api::live_count())
    }
}

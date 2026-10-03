//! Low-fuel host traps before RustPython can execute; one compiled registry in this binary.
use dekopon_provider_sdk_testkit::{BrokerHostLimits, Harness};
use dekopon_python_provider::PythonProvider;
use serde_json::json;
use std::path::PathBuf;

fn component() -> PathBuf {
    std::env::var_os("DEKOPON_PROVIDER_COMPONENT")
        .expect("fresh component")
        .into()
}

#[test]
fn finite_host_fuel_traps_before_guest_can_catch_it() {
    let limits = BrokerHostLimits {
        fuel: 10_000_000,
        ..BrokerHostLimits::default()
    };
    let error = Harness::<PythonProvider>::get(component()).host_limits(limits)
        .call("python.eval", json!({"script":"try:\n    while True: pass\nexcept Exception:\n    result = 'caught'"}))
        .expect_err("fuel exhaustion is a host trap, not a Python exception");
    assert!(
        format!("{error:?}").to_ascii_lowercase().contains("fuel"),
        "{error:?}"
    );
    assert_eq!(Harness::<PythonProvider>::compiled_identities(), 1);
}

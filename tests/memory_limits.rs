//! Guest recursion is structured; Wasm memory exhaustion is a host trap, not guest JSON.
use dekopon_provider_sdk_testkit::{BrokerHostLimits, Harness};
use dekopon_python_provider::PythonProvider;
use serde_json::{Value, json};
use std::path::PathBuf;

fn component() -> PathBuf {
    std::env::var_os("DEKOPON_PROVIDER_COMPONENT")
        .expect("fresh component")
        .into()
}
#[test]
fn recursion_and_linear_memory_limits_remain_distinct() {
    assert_eq!(
        BrokerHostLimits::default().max_memory_bytes,
        64 * 1024 * 1024
    );
    let recursion = Harness::<PythonProvider>::get(component())
        .call("python.eval", json!({"script":"def f(): return f()\nf()"}))
        .expect("recursion is a Python-level error");
    assert_eq!(recursion.status, 0, "{}", recursion.stderr);
    let body: Value = serde_json::from_slice(&recursion.stdout).expect("bounded error envelope");
    assert_eq!(body["error"]["type"], "RecursionError");

    let error = Harness::<PythonProvider>::get(component())
        .call(
            "python.eval",
            json!({"script":"result = bytearray(100000000)"}),
        )
        .expect_err("Wasm memory exhaustion is not a catchable Python error");
    let detail = format!("{error:?}").to_ascii_lowercase();
    assert!(
        detail.contains("memory")
            || detail.contains("grow")
            || detail.contains("resource")
            || detail.contains("unreachable"),
        "{detail}"
    );
    assert_eq!(Harness::<PythonProvider>::compiled_identities(), 1);
}

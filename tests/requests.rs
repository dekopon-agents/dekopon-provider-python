//! Broker-mediated HTTP is available only inside one authorized invocation.
use dekopon_provider_sdk::provider::{Header, Response};
use dekopon_provider_sdk_testkit::{Harness, HttpScript, Native};
use dekopon_python_provider::PythonProvider;
use serde_json::{Value, json};
use std::path::PathBuf;

fn component() -> PathBuf {
    std::env::var_os("DEKOPON_PROVIDER_COMPONENT")
        .expect("DEKOPON_PROVIDER_COMPONENT must point at freshly built component")
        .into()
}
fn script() -> HttpScript {
    // The deployed crates.io GET use case, served locally by the broker's HTTP fixture.
    HttpScript::new(
        "crates.io",
        "GET",
        Response {
            status: 200,
            headers: vec![Header::text("content-type", "application/json").unwrap()],
            body: br#"{"crate":{"name":"serde","max_version":"1.0.229"}}"#.to_vec(),
        },
    )
}
fn program(origin: &str) -> Value {
    json!({"script":format!("import dekopon_requests as requests\nr = requests.get({:?})\nresult = [r.status_code, r.json()['crate']['name'], r.json()['crate']['max_version']]", format!("{origin}/api/v1/crates/serde"))})
}
fn result(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).expect("bounded JSON")
}

#[test]
fn deployed_crates_io_get_uses_authorized_http_in_real_component_and_native() {
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(|| {
            let path = component();
            let run = Harness::<PythonProvider>::get(&path).http(script());
            let origin = run.origin().expect("broker HTTPS fixture").to_owned();
            let actual = run
                .call("python.eval", program(&origin))
                .expect("authorized component call");
            assert_eq!(actual.status, 0, "{}", actual.stderr);
            assert_eq!(
                result(&actual.stdout)["result"],
                json!([200, "serde", "1.0.229"])
            );
            let native = Native::<PythonProvider>::new().http(script());
            let local = native.call("python.eval", &program("https://crates.io").to_string());
            assert_eq!(local.status, 0, "{}", local.stderr);
            assert_eq!(result(&local.stdout), result(&actual.stdout));
            let requests = native.requests();
            assert_eq!(requests.len(), 1);
            assert_eq!(requests[0].method, "GET");
            assert_eq!(requests[0].uri, "https://crates.io/api/v1/crates/serde");
            assert!(requests[0].body.is_empty());
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn no_http_handle_outside_invocation_and_no_reuse_in_next_invocation() {
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(|| {
            let native = Native::<PythonProvider>::new().http(script());
            let first = native.call("python.eval", &program("https://crates.io").to_string());
            assert_eq!(first.status, 0, "{}", first.stderr);
            assert_eq!(result(&first.stdout)["ok"], true);
            // A separate VM without a broker-granted HTTP response cannot use the prior invocation's
            // handle; neither the module import nor the captured Python function gives ambient access.
            let denied = Native::<PythonProvider>::new()
                .call("python.eval", &program("https://crates.io").to_string());
            assert_eq!(denied.status, 0, "{}", denied.stderr);
            assert_eq!(result(&denied.stdout)["ok"], false);
            assert_eq!(result(&denied.stdout)["error"]["kind"], "runtime");
            assert_eq!(result(&denied.stdout)["error"]["message"], "http not granted");
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn http_grant_is_not_needed_for_pure_scripts_and_import_alone_is_safe() {
    let real = Harness::<PythonProvider>::get(component())
        .call(
            "python.eval",
            json!({"script":"import dekopon_requests\nresult = 42"}),
        )
        .expect("pure call");
    assert_eq!(real.status, 0, "{}", real.stderr);
    assert_eq!(result(&real.stdout)["result"], 42);
}

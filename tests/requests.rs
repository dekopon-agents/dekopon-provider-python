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
            let no_grant = Native::<PythonProvider>::new();
            let denied = no_grant.call("python.eval", &program("https://crates.io").to_string());
            assert_eq!(denied.status, 0, "{}", denied.stderr);
            assert_eq!(result(&denied.stdout)["ok"], false);
            assert_eq!(result(&denied.stdout)["error"]["kind"], "runtime");
            assert_eq!(result(&denied.stdout)["error"]["message"], "denied");
            // Native records attempted calls *before* checking its fixture; one denied attempt
            // must not be mistaken for a successful network request or reused first fixture.
            assert_eq!(no_grant.requests().len(), 1);
            let real_denial = Harness::<PythonProvider>::get(component())
                .call("python.eval", program("https://crates.io"))
                .expect_err("the broker denies HTTP without a grant");
            let dekopon_provider_sdk_testkit::HarnessError::Invocation(failure) = real_denial
            else {
                panic!("expected broker invocation failure: {real_denial:?}");
            };
            assert!(
                failure.http_calls.is_empty(),
                "no HTTP transport call without grant: {failure:?}"
            );
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

fn scripted(
    status: u16,
    body: Vec<u8>,
    headers: Vec<Header>,
) -> dekopon_provider_sdk_testkit::Run<PythonProvider> {
    Harness::<PythonProvider>::get(component()).http(HttpScript::new(
        "localhost",
        "GET",
        Response {
            status,
            headers,
            body,
        },
    ))
}

fn request_script(origin: &str, body: &str) -> Value {
    json!({"script": format!("import dekopon_requests as requests\nbase = {:?}\n{body}", origin)})
}

#[test]
fn broker_denies_out_of_scope_host_and_method_before_transport() {
    let run = scripted(200, b"ok".to_vec(), vec![]);
    let mismatched_host = run.call(
        "python.eval",
        request_script(
            "https://elsewhere.invalid",
            "requests.get(base + '/not-granted')",
        ),
    );
    let error = mismatched_host.expect_err("host mismatch must be a fatal broker refusal");
    assert!(
        format!("{error:?}").to_ascii_lowercase().contains("denied"),
        "{error:?}"
    );

    // A GET grant cannot be used for HEAD, even if the Python script catches the exception.
    let run = scripted(200, Vec::new(), vec![]);
    let origin = run.origin().expect("broker fixture").to_owned();
    let denied = run.call("python.eval", request_script(&origin, "try:\n    requests.head(base + '/index')\nexcept requests.RequestException:\n    pass\nresult = 'caught'"));
    let error = denied.expect_err("caught method denial remains fatal at the broker");
    assert!(
        format!("{error:?}").to_ascii_lowercase().contains("denied"),
        "{error:?}"
    );
}

#[test]
fn host_call_budget_survives_guest_exception_handler() {
    let run = scripted(200, b"one".to_vec(), vec![]);
    let origin = run.origin().expect("broker fixture").to_owned();
    let denied = run.call("python.eval", request_script(&origin,
        "for i in range(3):\n    try: requests.get(base + '/one')\n    except requests.RequestException: pass\nresult = 'caught'"));
    let error = denied.expect_err("host call budget must remain exhausted");
    assert!(
        format!("{error:?}")
            .to_ascii_lowercase()
            .contains("host-call-limit"),
        "{error:?}"
    );
}

#[test]
fn redirect_is_data_and_not_followed() {
    let run = scripted(
        302,
        Vec::new(),
        vec![Header::text("location", "https://elsewhere.invalid/never").unwrap()],
    );
    let origin = run.origin().expect("broker fixture").to_owned();
    let output = run.call("python.eval", request_script(&origin,
        "r = requests.get(base + '/redirect')\nresult = [r.status_code, r.ok, len(r.content)]"))
        .expect("redirect response, not a request to the Location");
    assert_eq!(output.status, 0, "{}", output.stderr);
    assert_eq!(result(&output.stdout)["result"], json!([302, true, 0]));
    assert_eq!(output.http_calls.len(), 1, "no redirect follow-up");
}

#[test]
fn broker_response_ceiling_and_url_credentials_are_refused() {
    let run = scripted(200, vec![b'x'; 70_000], vec![]);
    let origin = run.origin().expect("broker fixture").to_owned();
    let error = run
        .call(
            "python.eval",
            request_script(&origin, "requests.get(base + '/large')"),
        )
        .expect_err("64 KiB scripted response exceeds testkit's broker ceiling");
    assert!(
        format!("{error:?}")
            .to_ascii_lowercase()
            .contains("response"),
        "{error:?}"
    );
    let run = scripted(200, Vec::new(), vec![]);
    let origin = run.origin().expect("broker fixture").to_owned();
    let with_userinfo = origin.replacen("https://", "https://user:password@", 1);
    let error = run
        .call(
            "python.eval",
            request_script(&with_userinfo, "requests.get(base + '/index')"),
        )
        .expect_err("URI credentials must not be sent to host transport");
    assert!(
        format!("{error:?}")
            .to_ascii_lowercase()
            .contains("invalid"),
        "{error:?}"
    );
}

#[test]
fn invalid_json_and_out_of_range_numbers_keep_facade_errors() {
    for body in [
        b"not JSON".to_vec(),
        b"9007199254740992".to_vec(),
        b"[".repeat(130),
    ] {
        let run = scripted(200, body, vec![]);
        let origin = run.origin().expect("broker fixture").to_owned();
        let output = run
            .call(
                "python.eval",
                request_script(&origin, "requests.get(base + '/json').json()"),
            )
            .expect("Python JSON error is a structured response");
        assert_eq!(output.status, 0, "{}", output.stderr);
        assert_eq!(result(&output.stdout)["error"]["type"], "JSONDecodeError");
    }
}

#[test]
fn status_errors_url_limits_and_facade_signature_remain_closed() {
    let run = scripted(404, b"missing".to_vec(), vec![]);
    let origin = run.origin().expect("broker fixture").to_owned();
    let output = run
        .call(
            "python.eval",
            request_script(
                &origin,
                "requests.get(base + '/missing').raise_for_status()",
            ),
        )
        .expect("HTTP status is a Python facade error");
    assert_eq!(result(&output.stdout)["error"]["type"], "HTTPError");
    assert_eq!(
        result(&output.stdout)["error"]["message"],
        "HTTP status 404"
    );
    for source in [
        "requests.get('x' * 8193)",
        "requests.get('https://crates.io/', allow_redirects=True)",
        "requests.get(1)",
    ] {
        let output = Harness::<PythonProvider>::get(component())
            .call(
                "python.eval",
                json!({"script": format!("import dekopon_requests as requests\n{source}")}),
            )
            .expect("facade refusal is structured JSON and needs no host effect");
        assert_eq!(output.status, 0, "{}", output.stderr);
        let decoded = result(&output.stdout);
        assert_eq!(decoded["ok"], false, "{source}: {decoded}");
        assert_eq!(
            output.http_calls.len(),
            0,
            "{source} cannot reach broker HTTP"
        );
    }
}

#[test]
fn provider_additional_body_ceiling_is_bounded_after_native_host_response() {
    std::thread::Builder::new().stack_size(32 * 1024 * 1024).spawn(|| {
        for (size, allowed) in [(131_072, true), (131_073, false)] {
            let native = Native::<PythonProvider>::new().http(HttpScript::new("crates.io", "GET", Response {
                status: 200, headers: vec![], body: vec![b'x'; size],
            }));
            let output = native.call("python.eval", &json!({"script":
                "import dekopon_requests as r\nresult = len(r.get('https://crates.io/large').content)"}).to_string());
            assert_eq!(output.status, 0, "{}", output.stderr);
            let decoded = result(&output.stdout);
            if allowed {
                assert_eq!(decoded["result"], size);
            } else {
                assert_eq!(decoded["error"]["type"], "RequestException");
                assert_eq!(decoded["error"]["message"], "response exceeds 131072 bytes");
            }
            assert_eq!(native.requests().len(), 1);
        }
    }).unwrap().join().unwrap();
}

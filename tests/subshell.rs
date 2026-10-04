//! `dekopon_subshell` runs child scripts only through the invocation's spawn import.
use dekopon_provider_sdk_testkit::{ChildInput, ChildScript, Harness, Native};
use dekopon_python_provider::PythonProvider;
use serde_json::{Value, json};
use std::path::PathBuf;

const STDOUT_BYTES: usize = 65_536;
const PIPELINE: &str = "gh pr list -R dekopon-agents/dekopon | rg x";

fn program(body: &str) -> Value {
    json!({"script": format!("import dekopon_subshell as s\n{body}")})
}

const ENVELOPE: &str = "result = {'returncode': r.returncode, 'stdout': r.stdout, 'stderr': r.stderr, 'truncated': r.truncated}";

fn child(status: u8, stdout: &[u8]) -> ChildScript {
    ChildScript {
        script: PIPELINE.to_owned(),
        status,
        stdout: stdout.to_vec(),
        ..ChildScript::default()
    }
}

fn answer(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).expect("bounded JSON")
}

fn on_big_stack(f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap();
}

fn native(native: &Native<PythonProvider>, body: &str) -> Value {
    let output = native.call("python.eval", &program(body).to_string());
    assert_eq!(output.status, 0, "{}", output.stderr);
    answer(&output.stdout)
}

#[test]
fn run_returns_completed_run_with_child_status_and_stdout() {
    on_big_stack(|| {
        let local = Native::<PythonProvider>::new()
            .child(child(0, b"fix x\n"))
            .child(child(3, b""));
        let ok = native(
            &local,
            &format!(
                "r = s.run({PIPELINE:?})\n{ENVELOPE}\nr = s.run({PIPELINE:?})\nresult = [result, r.returncode, r.stdout]"
            ),
        );
        assert_eq!(
            ok["result"],
            json!([{"returncode": 0, "stdout": "fix x\n", "stderr": "", "truncated": false}, 3, ""]),
            "{ok}"
        );
        let children = local.children();
        assert_eq!(children.len(), 2);
        assert_eq!(children[0].script, PIPELINE);
        assert_eq!(children[0].stdin, ChildInput::None);
    });
}

#[test]
fn child_stdout_is_capped_on_a_utf8_boundary_and_flagged() {
    on_big_stack(|| {
        let local = Native::<PythonProvider>::new()
            .child(child(0, "é".repeat(STDOUT_BYTES).as_bytes()))
            .child(child(0, &vec![b'a'; STDOUT_BYTES]));
        let output = native(
            &local,
            &format!(
                "r = s.run({PIPELINE:?})\nq = s.run({PIPELINE:?})\nresult = [len(r.stdout.encode()), r.truncated, len(q.stdout), q.truncated]"
            ),
        );
        assert_eq!(
            output["result"],
            json!([STDOUT_BYTES, true, STDOUT_BYTES, false]),
            "{output}"
        );
    });
}

#[test]
fn stdin_is_none_by_default_and_inherit_hands_over_the_rest() {
    on_big_stack(|| {
        let local = Native::<PythonProvider>::new()
            .stdin(b"piped rows\n".to_vec())
            .child(child(0, b""))
            .child(child(0, b""));
        let output = native(
            &local,
            &format!(
                "s.run({PIPELINE:?}, stdin=None)\ns.run({PIPELINE:?}, stdin=s.INHERIT)\nresult = 1"
            ),
        );
        assert_eq!(output["ok"], true, "{output}");
        let children = local.children();
        assert_eq!(children[0].stdin, ChildInput::None);
        assert_eq!(
            children[1].stdin,
            ChildInput::Inherit(b"piped rows\n".to_vec())
        );
        let refused = native(
            &Native::<PythonProvider>::new(),
            &format!("s.run({PIPELINE:?}, stdin=b'data')"),
        );
        assert_eq!(refused["error"]["type"], "TypeError", "{refused}");
    });
}

#[test]
fn subshell_errors_and_closed_imports() {
    on_big_stack(|| {
        let local = Native::<PythonProvider>::new();
        let refused = native(&local, "s.run('x' * 65537)");
        assert_eq!(refused["error"]["type"], "SubshellError", "{refused}");
        assert_eq!(
            refused["error"]["message"],
            "script exceeds 65536 UTF-8 bytes"
        );
        let bad_input = native(&local, &format!("s.run({PIPELINE:?}, input=b'data')"));
        assert_eq!(bad_input["error"]["type"], "TypeError", "{bad_input}");
        for module in ["subprocess", "os", "sys", "socket", "ctypes"] {
            let denied = native(&local, &format!("import {module}"));
            assert_eq!(denied["ok"], false, "{module}: {denied}");
            assert_eq!(denied["error"]["type"], "ImportError", "{module}: {denied}");
        }
        assert!(local.children().is_empty());
    });
}

#[test]
fn offline_pipeline_fixture_returns_bounded_envelope() {
    on_big_stack(|| {
        let local = Native::<PythonProvider>::new().child(child(0, b"x #123 fix\n"));
        let output = native(&local, &format!("r = s.run({PIPELINE:?})\n{ENVELOPE}"));
        assert_eq!(
            output["result"],
            json!({"returncode": 0, "stdout": "x #123 fix\n", "stderr": "", "truncated": false})
        );
        assert_eq!(local.children()[0].script, PIPELINE);
    });
}

#[test]
fn component_and_native_subshell_agree() {
    let component: PathBuf = std::env::var_os("DEKOPON_PROVIDER_COMPONENT")
        .expect("DEKOPON_PROVIDER_COMPONENT must point at freshly built component")
        .into();
    on_big_stack(move || {
        let body = format!(
            "r = s.run({PIPELINE:?}, stdin=s.INHERIT)\n{ENVELOPE}\nq = s.run({PIPELINE:?})\nresult['big'] = [len(q.stdout), q.truncated]"
        );
        let real = Harness::<PythonProvider>::get(&component)
            .stdin(b"rows\n".to_vec())
            .child(child(1, b"x\n"))
            .child(child(0, &vec![b'y'; STDOUT_BYTES + 10]))
            .call("python.eval", program(&body))
            .expect("component invocation");
        assert_eq!(real.status, 0, "{}", real.stderr);
        let local = Native::<PythonProvider>::new()
            .stdin(b"rows\n".to_vec())
            .child(child(1, b"x\n"))
            .child(child(0, &vec![b'y'; STDOUT_BYTES + 10]));
        let native_output = native(&local, &body);
        assert_eq!(answer(&real.stdout), native_output);
        assert_eq!(
            native_output["result"],
            json!({"returncode": 1, "stdout": "x\n", "stderr": "", "truncated": false, "big": [STDOUT_BYTES, true]}),
            "{native_output}"
        );
        assert_eq!(real.children, local.children());
        assert_eq!(
            real.children[0].stdin,
            ChildInput::Inherit(b"rows\n".to_vec())
        );
        assert_eq!(real.children[1].stdin, ChildInput::None);
    });
}

//! Real-component, authorized invocation and pure CLI proposal witnesses.
use dekopon_provider_sdk::{CommandRunOutcome, provider};
use dekopon_provider_sdk_testkit::{Harness, Native, conformance};
use dekopon_python_provider::PythonProvider;
use serde_json::{Value, json};
use std::path::PathBuf;

fn component() -> PathBuf {
    std::env::var_os("DEKOPON_PROVIDER_COMPONENT")
        .expect("DEKOPON_PROVIDER_COMPONENT must point at freshly built component")
        .into()
}
fn answer(output: &[u8]) -> Value {
    serde_json::from_slice(output).expect("bounded JSON on stdout")
}

#[test]
fn manifest_and_component_conform_to_typed_stdio() -> Result<(), Box<dyn std::error::Error>> {
    conformance::<PythonProvider>(component())?;
    let manifest = provider::manifest::<PythonProvider>()?;
    assert_eq!(manifest.id.as_str(), "python");
    assert_eq!(manifest.command_words, ["python"]);
    assert_eq!(manifest.capabilities.len(), 1);
    let cap = &manifest.capabilities[0];
    assert_eq!(cap.id.as_str(), "python.eval");
    assert_eq!(cap.input_schema["additionalProperties"], false);
    Ok(())
}

#[test]
fn broker_component_and_native_keep_sandbox_and_script_streams() {
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(|| {
            let path = component();
            let source = b"print('hello')\nresult = 40 + 2\n";
            let input = json!({"script":"", "stdin_script":true});
            let real = Harness::<PythonProvider>::get(&path)
                .stdin(source.to_vec())
                .call("python.eval", input.clone())
                .expect("component invocation");
            assert_eq!(real.status, 0, "{}", real.stderr);
            let native = Native::<PythonProvider>::new().stdin(source.to_vec());
            let local = native.call("python.eval", &input.to_string());
            assert_eq!(local.status, 0, "{}", local.stderr);
            assert_eq!(answer(&real.stdout), answer(&local.stdout));
            assert_eq!(answer(&real.stdout)["result"], 42);
            assert_eq!(answer(&real.stdout)["stdout"], "hello\n");

            // -c is an inline proposal: piped stdin is program data, not replacement source.
            let CommandRunOutcome::Proposed {
                input: code,
                capability,
                ..
            } = provider::command::<PythonProvider>(&["-c".into(), "result = 7".into()], true)
            else {
                panic!("-c must propose")
            };
            assert_eq!(capability.as_str(), "python.eval");
            assert_eq!(code, json!({"script":"result = 7"}));
            let code_result = Harness::<PythonProvider>::get(&path)
                .stdin(b"raise RuntimeError('ignored')".to_vec())
                .call("python.eval", code)
                .expect("-c invocation");
            assert_eq!(answer(&code_result.stdout)["result"], 7);
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn input_bound_and_invalid_input_are_rejected_before_vm_or_http() {
    let component = component();
    let oversized = json!({"script":"x".repeat(65_537)});
    let result = Harness::<PythonProvider>::get(&component)
        .call("python.eval", oversized)
        .expect("host invokes component");
    assert_ne!(result.status, 0);
    let input = json!({"script":"", "stdin_script":true});
    let result = Harness::<PythonProvider>::get(&component)
        .stdin(vec![b'x'; 65_537])
        .call("python.eval", input)
        .expect("host invokes component");
    assert_ne!(result.status, 0);
    for invalid in [json!({"script":1}), json!({"script":"", "extra":true})] {
        let result = Harness::<PythonProvider>::get(&component)
            .call("python.eval", invalid)
            .expect("invalid input reported");
        assert_ne!(result.status, 0);
    }
}

#[test]
fn sandbox_remains_fresh_and_denies_effectful_builtins() {
    let path = component();
    for code in [
        "result = 'state' in globals()",
        "result = all(name not in __builtins__.__dict__ for name in ['open','input','eval','exec','compile'])",
        "import os",
    ] {
        let result = Harness::<PythonProvider>::get(&path)
            .call("python.eval", json!({"script":code}))
            .expect("component");
        assert_eq!(result.status, 0, "{}", result.stderr);
        let body = answer(&result.stdout);
        if code.contains("import os") {
            assert_eq!(body["ok"], false);
        } else if code.contains("state") {
            assert_eq!(body["result"], false);
        } else {
            assert_eq!(body["result"], true);
        }
    }
}

#[test]
fn repeated_evaluations_on_one_thread_close_guest_introspection_without_poisoning_next_vm() {
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(|| {
            let first = Native::<PythonProvider>::new()
                .call("python.eval", &json!({"script": "result = 41"}).to_string());
            assert_eq!(first.status, 0, "{}", first.stderr);
            assert_eq!(answer(&first.stdout)["result"], 41);
            let attack = Native::<PythonProvider>::new().call(
                "python.eval",
                &json!({"script": "object.__subclasses__()"}).to_string(),
            );
            assert_eq!(attack.status, 0, "{}", attack.stderr);
            assert_eq!(answer(&attack.stdout)["ok"], false);
            assert_eq!(answer(&attack.stdout)["error"]["type"], "AttributeError");
            let healthy = Native::<PythonProvider>::new().call(
                "python.eval",
                &json!({"script": "import json\nresult = json.loads('[42]')"}).to_string(),
            );
            assert_eq!(healthy.status, 0, "{}", healthy.stderr);
            assert_eq!(answer(&healthy.stdout)["result"], json!([42]));
        })
        .unwrap()
        .join()
        .unwrap();
}

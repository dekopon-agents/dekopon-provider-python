//! Every host-level assertion about the built component.
//!
//! The component only fully exists when a real Wasmtime host runs it, so protocol shape, sandbox
//! denials, YAML policy, and host resource termination are all asserted here against
//! [`FakeBroker`] rather than through a command-line host. Each test returns early when
//! `DEKOPON_PYTHON_COMPONENT` is unset: a plain `cargo test` then proves only that the harness
//! compiles, while `scripts/test-broker-testkit.sh` sets the variable after building the ignored
//! artifact and therefore exercises the real broker host.

use std::{path::PathBuf, time::Duration};

use dekopon_provider_sdk_testkit::{
    BrokerHostLimits, CommandRunOutcome, FakeBroker, FakeBrokerError,
};
use serde_json::{Value, json};

fn component() -> Option<PathBuf> {
    std::env::var_os("DEKOPON_PYTHON_COMPONENT").map(PathBuf::from)
}

fn cache_directory() -> Result<PathBuf, std::io::Error> {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("broker-testkit-compile-cache");
    std::fs::create_dir_all(&directory)?;
    directory.canonicalize()
}

fn dedicated_limits() -> BrokerHostLimits {
    BrokerHostLimits {
        fuel: 1_000_000_000,
        max_timeout: Duration::from_secs(5),
        ..BrokerHostLimits::default()
    }
}

/// A broker on the dedicated immediate profile: 64 MiB, 1G fuel, 5 s, 768 KiB of output.
async fn dedicated_broker(component: &PathBuf) -> Result<FakeBroker, FakeBrokerError> {
    FakeBroker::builder()
        .component(component)
        .provider("python")
        .host_limits(BrokerHostLimits {
            max_memory_bytes: 64 * 1024 * 1024,
            ..dedicated_limits()
        })
        .compile_cache(cache_directory()?)
        .timeout_ms(5_000)
        .max_output_bytes(786_432)
        .build()
        .await
}

#[tokio::test(flavor = "multi_thread")]
async fn broker_runs_success_yaml_denial_and_fresh_state() -> Result<(), Box<dyn std::error::Error>>
{
    let Some(component) = component() else {
        return Ok(());
    };
    let broker = FakeBroker::builder()
        .component(component)
        .provider("python")
        .host_limits(dedicated_limits())
        .compile_cache(cache_directory()?)
        .timeout_ms(5_000)
        .max_output_bytes(786_432)
        .build()
        .await?;

    let first = broker
        .invoke(
            "python.eval",
            json!({"script": "import json\nimport re\nimport yaml\nstate = 41\nresult = [json.loads('[1]')[0], re.search('b+', 'abb').group(0), yaml.safe_load('x: 2')['x']]"}),
        )
        .await?;
    assert_eq!(first["ok"], true);
    assert_eq!(first["result"], json!([1, "bb", 2]));

    let second = broker
        .invoke(
            "python.eval",
            json!({"script": "result = 'state' in globals()"}),
        )
        .await?;
    assert_eq!(second["result"], false);

    let denied = broker
        .invoke("python.eval", json!({"script": "import os"}))
        .await?;
    assert_eq!(denied["ok"], false);
    assert_eq!(denied["error"]["type"], "ImportError");

    for _ in 0..3 {
        let output = broker
            .invoke(
                "python.eval",
                json!({"script": r#"
import yaml
original = yaml.YAMLError
assert not hasattr(original, 'marker')
try:
    original.marker = 'leak'
except TypeError:
    pass
else:
    raise AssertionError('mutable class')
class Malicious(original):
    def __new__(cls, *args):
        raise AssertionError('guest constructor')
    def __init__(self, *args):
        raise AssertionError('guest initializer')
for replacement in [original, int, 42, Malicious, None]:
    if replacement is None:
        del yaml.YAMLError
    else:
        yaml.YAMLError = replacement
    try:
        yaml.safe_load('[')
    except original as error:
        assert type(error) is original
    else:
        raise AssertionError('missing error')
result = True
"#}),
            )
            .await?;
        assert_eq!(output["result"], true, "{output}");
    }

    // The host decoded the rebuilt manifest: one capability, the `python` word, and no retired
    // `idempotency` field for the SDK's compatibility decoder to swallow.
    let manifests: Vec<_> = broker.registry().manifests().collect();
    assert_eq!(manifests.len(), 1);
    assert_eq!(manifests[0].id.as_str(), "python");
    assert_eq!(manifests[0].command_words, ["python"]);
    assert_eq!(manifests[0].capabilities.len(), 1);
    assert_eq!(manifests[0].capabilities[0].id.as_str(), "python.eval");
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn broker_projects_the_exact_capability_envelope() -> Result<(), Box<dyn std::error::Error>> {
    let Some(component) = component() else {
        return Ok(());
    };
    let broker = dedicated_broker(&component).await?;

    let success = broker
        .invoke(
            "python.eval",
            json!({"script": "print(\"hello\")\nresult = {\"answer\": 6 * 7}"}),
        )
        .await?;
    assert_eq!(
        success,
        json!({
            "ok": true,
            "stdout": "hello\n",
            "stdoutTruncated": false,
            "result": {"answer": 42},
        })
    );

    let syntax = broker
        .invoke("python.eval", json!({"script": "if:"}))
        .await?;
    assert_eq!(syntax["ok"], false);
    assert_eq!(syntax["error"]["kind"], "syntax");
    assert_eq!(syntax["error"]["type"], "SyntaxError");

    let runtime = broker
        .invoke(
            "python.eval",
            json!({"script": "print(\"before\")\nraise ValueError(\"boom\")"}),
        )
        .await?;
    assert_eq!(runtime["ok"], false);
    assert_eq!(runtime["stdout"], "before\n");
    assert_eq!(
        runtime["error"],
        json!({"kind": "runtime", "type": "ValueError", "message": "boom"})
    );

    let oversized_result = broker
        .invoke("python.eval", json!({"script": "result = \"x\" * 131073"}))
        .await?;
    assert_eq!(oversized_result["ok"], false);
    assert_eq!(oversized_result["error"]["kind"], "result");

    let oversized_yaml = broker
        .invoke(
            "python.eval",
            json!({"script": "import yaml\nresult = yaml.safe_load(\"x\" * 70000)"}),
        )
        .await?;
    assert_eq!(oversized_yaml["ok"], false);
    assert_eq!(oversized_yaml["error"]["kind"], "yaml");

    // Stdout is captured in Rust and bounded there, so a guest writing far past the ceiling is
    // truncated rather than refused.
    let truncated = broker
        .invoke(
            "python.eval",
            json!({"script": "print('\u{e9}' * 40000, end='')\nresult = None"}),
        )
        .await?;
    assert_eq!(truncated["ok"], true);
    assert_eq!(truncated["stdoutTruncated"], true);
    let stdout = truncated["stdout"].as_str().expect("captured stdout");
    assert!(stdout.len() <= 65_536, "{} bytes", stdout.len());

    // A non-object input never reaches the component: the host refuses it, so the failure is
    // not one the provider declared.
    let not_an_object = broker
        .invoke("python.eval", json!(null))
        .await
        .expect_err("non-object input");
    assert!(
        not_an_object.provider_failure().is_none(),
        "{not_an_object:?}"
    );

    // Everything object-shaped is refused by the provider before any VM is constructed.
    for input in [
        json!({}),
        json!({"script": 1}),
        json!({"script": "result = 1", "extra": true}),
    ] {
        let error = broker
            .invoke("python.eval", input)
            .await
            .expect_err("invalid input");
        assert_eq!(
            error.provider_failure().map(|(code, _)| code),
            Some("invalid-input"),
            "{error:?}"
        );
    }

    let oversized_script = broker
        .invoke("python.eval", json!({"script": "x".repeat(65_536 + 1)}))
        .await
        .expect_err("oversized script");
    assert_eq!(
        oversized_script.provider_failure().map(|(code, _)| code),
        Some("input-too-large")
    );

    Ok(())
}

/// The `python` word through the real broker host's `run-command` export: the guest renders help
/// and usage errors itself, `-c` and `-` propose exactly the input a direct call sends, and
/// invoking that proposal closes the loop.
#[tokio::test(flavor = "multi_thread")]
async fn broker_runs_the_python_command_word() -> Result<(), Box<dyn std::error::Error>> {
    let Some(component) = component() else {
        return Ok(());
    };
    let broker = dedicated_broker(&component).await?;
    let argv =
        |words: &[&str]| -> Vec<String> { words.iter().map(|word| (*word).to_owned()).collect() };

    match broker
        .run_command("python", &argv(&["--help"]), None)
        .await?
    {
        CommandRunOutcome::Rendered {
            stdout,
            stderr,
            status: 0,
        } => {
            assert!(
                stdout.contains("Usage: python -c <CODE>\n       python - <<'EOF'"),
                "{stdout}"
            );
            assert!(stderr.is_empty(), "{stderr}");
        }
        other => panic!("expected help at status 0, got {other:?}"),
    }

    // Bare argv needs something actually piped: nothing at all, or an empty pipe, is still a
    // usage error, unlike explicit `-` which accepts an empty piped value as an empty script.
    for stdin in [None, Some("")] {
        match broker.run_command("python", &[], stdin).await? {
            CommandRunOutcome::Rendered {
                stdout, status: 2, ..
            } => assert!(stdout.is_empty(), "{stdin:?}: {stdout}"),
            other => panic!("expected a usage error at status 2 for {stdin:?}, got {other:?}"),
        }
    }

    match broker.run_command("python", &argv(&["-"]), None).await? {
        CommandRunOutcome::Failed { error } => {
            assert_eq!(error.code, "usage");
            assert_eq!(error.message, "python -: nothing was piped in");
        }
        other => panic!("expected a usage decline, got {other:?}"),
    }

    let code = "print(\"hi\")\nresult = 6 * 7";
    let piped = format!("{code}\n");
    for (words, stdin, script) in [
        (&["-c", code][..], None, code),
        (&["-"][..], Some(piped.as_str()), piped.as_str()),
        // Bare argv with something piped is `-` in disguise, matching CPython's own read of a
        // non-tty stdin when given no file.
        (&[][..], Some(piped.as_str()), piped.as_str()),
    ] {
        let CommandRunOutcome::Proposed {
            capability, input, ..
        } = broker.run_command("python", &argv(words), stdin).await?
        else {
            panic!("expected a proposal for {words:?}");
        };
        assert_eq!(capability.as_str(), "python.eval");
        assert_eq!(input, json!({"script": script}));
        let output = broker.invoke(capability.as_str(), input).await?;
        assert_eq!(
            output,
            json!({"ok": true, "stdout": "hi\n", "stdoutTruncated": false, "result": 42})
        );
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn sandbox_denies_modules_builtins_and_every_recovery_path()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(component) = component() else {
        return Ok(());
    };
    let broker = dedicated_broker(&component).await?;
    let script = r#"
import json
import re
import yaml
from json import loads
from re import search
from yaml import safe_load
supported = [
    loads('{"x": 1}')["x"],
    search(r"b+", "abbc").group(0),
    safe_load("x: 2")["x"],
]
private_from_denied = []
try:
    from json import decoder
except ImportError:
    private_from_denied.append(True)
else:
    private_from_denied.append(False)
try:
    from re import _parser
except ImportError:
    private_from_denied.append(True)
else:
    private_from_denied.append(False)
for name, fromlist in (("json", ("decoder",)), ("re", ("_parser",))):
    try:
        __import__(name, fromlist=fromlist)
    except ImportError:
        private_from_denied.append(True)
    else:
        private_from_denied.append(False)
denied = {}
for name in ("sys", "os", "pathlib", "time", "random", "secrets", "socket", "ssl", "sqlite3", "subprocess", "threading", "ctypes", "tkinter", "webbrowser"):
    try:
        __import__(name)
    except ImportError:
        denied[name] = True
    else:
        denied[name] = False
builtins_denied = []
for expression in ("open('x')", "input()", "breakpoint()", "eval('1')", "exec('x=1')", "compile('1', 'x', 'eval')"):
    try:
        if expression.startswith("open"):
            open("x")
        elif expression.startswith("input"):
            input()
        elif expression.startswith("breakpoint"):
            breakpoint()
        elif expression.startswith("eval"):
            eval("1")
        elif expression.startswith("exec"):
            exec("x=1")
        else:
            compile("1", "x", "eval")
    except Exception:
        builtins_denied.append(True)
    else:
        builtins_denied.append(False)

recovery_denied = []
try:
    recovery_denied.append(re.enum.sys.modules.get("_dekopon_policy") is None)
except (AttributeError, KeyError):
    recovery_denied.append(True)
for module in (json, re, re.search.__globals__["_compiler"]):
    recovery_denied.extend([
        not hasattr(module, "__loader__"),
        not hasattr(module, "__spec__"),
    ])
for namespace in (
    re.search.__globals__,
    re.RegexFlag.__new__.__globals__,
    json.loads.__globals__,
    json.JSONDecoder.decode.__globals__,
):
    recovery_denied.extend([
        "sys" not in namespace,
        "_original_import" not in namespace,
        "_original_eval" not in namespace,
        "_original_compile" not in namespace,
    ])
builtins_view = re.search.__globals__["__builtins__"]
if type(builtins_view) is dict:
    recovery_denied.extend(name not in builtins_view for name in ("eval", "exec", "compile", "open"))
else:
    recovery_denied.extend(not hasattr(builtins_view, name) for name in ("eval", "exec", "compile", "open"))
try:
    subclasses = object.__subclasses__()
except AttributeError:
    recovery_denied.append(True)
else:
    recovered = False
    for loader in subclasses:
        if loader.__name__ in ("BuiltinImporter", "FrozenImporter"):
            try:
                loader.load_module("sys")
            except Exception:
                pass
            else:
                recovered = True
    recovery_denied.append(not recovered)
result = {
    "supported": supported,
    "privateFromDenied": private_from_denied,
    "denied": denied,
    "builtinsDenied": builtins_denied,
    "recoveryDenied": recovery_denied,
}
"#;

    let output = broker
        .invoke("python.eval", json!({"script": script}))
        .await?;
    assert_eq!(output["ok"], true, "{output}");
    let result = &output["result"];
    assert_eq!(result["supported"], json!([1, "bb", 2]));
    assert_eq!(result["privateFromDenied"], json!([true, true, true, true]));
    assert!(all_true(&result["denied"]), "{}", result["denied"]);
    assert!(
        all_true(&result["builtinsDenied"]),
        "{}",
        result["builtinsDenied"]
    );
    assert!(
        all_true(&result["recoveryDenied"]),
        "{}",
        result["recoveryDenied"]
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn yaml_policy_rejects_every_unsafe_document() -> Result<(), Box<dyn std::error::Error>> {
    let Some(component) = component() else {
        return Ok(());
    };
    let broker = dedicated_broker(&component).await?;
    let script = r#"
import yaml
cases = {
    "directive": "%YAML 1.2\n---\na: b",
    "anchor": "a: &x [1]\nb: *x",
    "alias": "a: *missing",
    "tag": "a: !thing b",
    "duplicate": "a: 1\na: 2",
    "merge": "<<: value",
    "complexKey": "? [a, b]\n: value",
    "multiple": "---\na: b\n---\nc: d",
    "nonFinite": "a: .inf",
    "integerRange": "a: 9007199254740992",
}
rejected = {}
for name, source in cases.items():
    try:
        yaml.safe_load(source)
    except yaml.YAMLError:
        rejected[name] = True
    else:
        rejected[name] = False
safe = yaml.safe_load("date: 2025-02-03\narray: [null, true, 2.5]")
dumped = yaml.safe_dump({"a": [1, 2], "date": "2025-02-03"})
result = {"rejected": rejected, "safe": safe, "roundTrip": yaml.safe_load(dumped)}
"#;

    let output = broker
        .invoke("python.eval", json!({"script": script}))
        .await?;
    assert_eq!(output["ok"], true, "{output}");
    let result = &output["result"];
    assert!(all_true(&result["rejected"]), "{}", result["rejected"]);
    assert_eq!(
        result["safe"],
        json!({"date": "2025-02-03", "array": [null, true, 2.5]})
    );
    assert_eq!(
        result["roundTrip"],
        json!({"a": [1, 2], "date": "2025-02-03"})
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn broker_terminates_deadline_fuel_and_memory_exhaustion()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(component) = component() else {
        return Ok(());
    };
    let cache = cache_directory()?;

    let deadline = FakeBroker::builder()
        .component(&component)
        .provider("python")
        .host_limits(dedicated_limits())
        .compile_cache(&cache)
        .timeout_ms(50)
        .max_output_bytes(786_432)
        .build()
        .await?;
    let deadline_error = match deadline
        .invoke("python.eval", json!({"script": "while True:\n    pass"}))
        .await
    {
        Ok(value) => panic!("infinite loop unexpectedly succeeded: {value}"),
        Err(error) => error,
    };
    let deadline_detail = deadline_error.to_string().to_ascii_lowercase();
    assert!(
        deadline_detail.contains("deadline")
            || deadline_detail.contains("timeout")
            || deadline_detail.contains("timed out")
            || deadline_detail.contains("exceeded"),
        "{deadline_detail}"
    );

    // The measured startup bracket for this exact build: RustPython never reaches guest code
    // under either probe, and the dedicated 1G profile below runs it comfortably.
    for fuel in [10_000_000_u64, 50_000_000] {
        let low_fuel = FakeBroker::builder()
            .component(&component)
            .provider("python")
            .host_limits(BrokerHostLimits {
                fuel,
                max_timeout: Duration::from_secs(5),
                ..BrokerHostLimits::default()
            })
            .compile_cache(&cache)
            .timeout_ms(5_000)
            .max_output_bytes(786_432)
            .build()
            .await?;
        let fuel_error = match low_fuel
            .invoke("python.eval", json!({"script": "result = 2"}))
            .await
        {
            Ok(value) => panic!("{fuel}-fuel invocation unexpectedly succeeded: {value}"),
            Err(error) => error,
        };
        let fuel_detail = format!("{fuel_error:?}").to_ascii_lowercase();
        assert!(fuel_detail.contains("fuel"), "{fuel_detail}");
    }

    let selected_memory = dedicated_broker(&component).await?;
    let memory_ok = selected_memory
        .invoke("python.eval", json!({"script": "result = 2"}))
        .await?;
    assert_eq!(memory_ok["ok"], true);
    assert_eq!(memory_ok["result"], 2);

    // Python's own recursion limit is a structured guest error under the same profile.
    let recursion = selected_memory
        .invoke("python.eval", json!({"script": "def f(): return f()\nf()"}))
        .await?;
    assert_eq!(recursion["ok"], false);
    assert_eq!(recursion["error"]["type"], "RecursionError");

    let memory_error = match selected_memory
        .invoke(
            "python.eval",
            json!({"script": "result = bytearray(100000000)"}),
        )
        .await
    {
        Ok(value) => panic!("low-memory invocation unexpectedly succeeded: {value}"),
        Err(error) => error,
    };
    assert!(memory_error.provider_failure().is_none());
    let memory_detail = format!("{memory_error:?}").to_ascii_lowercase();
    assert!(
        memory_detail.contains("memory")
            || memory_detail.contains("grow")
            || memory_detail.contains("resource")
            || memory_detail.contains("unreachable"),
        "{memory_detail}"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn adversarial_regex_never_outlives_the_authorization_deadline()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(component) = component() else {
        return Ok(());
    };
    let broker = FakeBroker::builder()
        .component(&component)
        .provider("python")
        .host_limits(BrokerHostLimits {
            max_memory_bytes: 64 * 1024 * 1024,
            fuel: 8_000_000_000,
            max_timeout: Duration::from_secs(5),
            ..BrokerHostLimits::default()
        })
        .compile_cache(cache_directory()?)
        .timeout_ms(250)
        .max_output_bytes(786_432)
        .build()
        .await?;

    // Either the value envelope carries the outcome or the host stops it; both are acceptable,
    // running past the 250 ms authorization-equivalent deadline is not.
    match broker
        .invoke(
            "python.eval",
            json!({"script": "import re\nresult = bool(re.search(\"(a+)+$\", \"a\" * 20000 + \"!\"))"}),
        )
        .await
    {
        Ok(value) => assert!(value["ok"].is_boolean(), "{value}"),
        Err(error) => {
            let detail = format!("{error:?}").to_ascii_lowercase();
            assert!(
                detail.contains("deadline")
                    || detail.contains("timeout")
                    || detail.contains("timed out")
                    || detail.contains("exceeded")
                    || detail.contains("fuel")
                    || detail.contains("memory")
                    || detail.contains("resource"),
                "{detail}"
            );
        }
    }
    Ok(())
}

fn all_true(value: &Value) -> bool {
    match value {
        Value::Array(items) => items.iter().all(|item| item == &Value::Bool(true)),
        Value::Object(fields) => fields.values().all(|item| item == &Value::Bool(true)),
        _ => false,
    }
}

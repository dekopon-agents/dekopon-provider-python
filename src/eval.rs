use rustpython_vm::{AsObject, Interpreter, Settings, VirtualMachine, compiler::Mode};
use serde::Serialize;
use serde_json::{Value, json};

use crate::{
    capture,
    limits::{DIAGNOSTIC_BYTES, HASH_SEED, PYTHON_RECURSION, RESPONSE_BYTES},
    policy,
    value::py_to_safe_json,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Success {
    ok: bool,
    stdout: String,
    stdout_truncated: bool,
    result: Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Failure {
    ok: bool,
    stdout: String,
    stdout_truncated: bool,
    error: ErrorDetail,
}

#[derive(Clone, Debug, Serialize)]
struct ErrorDetail {
    kind: String,
    #[serde(rename = "type")]
    type_name: String,
    message: String,
}

impl ErrorDetail {
    fn new(kind: &str, type_name: &str, message: impl AsRef<str>) -> Self {
        Self {
            kind: kind.to_owned(),
            type_name: bounded_text(type_name, DIAGNOSTIC_BYTES),
            message: bounded_text(message.as_ref(), DIAGNOSTIC_BYTES),
        }
    }
}

pub(crate) fn evaluate(script: &str) -> Value {
    evaluate_with_sql(script, false)
}

pub(crate) fn evaluate_sql(script: &str) -> Value {
    evaluate_with_sql(script, true)
}

fn evaluate_with_sql(script: &str, sql: bool) -> Value {
    capture::reset();
    let interpreter = interpreter_with_sql(sql);
    let execution = interpreter.enter(|vm| execute(script, vm, sql));
    let captured = capture::snapshot();
    let value = match execution {
        Ok(result) => serde_json::to_value(Success {
            ok: true,
            stdout: captured.stdout,
            stdout_truncated: captured.truncated,
            result,
        }),
        Err(error) => serde_json::to_value(Failure {
            ok: false,
            stdout: captured.stdout,
            stdout_truncated: captured.truncated,
            error,
        }),
    }
    .unwrap_or_else(|_error| response_serialization_failure());
    enforce_response_limit(value)
}

#[cfg(test)]
pub(crate) fn interpreter() -> Interpreter {
    interpreter_with_sql(false)
}

fn interpreter_with_sql(sql: bool) -> Interpreter {
    let mut settings = Settings::default();
    settings.isolated = true;
    settings.ignore_environment = true;
    settings.install_signal_handlers = false;
    settings.hash_seed = Some(HASH_SEED);
    settings.import_site = false;
    settings.user_site_directory = false;
    settings.write_bytecode = false;
    settings.safe_path = true;
    settings.allow_external_library = false;
    settings.path_list.clear();
    settings.argv = vec!["<python.eval>".to_owned()];
    settings.int_max_str_digits = 4_300;

    let builder = Interpreter::builder(settings);
    let mut definitions = rustpython_stdlib::stdlib_module_defs(&builder.ctx);
    definitions.extend([
        crate::capture::stdout_module::module_def(&builder.ctx),
        crate::capture::stderr_module::module_def(&builder.ctx),
        crate::policy::policy_module::module_def(&builder.ctx),
        crate::numeric::numeric_module::module_def(&builder.ctx),
        crate::time::time_module::module_def(&builder.ctx),
        crate::entropy::entropy_module::module_def(&builder.ctx),
        crate::random::module_def(&builder.ctx),
        crate::yaml::yaml_module::module_def(&builder.ctx),
    ]);
    if sql {
        definitions.push(crate::sqlite::sqlite_module::module_def(&builder.ctx));
    }
    #[cfg(feature = "tables")]
    definitions.push(crate::tables::tables_module::module_def(&builder.ctx));
    #[cfg(feature = "http")]
    definitions.extend([
        crate::requests::requests_module::module_def(&builder.ctx),
        crate::subshell::subshell_module::module_def(&builder.ctx),
    ]);
    builder
        .add_native_modules(&definitions)
        .init_hook(|vm| {
            // Resolve time through our frozen UTC facade, never the ambient native module.
            rustpython_common::rc::PyRc::get_mut(&mut vm.state)
                .expect("exclusive VM initialization")
                .module_defs
                .remove("time");
        })
        .add_frozen_modules(rustpython_pylib::FROZEN_STDLIB)
        .add_frozen_modules(include!(concat!(env!("OUT_DIR"), "/invocation_frozen.rs")))
        .build()
}

fn execute(script: &str, vm: &VirtualMachine, sql: bool) -> Result<Value, ErrorDetail> {
    vm.recursion_limit.set(PYTHON_RECURSION);
    capture::install(vm).map_err(|error| python_error(error, vm, "runtime"))?;
    let _closed = policy::install(vm, sql).map_err(|error| python_error(error, vm, "runtime"))?;

    let scope = vm.new_scope_with_builtins();
    scope
        .globals
        .set_item("result", vm.ctx.none(), vm)
        .map_err(|error| python_error(error, vm, "runtime"))?;
    let code = vm
        .compile(script, Mode::Exec, "<python.eval>".to_owned())
        .map_err(|error| ErrorDetail::new("syntax", "SyntaxError", error.to_string()))?;
    vm.run_code_obj(code, scope.clone())
        .map_err(|error| python_error(error, vm, "runtime"))?;
    let result = scope
        .globals
        .get_item("result", vm)
        .map_err(|error| python_error(error, vm, "runtime"))?;
    py_to_safe_json(&result, vm)
        .map_err(|error| ErrorDetail::new("result", "ResultError", error.message()))
}

fn python_error(
    exception: rustpython_vm::builtins::PyBaseExceptionRef,
    vm: &VirtualMachine,
    default_kind: &str,
) -> ErrorDetail {
    let type_name = exception.class().name().to_string();
    let kind = if exception
        .class()
        .fast_issubclass(vm.ctx.exceptions.syntax_error)
    {
        "syntax"
    } else if type_name == "YAMLError" {
        "yaml"
    } else {
        default_kind
    };
    let args = exception.args();
    let message = args
        .as_slice()
        .first()
        .filter(|value| value.class().is(vm.ctx.types.str_type))
        .and_then(|value| value.downcast_ref::<rustpython_vm::builtins::PyStr>())
        .and_then(|value| value.to_str())
        .unwrap_or("Python execution failed");
    ErrorDetail::new(kind, &type_name, message)
}

fn enforce_response_limit(value: Value) -> Value {
    match serde_json::to_vec(&value) {
        Ok(encoded) if encoded.len() <= RESPONSE_BYTES => value,
        _ => json!({
            "ok": false,
            "stdout": "",
            "stdoutTruncated": false,
            "error": {
                "kind": "result",
                "type": "ResponseError",
                "message": format!("complete response exceeds {RESPONSE_BYTES} bytes")
            }
        }),
    }
}

fn response_serialization_failure() -> Value {
    json!({
        "ok": false,
        "stdout": "",
        "stdoutTruncated": false,
        "error": {
            "kind": "result",
            "type": "ResponseError",
            "message": "response could not be serialized"
        }
    })
}

fn bounded_text(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_owned();
    }
    let mut boundary = limit;
    while boundary > 0 && !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value[..boundary].to_owned()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::evaluate as evaluate_on_current_thread;
    use crate::limits::{DIAGNOSTIC_BYTES, MAX_SAFE_INTEGER, STDOUT_BYTES};

    fn evaluate(script: &str) -> serde_json::Value {
        let script = script.to_owned();
        on_python_thread(move || evaluate_on_current_thread(&script))
    }

    fn on_python_thread<R: Send + 'static>(f: impl FnOnce() -> R + Send + 'static) -> R {
        std::thread::Builder::new()
            .name("rustpython-test".to_owned())
            .stack_size(32 * 1024 * 1024)
            .spawn(f)
            .expect("spawn RustPython test thread")
            .join()
            .expect("RustPython test thread")
    }

    fn native(script: &str, entropy: Vec<u8>) -> (serde_json::Value, usize) {
        use dekopon_provider_sdk_testkit::Native;
        crate::entropy::READS.with(|r| r.set(0));
        let output = Native::<crate::PythonProvider>::new()
            .entropy(entropy)
            .clock(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000))
            .call("python.eval", &json!({"script": script}).to_string());
        assert_eq!(output.status, 0, "{}", output.stderr);
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["ok"], true, "{value}");
        (
            value["result"].clone(),
            crate::entropy::READS.with(|r| r.get()),
        )
    }

    #[test]
    fn random_install_import_command_and_describe_read_no_entropy() {
        on_python_thread(|| {
            let (_, reads) = native("import random", vec![]);
            assert_eq!(reads, 0);
            let (_, reads) = native("", vec![]);
            assert_eq!(reads, 0);
            dekopon_provider_sdk::provider::manifest::<crate::PythonProvider>().unwrap();
            let proposed = dekopon_provider_sdk::provider::command::<crate::PythonProvider>(
                &["-c".into(), "import random".into()],
                false,
            );
            assert!(matches!(
                proposed,
                dekopon_provider_sdk::CommandRunOutcome::Proposed { .. }
            ));
            assert_eq!(crate::entropy::READS.with(|r| r.get()), 0);
        });
    }

    #[test]
    fn random_is_lazy_and_integer_seeds_repeat_without_entropy() {
        on_python_thread(|| {
            let script = "import random\nresult=[str(random.getrandbits(64)), random.random(), random.randrange(100)]";
            let (first, reads) = native(script, vec![1; 32]);
            assert_eq!(reads, 1);
            let (second, reads) = native(script, vec![2; 32]);
            assert_eq!(reads, 1);
            assert_ne!(first, second);
            let script = "import random\nrandom.seed(7)\nresult=[str(random.getrandbits(64)), random.random()]";
            let (first, reads) = native(script, vec![]);
            assert_eq!(reads, 0);
            let (second, reads) = native(script, vec![]);
            assert_eq!(reads, 0);
            assert_eq!(first, second);
        });
    }

    #[test]
    fn secrets_system_random_and_urandom_each_read_the_broker() {
        on_python_thread(|| {
            let (result, reads) = native(
                "import secrets, random\nresult=[secrets.token_bytes(16).hex(), secrets.token_bytes(16).hex(), random._urandom(3).hex(), random.SystemRandom().random()]",
                (0..42).collect(),
            );
            assert_eq!(reads, 4);
            assert_ne!(result[0], result[1]);
            assert_eq!(result[2], "202122");
            assert!(result[3].as_f64().is_some_and(|n| n > 0.0 && n < 1.0));
        });
    }

    #[test]
    fn new_modules_expose_no_os_sys_or_hash_attributes_and_refuse_byte_seeds() {
        let result = evaluate(
            "import random, secrets, datetime, statistics\nresult=[hasattr(random, '_os'), hasattr(random, '_sha512'), hasattr(secrets, 'compare_digest'), hasattr(statistics, 'sys'), hasattr(datetime.datetime.now.__func__, '__globals__') and 'sys' in datetime.datetime.now.__func__.__globals__]",
        );
        assert_eq!(
            result["result"],
            json!([false, false, false, false, false]),
            "{result}"
        );
        for seed in ["'text'", "b'bytes'", "bytearray(b'bytes')"] {
            let output = evaluate(&format!("import random\nrandom.seed({seed})"));
            assert_eq!(output["error"]["type"], "TypeError", "{output}");
            assert!(
                output["error"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("no hash primitives")
            );
        }
    }

    #[test]
    fn entropy_and_datetime_fail_closed_outside_invocation() {
        for script in [
            "import random\nrandom.random()",
            "import random\nrandom.seed(7)\nrandom.random()",
            "import random\nrandom._random.Random().random()",
            "import random\nrandom.SystemRandom().random()",
            "import random\nrandom._urandom(1)",
            "import secrets\nsecrets.token_bytes(16)",
            "import datetime\ndatetime.datetime.now()",
            "import datetime\ndatetime.date.today()",
            "import time\ntime.localtime()",
            "import time\ntime.strftime('%Y')",
        ] {
            let output = evaluate(script);
            assert_eq!(
                output["error"]["type"], "RuntimeError",
                "{script}: {output}"
            );
            assert!(
                output["error"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("unavailable outside invocation"),
                "{output}"
            );
        }
    }

    #[test]
    fn datetime_uses_invocation_utc_and_statistics_works() {
        on_python_thread(|| {
            let (result, reads) = native(
                "import datetime as d, statistics as s, time\nn=d.datetime.now()\nresult=[n.isoformat(), str(d.date.today()), n.tzinfo is None, s.mean([1,2,3,4]), s.median([4,1,3,2]), time.strftime('%Y-%m-%d %H:%M:%S %Z', time.gmtime(0)), str(n.astimezone().tzinfo)]",
                vec![],
            );
            assert_eq!(reads, 0);
            assert_eq!(
                result,
                json!([
                    "2023-11-14T22:13:20",
                    "2023-11-14",
                    true,
                    2.5,
                    2.5,
                    "1970-01-01 00:00:00 UTC",
                    "UTC"
                ])
            );
        });
    }

    #[test]
    fn clocks_refuse_outside_invocation_instead_of_reading_during_metadata() {
        let output = evaluate("import time\nresult=time.time()");
        assert_eq!(output["error"]["type"], "RuntimeError", "{output}");
        assert_eq!(
            output["error"]["message"],
            "clock unavailable outside invocation"
        );
        let output = evaluate("import time\nresult=time.monotonic()");
        assert_eq!(output["error"]["type"], "RuntimeError", "{output}");
    }

    #[test]
    fn requests_import_matches_the_compiled_authority_contract() {
        let output = evaluate(
            "import dekopon_requests as requests\nresult = [hasattr(requests, 'get'), hasattr(requests, 'head'), hasattr(requests, 'post'), issubclass(requests.HTTPError, requests.RequestException)]",
        );
        #[cfg(feature = "http")]
        assert_eq!(
            output["result"],
            json!([true, true, false, true]),
            "{output}"
        );
        #[cfg(not(feature = "http"))]
        assert_eq!(output["error"]["type"], "ImportError", "{output}");
    }

    #[test]
    fn evaluates_python_three_with_supported_modules_and_native_yaml() {
        let output = evaluate(
            r#"
import json
import re
import yaml
print("héllo")
document = yaml.safe_load("name: dekopon\nwhen: 2025-02-03")
result = {
    "sum": sum(i * i for i in range(5)),
    "json": json.loads('{"ok": true}'),
    "match": re.search(r"d.ko", document["name"]).group(0),
    "yaml": document,
    "dumped": yaml.safe_dump({"a": [1, 2]}),
}
"#,
        );
        assert_eq!(output["ok"], true, "{output}");
        assert_eq!(output["stdout"], "héllo\n");
        assert_eq!(output["stdoutTruncated"], false);
        assert_eq!(output["result"]["sum"], 30);
        assert_eq!(output["result"]["json"], json!({"ok": true}));
        assert_eq!(output["result"]["match"], "deko");
        assert_eq!(output["result"]["yaml"]["when"], "2025-02-03");
    }

    #[test]
    fn isolates_state_and_uses_a_deterministic_hash_policy() {
        let first = evaluate("state = 41\nresult = hash('dekopon') % 1000000");
        let second = evaluate("result = ('state' in globals(), hash('dekopon') % 1000000)");
        assert_eq!(first["ok"], true);
        assert_eq!(second["result"][0], false);
        assert_eq!(second["result"][1], first["result"]);
    }

    #[test]
    fn denies_ambient_modules_and_removed_builtins() {
        for script in [
            "import os",
            "import sys",
            "import hashlib",
            "import hmac",
            "import ssl",
            "import socket",
            "import subprocess",
            "import ctypes",
            "import re._parser",
            "import json.decoder",
            "open('/etc/passwd')",
            "input()",
            "breakpoint()",
            "eval('1 + 1')",
            "exec('result = 1')",
            "compile('1', 'guest', 'eval')",
        ] {
            let output = evaluate(script);
            assert_eq!(output["ok"], false, "{script}: {output}");
            assert_eq!(output["error"]["kind"], "runtime", "{script}: {output}");
        }
    }

    #[test]
    fn denies_privileged_callable_and_module_recovery_by_introspection() {
        let output = evaluate(
            r#"
import json
import re
checks = []

# The reviewed exploit must not reach enum.sys.modules or the policy module.
try:
    checks.append(re.enum.sys.modules.get("_dekopon_policy") is None)
except (AttributeError, KeyError):
    checks.append(True)

# Module metadata must not lead back to frozen importlib loaders, and function globals and
# transitive modules must not retain denied module objects.
for module in (json, re, re.search.__globals__["_compiler"]):
    checks.append(not hasattr(module, "__loader__"))
    checks.append(not hasattr(module, "__spec__"))
for namespace in (
    re.search.__globals__,
    re.RegexFlag.__new__.__globals__,
    json.loads.__globals__,
    json.JSONDecoder.decode.__globals__,
):
    checks.append("sys" not in namespace)
    checks.append("_original_import" not in namespace)
    checks.append("_original_eval" not in namespace)
    checks.append("_original_compile" not in namespace)

builtins_view = re.search.__globals__["__builtins__"]
if type(builtins_view) is dict:
    checks.extend(name not in builtins_view for name in ("eval", "exec", "compile", "open"))
else:
    checks.extend(not hasattr(builtins_view, name) for name in ("eval", "exec", "compile", "open"))

# Even a residual reference to sys must see only the closed public registry, and importlib loader
# classes must not be recoverable through the classic object-subclass traversal.
flag_globals = re.RegexFlag.__new__.__globals__
checks.append(flag_globals.get("sys") is None)
try:
    subclasses = object.__subclasses__()
except AttributeError:
    checks.append(True)
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
    checks.append(not recovered)
result = checks
"#,
        );
        assert_eq!(output["ok"], true, "{output}");
        let checks = output["result"].as_array().expect("check array");
        assert!(!checks.is_empty());
        assert!(checks.iter().all(|check| check == true), "{output}");
    }

    #[test]
    fn returns_bounded_structured_syntax_runtime_yaml_and_result_errors() {
        let cases = [
            ("if:", "syntax", "SyntaxError"),
            ("raise RuntimeError('boom')", "runtime", "RuntimeError"),
            (
                "import yaml\nresult = yaml.safe_load('a: &x [1]\\nb: *x')",
                "yaml",
                "YAMLError",
            ),
            ("result = 10 ** 100", "result", "ResultError"),
            ("result = float('nan')", "result", "ResultError"),
            ("result = {1: 'bad'}", "result", "ResultError"),
            ("result = object()", "result", "ResultError"),
            (
                "result = []\nresult.append(result)",
                "result",
                "ResultError",
            ),
            (
                "result = None\nfor _ in range(33): result = [result]",
                "result",
                "ResultError",
            ),
            ("result = [None] * 10001", "result", "ResultError"),
            ("result = 'x' * 131073", "result", "ResultError"),
            (
                "class Evil:\n    def __iter__(self):\n        print('conversion hook ran')\n        return iter([])\nresult = Evil()",
                "result",
                "ResultError",
            ),
            (
                "class EvilList(list): pass\nresult = EvilList([1])",
                "result",
                "ResultError",
            ),
        ];
        for (script, kind, type_name) in cases {
            let output = evaluate(script);
            assert_eq!(output["ok"], false, "{script}: {output}");
            assert_eq!(output["error"]["kind"], kind, "{script}: {output}");
            assert_eq!(output["error"]["type"], type_name, "{script}: {output}");
            assert!(
                output["error"]["message"].as_str().expect("message").len() <= DIAGNOSTIC_BYTES
            );
        }
        let boundary = evaluate(&format!("result = {}", MAX_SAFE_INTEGER));
        assert_eq!(boundary["result"], MAX_SAFE_INTEGER);
    }

    #[test]
    fn truncates_stdout_natively_without_splitting_utf8() {
        let output = evaluate(&format!("print({:?}, end='')", "é".repeat(STDOUT_BYTES)));
        assert_eq!(output["ok"], true);
        assert_eq!(output["stdoutTruncated"], true);
        let stdout = output["stdout"].as_str().expect("stdout string");
        assert!(stdout.len() <= STDOUT_BYTES);
        assert!(stdout.is_char_boundary(stdout.len()));
    }
}

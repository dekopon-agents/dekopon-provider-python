//! Behaviour and public inventory observed through the built component, never Native read counts.
use dekopon_provider_sdk_testkit::Harness;
use dekopon_python_provider::PythonProvider;
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    time::{Duration, UNIX_EPOCH},
};

fn eval(capability: &str, script: &str) -> Value {
    let component = PathBuf::from(
        std::env::var_os("DEKOPON_PROVIDER_COMPONENT").expect("built component path"),
    );
    let output = Harness::<PythonProvider>::get(component)
        .clock(UNIX_EPOCH + Duration::from_secs(1_700_000_000))
        .call(capability, json!({"script": script}))
        .expect("component invocation");
    assert_eq!(output.status, 0, "{}", output.stderr);
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["ok"], true, "{script}: {result}");
    result["result"].clone()
}

#[test]
fn component_random_secrets_datetime_and_statistics_work_in_eval() {
    let capability = "python.eval";
    let random = "import random\nresult=str(random.getrandbits(64))";
    assert_ne!(eval(capability, random), eval(capability, random));
    let secrets = "import secrets\nresult=secrets.token_bytes(16).hex()";
    assert_ne!(eval(capability, secrets), eval(capability, secrets));
    let seeded =
        "import random\nrandom.seed(7)\nresult=[str(random.getrandbits(64)), random.random()]";
    assert_eq!(eval(capability, seeded), eval(capability, seeded));
    let result = eval(
        capability,
        "import datetime as d, statistics as s\nn=d.datetime.now(d.UTC)\nresult=[n.timestamp(), str(d.date.today()), s.mean([1,2,3,4]), s.median([4,1,3,2])]",
    );
    assert!(
        (result[0].as_f64().unwrap() - 1_700_000_000.0).abs() < 1.0,
        "{result}"
    );
    assert_eq!(result[1], "2023-11-14");
    assert_eq!(result[2], 2.5);
    assert_eq!(result[3], 2.5);
}

#[test]
fn component_public_module_inventory_matches_fixture() {
    let names = eval(
        "python.eval",
        r#"
import random, secrets, datetime, statistics, time
result = {}
for module in [random, secrets, datetime, statistics, time]:
    result[module.__name__] = [name for name in dir(module) if not name.startswith('_')]
"#,
    );
    let mut added_time_names = serde_json::Map::new();
    for name in names["time"].as_array().unwrap() {
        let name = name.as_str().unwrap();
        let access = match name {
            "time" | "monotonic" => continue, // Existing facade, not additions.
            "gmtime" | "localtime" => "clock read only when seconds is omitted or None",
            "strftime" => "clock read only when tuple is omitted or None",
            "struct_time" => "no clock read",
            other => panic!("classify added time name {other}"),
        };
        added_time_names.insert(name.into(), json!(access));
    }
    let inventory = json!({"public_names": names, "added_time_names": added_time_names});
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/stats-datetime-inventory.json");
    if std::env::var_os("UPDATE_PYTHON_INVENTORY").is_some() {
        std::fs::write(
            &fixture,
            format!("{}\n", serde_json::to_string_pretty(&inventory).unwrap()),
        )
        .unwrap();
    }
    let expected: Value =
        serde_json::from_slice(&std::fs::read(fixture).expect("test-generated inventory")).unwrap();
    assert_eq!(
        inventory, expected,
        "regenerate with UPDATE_PYTHON_INVENTORY=1 against the built component"
    );
}

#[test]
fn component_survives_oversized_entropy_requests_and_keeps_drawing() {
    let result = eval(
        "python.eval",
        r#"import random, secrets
errors = []
for draw in [lambda: secrets.token_bytes(10**9), lambda: random._urandom(2**31-1),
             lambda: random.getrandbits(2**31-1), lambda: secrets.token_bytes(2**63-1)]:
    try:
        draw()
        errors.append(None)
    except (ValueError, OverflowError) as error:
        errors.append(type(error).__name__)
result = [errors, len(secrets.token_bytes(1048576)), len(secrets.token_bytes(16)),
          random.getrandbits(8388608) >= 0]
"#,
    );
    assert_eq!(
        result,
        json!([
            // 2**63-1 already overflows the 32-bit Wasm isize argument.
            ["ValueError", "ValueError", "ValueError", "OverflowError"],
            1_048_576,
            16,
            true
        ])
    );
}

#[test]
fn component_sequence_and_mapping_checks_work_for_scripts() {
    let result = eval(
        "python.eval",
        "import random, statistics, collections\nresult=[random.sample([1,2,3], 1)[0] in (1,2,3), statistics.mode([1,1,2]), dict(collections.Counter('aab'))]",
    );
    assert_eq!(result, json!([true, 1, {"a": 2, "b": 1}]));
}

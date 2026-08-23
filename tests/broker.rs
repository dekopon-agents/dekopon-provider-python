use std::{path::PathBuf, time::Duration};

use dekopon_provider_sdk_testkit::{BrokerHostLimits, FakeBroker};
use serde_json::json;

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

#[tokio::test(flavor = "multi_thread")]
async fn broker_runs_success_yaml_denial_and_fresh_state() -> Result<(), Box<dyn std::error::Error>>
{
    let Some(component) = component() else {
        // `cargo test` validates that the harness compiles. The component gate sets the variable
        // after building the ignored artifact and therefore exercises the real broker host.
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

    let stats = broker.registry().metrics().snapshot();
    assert!(stats.fuel_observations >= 3);
    assert!(stats.fuel_consumed > 0);
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
            || deadline_detail.contains("timed out")
            || deadline_detail.contains("exceeded"),
        "{deadline_detail}"
    );

    let low_fuel_limits = BrokerHostLimits {
        fuel: 10_000_000,
        max_timeout: Duration::from_secs(5),
        ..BrokerHostLimits::default()
    };
    let low_fuel = FakeBroker::builder()
        .component(&component)
        .provider("python")
        .host_limits(low_fuel_limits)
        .compile_cache(&cache)
        .timeout_ms(5_000)
        .max_output_bytes(786_432)
        .build()
        .await?;
    let fuel_error = match low_fuel
        .invoke("python.eval", json!({"script": "result = 2"}))
        .await
    {
        Ok(value) => panic!("low-fuel invocation unexpectedly succeeded: {value}"),
        Err(error) => error,
    };
    let fuel_detail = format!("{fuel_error:?}").to_ascii_lowercase();
    assert!(fuel_detail.contains("fuel"), "{fuel_detail}");

    let low_memory_limits = BrokerHostLimits {
        max_memory_bytes: 16 * 1024 * 1024,
        fuel: 1_000_000_000,
        max_timeout: Duration::from_secs(5),
        ..BrokerHostLimits::default()
    };
    let low_memory = FakeBroker::builder()
        .component(component)
        .provider("python")
        .host_limits(low_memory_limits)
        .compile_cache(cache)
        .timeout_ms(5_000)
        .max_output_bytes(786_432)
        .build()
        .await?;
    let memory_error = match low_memory
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

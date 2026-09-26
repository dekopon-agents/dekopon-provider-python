#![cfg(feature = "engine-swap")]
//! Exercises the composed component through the real SDK test host, not a mock WIT call.
use std::{path::PathBuf, process::Command, time::Duration};

use dekopon_provider_sdk_testkit::{BrokerHostLimits, FakeBroker};
use serde_json::{Value, json};

fn required_path(name: &str) -> PathBuf {
    let path = PathBuf::from(std::env::var_os(name).unwrap_or_else(|| panic!("missing {name}")));
    assert!(path.is_file(), "{} is not a file", path.display());
    path
}

fn limits() -> BrokerHostLimits {
    BrokerHostLimits {
        max_memory_bytes: 64 * 1024 * 1024,
        fuel: 1_000_000_000,
        max_timeout: Duration::from_secs(5),
        ..BrokerHostLimits::default()
    }
}

async fn broker(
    path: &PathBuf,
    limits: BrokerHostLimits,
) -> Result<FakeBroker, Box<dyn std::error::Error>> {
    Ok(FakeBroker::builder()
        .component(path)
        .provider("python")
        .host_limits(limits)
        .timeout_ms(5_000)
        .max_output_bytes(786_432)
        .build()
        .await?)
}

async fn script(broker: &FakeBroker, source: &str) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(broker
        .invoke("python.eval", json!({"script": source}))
        .await?)
}

#[tokio::test(flavor = "multi_thread")]
async fn separately_composed_engines_run_the_same_python_facade()
-> Result<(), Box<dyn std::error::Error>> {
    for (name, path) in [
        ("engine-a", required_path("DEKOPON_ENGINE_A")),
        ("engine-b", required_path("DEKOPON_ENGINE_B")),
    ] {
        let host = broker(&path, limits()).await?;
        let first = script(&host, "import dekopon_engine as engine\nstate = 42\nresult = [engine.engine_name(), engine.probe()]").await?;
        assert_eq!(first["ok"], true, "{name}: {first}");
        assert_eq!(
            first["result"],
            json!([name, [name, 0, 1, 0]]),
            "{name}: {first}"
        );
        let second = script(
            &host,
            "import dekopon_engine as engine\nresult = ['state' in globals(), engine.probe()]",
        )
        .await?;
        assert_eq!(
            second["result"],
            json!([false, [name, 0, 1, 0]]),
            "{name}: {second}"
        );
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn engine_guest_work_is_host_bounded() -> Result<(), Box<dyn std::error::Error>> {
    let path = required_path("DEKOPON_ENGINE_A");
    let host = broker(&path, limits()).await?;
    let small = script(
        &host,
        "import dekopon_engine as engine\nresult = [engine.burn(100), engine.allocate(1024)]",
    )
    .await?;
    assert_eq!(small["result"], json!([100, 1024]), "{small}");

    let low_fuel = broker(
        &path,
        BrokerHostLimits {
            fuel: 500_000_000,
            ..limits()
        },
    )
    .await?;
    let low_fuel_control = script(&low_fuel, "result = 2").await?;
    assert_eq!(low_fuel_control["result"], 2, "{low_fuel_control}");
    let fuel_error = low_fuel
        .invoke(
            "python.eval",
            json!({"script": "import dekopon_engine as engine\nresult = engine.burn(100000000)"}),
        )
        .await
        .expect_err("engine loop must exceed fuel");
    assert!(
        format!("{fuel_error:?}")
            .to_ascii_lowercase()
            .contains("fuel"),
        "{fuel_error:?}"
    );

    let memory_error = host.invoke("python.eval", json!({"script": "import dekopon_engine as engine\nresult = engine.allocate(100000000)"})).await.expect_err("engine allocation must exceed memory limit");
    let detail = format!("{memory_error:?}").to_ascii_lowercase();
    assert!(
        detail.contains("memory")
            || detail.contains("grow")
            || detail.contains("resource")
            || detail.contains("unreachable"),
        "{detail}"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn raw_facade_cannot_load_without_engine_link() -> Result<(), Box<dyn std::error::Error>> {
    let path = required_path("DEKOPON_ENGINE_RAW");
    match broker(&path, limits()).await {
        Err(error) => {
            let detail = format!("{error:?}").to_ascii_lowercase();
            assert!(
                detail.contains("engine") || detail.contains("import"),
                "{detail}"
            );
        }
        Ok(host) => {
            let error = host
                .invoke("python.eval", json!({"script": "result = 2"}))
                .await
                .expect_err("unlinked engine import must be refused");
            let detail = format!("{error:?}").to_ascii_lowercase();
            assert!(
                detail.contains("engine") || detail.contains("import"),
                "{detail}"
            );
        }
    }
    Ok(())
}

#[test]
fn compositions_preserve_exact_shipped_external_contract() {
    for name in ["DEKOPON_ENGINE_A", "DEKOPON_ENGINE_B"] {
        let path = required_path(name);
        let output = Command::new("bash")
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/component_contract/assert-component-contract.sh"
            ))
            .arg(&path)
            .output()
            .expect("contract checker");
        assert!(
            output.status.success(),
            "{name}: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

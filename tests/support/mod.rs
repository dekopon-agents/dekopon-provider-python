//! One cold compiled-component cache per integration-test binary, not per broker or cargo run.

use std::path::PathBuf;

use dekopon_provider_sdk_testkit::{FakeBroker, FakeBrokerBuilder, FakeBrokerError};
use tempfile::TempDir;
use tokio::sync::Mutex;

// The static retains the private directory until process exit: never remove or replace artifacts
// while a broker might still map them. They live under target/ for ordinary artifact cleanup.
static CACHE: Mutex<Option<TempDir>> = Mutex::const_new(None);

pub async fn build_broker(builder: FakeBrokerBuilder) -> Result<FakeBroker, FakeBrokerError> {
    let mut cache = CACHE.lock().await;
    if cache.is_none() {
        let root =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/broker-testkit-compile-cache");
        std::fs::create_dir_all(&root)?;
        *cache = Some(
            tempfile::Builder::new()
                .prefix("binary-")
                .tempdir_in(root)?,
        );
    }
    let directory = cache
        .as_ref()
        .expect("cache initialized")
        .path()
        .canonicalize()?;

    // Separate registries do not coordinate cold publication; racing publishers fail closed.
    // Hold the async lock through loading only. Every test keeps its own broker/limits, and all
    // invocations still run in parallel after this returns. A warm load verifies immutable bytes.
    builder.compile_cache(directory).build().await
}

#[tokio::test(flavor = "multi_thread")]
async fn parallel_brokers_reuse_one_immutable_compilation() -> Result<(), Box<dyn std::error::Error>>
{
    let component = std::env::var_os("DEKOPON_PROVIDER_COMPONENT")
        .expect("DEKOPON_PROVIDER_COMPONENT must point at the built component");
    let builder = || {
        FakeBroker::builder()
            .component(PathBuf::from(&component))
            .provider("python")
    };
    let (first, second) = tokio::join!(build_broker(builder()), build_broker(builder()));
    let (_first, _second) = (first?, second?);
    let objects = CACHE
        .lock()
        .await
        .as_ref()
        .expect("cache initialized")
        .path()
        .join("v1/sha256");
    let snapshot = || -> Result<_, std::io::Error> {
        let mut entries = std::fs::read_dir(&objects)?
            .map(|entry| {
                let entry = entry?;
                let metadata = entry.metadata()?;
                Ok((entry.file_name(), metadata.len(), metadata.modified()?))
            })
            .collect::<Result<Vec<_>, std::io::Error>>()?;
        entries.sort();
        Ok(entries)
    };
    let before = snapshot()?;
    assert_eq!(before.len(), 1, "one compiled component per binary");
    let _third = build_broker(builder().host_limits(
        dekopon_provider_sdk_testkit::BrokerHostLimits {
            max_memory_bytes: 64 * 1024 * 1024,
            fuel: 50_000_000,
            ..Default::default()
        },
    ))
    .await?;
    assert_eq!(
        snapshot()?,
        before,
        "a warm load must not republish mapped code"
    );
    Ok(())
}

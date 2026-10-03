//! Host deadline terminates adversarial Python work; one registry compile in this test binary.
use dekopon_provider_sdk_testkit::{BrokerHostLimits, Harness};
use dekopon_python_provider::PythonProvider;
use serde_json::json;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

fn component() -> PathBuf {
    std::env::var_os("DEKOPON_PROVIDER_COMPONENT")
        .expect("fresh component")
        .into()
}

#[test]
fn infinite_loop_and_backtracking_regex_cannot_outlive_host_deadline() {
    let limits = BrokerHostLimits {
        fuel: 8_000_000_000,
        max_timeout: Duration::from_millis(250),
        ..BrokerHostLimits::default()
    };
    // Registry compilation is outside invocation fuel/deadline; warm it before wall timing.
    let warm = Harness::<PythonProvider>::get(component())
        .host_limits(limits.clone())
        .call("python.eval", json!({"script":"result = 0"}))
        .expect("warm registry");
    assert_eq!(warm.status, 0, "{warm:?}");
    for script in [
        "while True:\n    pass",
        "import re\nresult = bool(re.search('(a+)+$', 'a' * 20000 + '!'))",
    ] {
        let start = Instant::now();
        let outcome = Harness::<PythonProvider>::get(component())
            .host_limits(limits.clone())
            .call("python.eval", json!({"script":script}));
        if let Ok(ref output) = outcome {
            // Some regex implementations short-circuit; a successful bounded answer is valid.
            assert!(
                script.contains("import re"),
                "infinite loop completed: {output:?}"
            );
            assert_eq!(output.status, 0, "{output:?}");
        } else {
            let error = format!("{:?}", outcome.unwrap_err()).to_ascii_lowercase();
            assert!(
                error.contains("deadline") || error.contains("timeout") || error.contains("fuel"),
                "{error}"
            );
        }
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "host took too long after deadline"
        );
    }
    assert_eq!(Harness::<PythonProvider>::compiled_identities(), 1);
}

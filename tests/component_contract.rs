//! Provider-specific authority checks run by the ordinary shared-workflow test suite.
use std::{path::PathBuf, process::Command};

#[test]
fn component_has_exact_http_only_authority() {
    let component = std::env::var_os("DEKOPON_PROVIDER_COMPONENT")
        .expect("DEKOPON_PROVIDER_COMPONENT must point at the built component");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let status = Command::new("bash")
        .arg(root.join("tests/component_contract/assert-component-contract.sh"))
        .arg(component)
        .status()
        .expect("bash and wasm-tools must be installed for component contract tests");
    assert!(
        status.success(),
        "component authority or WIT contract drift"
    );

    // The decoded external component contract is the broker-linkable authority boundary.
}

#[test]
fn rustpython_vm_build_script_never_freezes_the_environment() {
    let source = include_str!("../patches/rustpython-vm/build.rs");
    assert!(source.contains("write!(f, \"sysvars! {{ }}\")"));
    assert!(!source.contains("env::vars"));
    assert!(!source.contains("Command::new(\"git\")"));
    for stamp in [
        "RUSTPYTHON_GIT_HASH=vendored",
        "RUSTPYTHON_GIT_TIMESTAMP=0",
        "RUSTPYTHON_GIT_TAG=0.5.0",
        "RUSTPYTHON_GIT_BRANCH=vendored",
    ] {
        assert!(source.contains(stamp), "missing constant stamp {stamp}");
    }
}

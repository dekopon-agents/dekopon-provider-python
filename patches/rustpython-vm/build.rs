use std::{env, io::prelude::*, path::PathBuf, process::Command};

// Dekopon patch: the upstream build script writes every environment variable of the build
// process into `_sysconfigdata` and stamps the crate with `git describe` of whatever repository
// contains it. Both put machine-local paths and checkout state into the shipped component and make
// two builds of one commit differ. The provider is sandboxed and never reads sysconfig, so the
// table is empty and the git stamps are constants.
fn main() {
    let frozen_libs = if cfg!(feature = "freeze-stdlib") {
        "Lib/*/*.py"
    } else {
        "Lib/python_builtins/*.py"
    };
    for entry in glob::glob(frozen_libs).expect("Lib/ exists?").flatten() {
        let display = entry.display();
        println!("cargo:rerun-if-changed={display}");
    }

    println!("cargo:rustc-env=RUSTPYTHON_GIT_HASH=vendored");
    println!("cargo:rustc-env=RUSTPYTHON_GIT_TIMESTAMP=0");
    println!("cargo:rustc-env=RUSTPYTHON_GIT_TAG=0.5.0");
    println!("cargo:rustc-env=RUSTPYTHON_GIT_BRANCH=vendored");
    println!("cargo:rustc-env=RUSTC_VERSION={}", rustc_version());

    println!(
        "cargo:rustc-env=RUSTPYTHON_TARGET_TRIPLE={}",
        env::var("TARGET").unwrap()
    );

    let mut env_path = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    env_path.push("env_vars.rs");
    let mut f = std::fs::File::create(env_path).unwrap();
    write!(f, "sysvars! {{ }}").unwrap();
}

fn rustc_version() -> String {
    let rustc = env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    match Command::new(rustc).args(["-V"]).output() {
        Ok(output) => match String::from_utf8(output.stdout) {
            Ok(s) => s,
            Err(err) => format!("(output error: {err})"),
        },
        Err(err) => format!("(command error: {err})"),
    }
}

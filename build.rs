// Adapt only authority-bearing entry points of the locked frozen standard library.
use std::{env, fs, path::PathBuf};

fn replace(source: &mut String, old: &str, new: &str) {
    assert_eq!(
        source.matches(old).count(),
        1,
        "locked stdlib changed: {old}"
    );
    *source = source.replacen(old, new, 1);
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/python/time.py");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let frozen = out.join("invocation_stdlib");
    fs::create_dir_all(&frozen).unwrap();
    let lib = PathBuf::from(rustpython_pylib::LIB_PATH);
    let mut random = fs::read_to_string(lib.join("random.py")).unwrap();
    replace(
        &mut random,
        "from os import urandom as _urandom",
        "from _dekopon_entropy import draw as _urandom",
    );
    replace(&mut random, "import os as _os", "");
    replace(&mut random, "_sha512 = None", "");
    let start = random
        .find("        if version == 1 and isinstance(a, (str, bytes)):")
        .unwrap();
    let end = random[start..]
        .find("        self.gauss_next = None")
        .unwrap()
        + start;
    random.replace_range(
        start..end,
        r#"        if isinstance(a, (str, bytes, bytearray)):
            raise TypeError('string and byte seeds are unavailable: no hash primitives')
        if a is not None and not isinstance(a, (int, float)):
            raise TypeError('seed must be None, int, or float')
        super().seed(a)
"#,
    );
    // No process/fork lifecycle exists in the component.
    let start = random.find("if hasattr(_os, ").unwrap();
    random.truncate(start);
    fs::write(frozen.join("random.py"), random).unwrap();
    let mut secrets = fs::read_to_string(lib.join("secrets.py")).unwrap();
    replace(&mut secrets, "from hmac import compare_digest", "");
    replace(&mut secrets, "           'compare_digest',", "");
    fs::write(frozen.join("secrets.py"), secrets).unwrap();
    let mut datetime = fs::read_to_string(lib.join("_pydatetime.py")).unwrap();
    replace(&mut datetime, "import sys", "");
    replace(
        &mut datetime,
        "if t < max_fold_seconds and sys.platform.startswith(\"win\"):",
        "if False:  # UTC has no Windows local-time restriction",
    );
    // These legacy readers still work, without runtime imports after the guest guard.
    for _ in 0..2 {
        let start = datetime
            .find("        import warnings\n        warnings.warn(")
            .unwrap();
        let end = start
            + datetime[start..]
                .find("                      stacklevel=2)\n")
                .unwrap()
            + "                      stacklevel=2)\n".len();
        datetime.replace_range(start..end, "");
    }
    fs::write(frozen.join("_pydatetime.py"), datetime).unwrap();
    fs::copy("src/python/time.py", frozen.join("time.py")).unwrap();
    fs::write(
        out.join("invocation_frozen.rs"),
        format!("rustpython_vm::py_freeze!(dir = {:?})", frozen),
    )
    .unwrap();
}

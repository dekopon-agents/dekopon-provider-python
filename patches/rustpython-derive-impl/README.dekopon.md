# Dekopon reproducibility patch

This directory is the complete `rustpython-derive-impl 0.5.0` crates.io source
(original archive SHA-256 `322d64ea8a21d52cd769db0f6190b6e7c17963b13c8c17f39d7364e68af96731`),
with one provider-local reproducibility fix under RustPython's MIT license.

RustPython's macro implementation emitted several randomly seeded standard collection orders into
compiled code:

- `py_freeze!` collected modules in a `HashMap` and encoded that map's iteration order directly
  into each frozen library;
- class get/set and member nurseries emitted `HashMap` iteration order into generated Rust tokens;
- module aliases emitted `HashSet` iteration order into generated Rust tokens.

Independent compiler processes therefore generated different `rustpython-pylib`, `rustpython-vm`,
and `rustpython-stdlib` metadata and ultimately different Wasm bytes from the same Python and Rust
sources. The patch uses ordered maps and explicit sorts before compilation, frozen-library
encoding, or token emission. It does not drop, rewrite, hash-normalize, or otherwise mask any
module, bytecode, Rust code, or data.

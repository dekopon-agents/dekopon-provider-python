#!/usr/bin/env bash
set -euo pipefail

lock=${1:-Cargo.lock}
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
[[ -f "$lock" ]] || { echo "error: missing lockfile $lock" >&2; exit 1; }

python3 - "$lock" <<'PY'
import pathlib, sys, tomllib
lock = tomllib.loads(pathlib.Path(sys.argv[1]).read_text())
packages = lock["package"]
versions = {}
for package in packages:
    versions.setdefault(package["name"], set()).add(package["version"])
required_rustpython = [
    "rustpython-codegen", "rustpython-common", "rustpython-compiler",
    "rustpython-compiler-core", "rustpython-derive", "rustpython-derive-impl",
    "rustpython-doc", "rustpython-literal", "rustpython-pylib",
    "rustpython-sre_engine", "rustpython-stdlib", "rustpython-vm", "rustpython-wtf8",
]
for name in required_rustpython:
    if versions.get(name) != {"0.5.0"}:
        raise SystemExit(f"error: {name} must resolve exactly once at 0.5.0, got {versions.get(name)}")
required = {
    "dekopon-provider-sdk": {"0.11.0"},
    "malachite-base": {"0.9.2"},
    "malachite-bigint": {"0.9.2"},
    "malachite-nz": {"0.9.2"},
    "malachite-q": {"0.9.2"},
    "r-efi": {"5.3.0", "6.0.0"},
    "yaml-rust2": {"0.12.0"},
}
for name, expected in required.items():
    if versions.get(name) != expected:
        raise SystemExit(f"error: {name} must be {expected}, got {versions.get(name)}")
checksums = {
    ("dekopon-provider-sdk", "0.11.0"): "40d29d6bfd3f634c6229cf6121b0ae78fb512e1355862ba112d7c5dcf3241e39",
    ("malachite-base", "0.9.2"): "a4f44099731f17094b07825c88ccb5fbd1bfa1f82fafff7daa33e8b8652db16e",
    ("malachite-bigint", "0.9.2"): "cc58206ba15e9c406e20c95c5f86efa07b12f94080945908e910b3a0faa23fef",
    ("malachite-nz", "0.9.2"): "a137660cdba20f136c8a223125f08088adb4e0b72fbb8466f08c43e31cc0427d",
    ("malachite-q", "0.9.2"): "5ffcbeed95e34c0fcc3864ccd146e129cbbf7de1513d3afbcfb47c7674c82d94",
    ("r-efi", "5.3.0"): "69cdb34c158ceb288df11e18b4bd39de994f6657d83847bdffdbd7f346754b0f",
    ("r-efi", "6.0.0"): "f8dcc9c7d52a811697d2151c701e0d08956f92b0e24136cf4cf27b57a6a0d9bf",
    ("yaml-rust2", "0.12.0"): "c6edb26322e610d4f04b7cd34478317685d24d0999437e551fb97c5441151041",
}
for package in packages:
    key = (package["name"], package["version"])
    if key in checksums and package.get("checksum") != checksums[key]:
        raise SystemExit(f"error: {key[0]} {key[1]} checksum drift")
PY

patch_manifest="$root/patches/rustpython-derive-impl/Cargo.toml"
patch_source="$root/patches/rustpython-derive-impl/src/compile_bytecode.rs"
patch_class="$root/patches/rustpython-derive-impl/src/pyclass.rs"
patch_module="$root/patches/rustpython-derive-impl/src/pymodule.rs"
[[ -f "$patch_manifest" && -f "$patch_source" && -f "$patch_class" && -f "$patch_module" ]] || {
  echo 'error: missing the pinned RustPython frozen-module reproducibility patch' >&2
  exit 1
}
grep -Fq 'rustpython-derive-impl = { path = "patches/rustpython-derive-impl" }' \
  "$root/Cargo.toml" || {
  echo 'error: Cargo.toml does not select the local RustPython reproducibility patch' >&2
  exit 1
}
grep -Fq 'collections::BTreeMap' "$patch_source"
grep -Fq 'paths.sort_by_key(|entry| entry.file_name());' "$patch_source"
grep -Fq 'map: BTreeMap<String, MemberNurseryEntry>' "$patch_class"
grep -Fq 'properties.sort_by(' "$patch_class"
grep -Fq 'py_names.sort();' "$patch_module"
rustup run 1.97.0 rustfmt --edition 2024 --check \
  "$patch_source" "$patch_class" "$patch_module"
if grep -Fq 'collections::HashMap' "$patch_source"; then
  echo 'error: patched py_freeze still uses randomized module ordering' >&2
  exit 1
fi

metadata=$(mktemp)
tree=$(mktemp)
features=$(mktemp)
trap 'rm -f "$metadata" "$tree" "$features"' EXIT
cargo metadata --locked --manifest-path "$root/Cargo.toml" --format-version 1 >"$metadata"
jq -e \
  --arg root "dekopon-python-provider" \
  --arg patch "$patch_manifest" '
  all(.packages[];
    .name == $root or
    (.name == "rustpython-derive-impl" and .version == "0.5.0" and
      .source == null and .manifest_path == $patch) or
    ((.source // "") | startswith("registry+https://github.com/rust-lang/crates.io-index"))
  )
' "$metadata" >/dev/null || {
  echo "error: dependency source is neither crates.io nor the exact local RustPython patch" >&2
  exit 1
}

cargo tree --locked --manifest-path "$root/Cargo.toml" \
  --target wasm32-unknown-unknown -e normal,build --prefix none >"$tree"
if grep -E '^(wasm-bindgen|js-sys|web-sys|wasm-bindgen-futures) v' "$tree"; then
  echo "error: browser/JavaScript package reached the Wasm target graph" >&2
  exit 1
fi
if grep -E '^(libloading|libffi|libffi-sys|reqwest|socket2|mio) v' "$tree"; then
  echo "error: dynamic loading or host networking package reached the Wasm target graph" >&2
  exit 1
fi
if grep -E '^(wasi|wasip[0-9]*|wasi-common|wasi-cap-std-sync) v' "$tree"; then
  echo "error: WASI package reached the wasm32-unknown-unknown target graph" >&2
  exit 1
fi
cargo tree --locked --manifest-path "$root/Cargo.toml" \
  --target wasm32-unknown-unknown -e features -f '{p} {f}' >"$features"
if grep 'rustpython-vm v0.5.0' "$features" | grep -E 'host_env|stdio|threading|wasmbind' >/dev/null; then
  echo "error: forbidden RustPython host feature enabled" >&2
  exit 1
fi

grep -Fq 'getrandom_backend=\"custom\"' "$root/.cargo/config.toml" || {
  echo "error: getrandom custom backend cfg is absent" >&2
  exit 1
}

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
    "malachite-bigint": {"0.9.2"},
    "yaml-rust2": {"0.12.0"},
}
for name, expected in required.items():
    if versions.get(name) != expected:
        raise SystemExit(f"error: {name} must be {expected}, got {versions.get(name)}")
for package in packages:
    if package["name"] == "dekopon-provider-sdk" and package.get("checksum") != "40d29d6bfd3f634c6229cf6121b0ae78fb512e1355862ba112d7c5dcf3241e39":
        raise SystemExit("error: dekopon-provider-sdk checksum drift")
    if package["name"] == "yaml-rust2" and package.get("checksum") != "c6edb26322e610d4f04b7cd34478317685d24d0999437e551fb97c5441151041":
        raise SystemExit("error: yaml-rust2 checksum drift")
PY

metadata=$(mktemp)
tree=$(mktemp)
features=$(mktemp)
trap 'rm -f "$metadata" "$tree" "$features"' EXIT
cargo metadata --locked --manifest-path "$root/Cargo.toml" --format-version 1 >"$metadata"
jq -e --arg root "dekopon-python-provider" '
  all(.packages[];
    .name == $root or
    ((.source // "") | startswith("registry+https://github.com/rust-lang/crates.io-index"))
  )
' "$metadata" >/dev/null || {
  echo "error: non-registry or local path dependency found" >&2
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

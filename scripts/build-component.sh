#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
core="$root/target/wasm32-unknown-unknown/release/dekopon_python_provider.wasm"
component=${1:-"$root/python-provider.wasm"}

required_rust="1.97.0"
required_rustc="rustc 1.97.0 (2d8144b78 2026-07-07)"
required_wasm_tools="1.236.1"

[[ "$(rustup run "$required_rust" rustc --version)" == "$required_rustc" ]] || {
  echo "error: expected $required_rustc" >&2
  exit 1
}
[[ "$(wasm-tools --version)" == "wasm-tools $required_wasm_tools" ]] || {
  echo "error: expected wasm-tools $required_wasm_tools" >&2
  exit 1
}

if git -C "$root" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  test -z "$(git -C "$root" ls-files '*.wasm')" || {
    echo "error: generated Wasm must never be tracked" >&2
    exit 1
  }
fi

# CARGO_ENCODED_RUSTFLAGS replaces target config rather than extending it, so preserve the exact
# custom getrandom cfg while normalizing only source paths. This leaves the global rustc wrapper
# (sccache), target directory, and incremental policy untouched.
cargo_home=${CARGO_HOME:-"$HOME/.cargo"}
cargo_home=$(cd "$cargo_home" && pwd -P)
sysroot=$(rustup run "$required_rust" rustc --print sysroot)
sysroot=$(cd "$sysroot" && pwd -P)
rustflags=(
  '--cfg=getrandom_backend="custom"'
  '--check-cfg=cfg(getrandom_backend, values("custom"))'
  "--remap-path-prefix=$root=/dekopon/source"
  "--remap-path-prefix=$cargo_home=/dekopon/cargo"
  "--remap-path-prefix=$sysroot=/dekopon/rust/$required_rust"
)
encoded_rustflags=$(printf '%s\x1f' "${rustflags[@]}")
encoded_rustflags=${encoded_rustflags%$'\x1f'}

CARGO_ENCODED_RUSTFLAGS="$encoded_rustflags" \
  cargo +"$required_rust" build \
    --locked \
    --manifest-path "$root/Cargo.toml" \
    --target wasm32-unknown-unknown \
    --release

wasm-tools validate "$core"
"$root/scripts/assert-zero-core-imports.sh" "$core"
wasm-tools component new "$core" -o "$component"
wasm-tools validate "$component"
"$root/scripts/assert-zero-core-imports.sh" "$component"

checksum="${component}.sha256"
(
  cd "$(dirname "$component")"
  sha256sum "$(basename "$component")" >"$(basename "$checksum")"
)
sha256sum --check --strict "$checksum"
printf 'generated %s (%s bytes)\n' "$component" "$(wc -c <"$component" | tr -d ' ')"

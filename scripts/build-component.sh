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

test -z "$(git -C "$root" ls-files '*.wasm')" || {
  echo "error: generated Wasm must never be tracked" >&2
  exit 1
}

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

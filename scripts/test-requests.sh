#!/usr/bin/env bash
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
component=${1:-"$root/python-provider.wasm"}
[[ -f "$component" ]] || { echo "error: build HTTP component first" >&2; exit 1; }
component=$(cd "$(dirname "$component")" && pwd -P)/$(basename "$component")
"$root/scripts/assert-component-contract.sh" "$component"
DEKOPON_PYTHON_COMPONENT="$component" \
  cargo +1.98.1 test --locked --manifest-path "$root/Cargo.toml" --test requests -- --nocapture

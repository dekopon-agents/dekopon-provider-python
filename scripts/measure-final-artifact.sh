#!/usr/bin/env bash
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
# shellcheck source=lib-sha256.sh
# Resolved from this script's absolute repository root.
# shellcheck disable=SC1091
source "$root/scripts/lib-sha256.sh"
component=${1:-"$root/python-provider.wasm"}
raw="$root/target/wasm32-unknown-unknown/release/dekopon_python_provider.wasm"

bytes=$(wc -c <"$component" | tr -d ' ')
sha=$(sha256sum_digest "$component")
raw_bytes=$(wc -c <"$raw" | tr -d ' ')

wasm-tools print --skeleton "$raw" >/tmp/dekopon-python-core-skeleton.wat
python3 - "$bytes" "$raw_bytes" "$sha" <<'PY'
import json, pathlib, sys
# The fuel figures are the bracket `tests/broker.rs` asserts against the real broker host: 10M and
# 50M never reach guest code, and the dedicated immediate profile below runs the VM comfortably.
record = {
    "componentBytes": int(sys.argv[1]),
    "rawCoreBytes": int(sys.argv[2]),
    "sha256": sys.argv[3],
    "insufficientFuel": [10_000_000, 50_000_000],
    "dedicatedImmediateFuel": 1_000_000_000,
    "selectedBrokerFuel": 1_000_000_000,
    "selectedMemoryBytes": 64 * 1024 * 1024,
}
pathlib.Path("/tmp/dekopon-python-measurements.json").write_text(json.dumps(record, indent=2) + "\n")
PY
cat /tmp/dekopon-python-measurements.json
printf 'core declarations: /tmp/dekopon-python-core-skeleton.wat\n'

#!/usr/bin/env bash
set -euo pipefail
component=${1:?usage: test-wasmtime-smoke.sh <component.wasm>}
test "$(wasmtime --version)" = "wasmtime 48.0.0"

wasmtime run --invoke 'describe()' "$component" \
  | jq -r . \
  | jq -e '.id == "python" and .commandWords == [] and (.capabilities | length) == 1' >/dev/null

wasmtime run \
  --invoke 'invoke("python.eval", "{\"script\":\"result = 2\"}")' \
  "$component" \
  | jq -r . \
  | jq -e '.outcome == "succeeded" and .output.ok == true and .output.result == 2' >/dev/null

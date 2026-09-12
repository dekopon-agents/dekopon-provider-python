#!/usr/bin/env bash
set -euo pipefail
component=${1:?usage: test-wasmtime-smoke.sh <component.wasm>}
# `cargo install wasmtime-cli` prints the bare version; a released binary appends its commit.
[[ "$(wasmtime --version)" == "wasmtime 48.0.2"* ]]

wasmtime run --invoke 'describe()' "$component" \
  | jq -r . \
  | jq -e '.id == "python" and .commandWords == [] and (.capabilities | length) == 1' >/dev/null

wasmtime run \
  --invoke 'invoke("python.eval", "{\"script\":\"result = 2\"}")' \
  "$component" \
  | jq -r . \
  | jq -e '.outcome == "succeeded" and .output.ok == true and .output.result == 2' >/dev/null

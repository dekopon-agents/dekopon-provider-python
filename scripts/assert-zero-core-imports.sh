#!/usr/bin/env bash
set -euo pipefail

file=${1:?usage: assert-zero-core-imports.sh <core-or-component.wasm>}
[[ -f "$file" ]] || { echo "error: missing $file" >&2; exit 1; }

skeleton=$(mktemp)
trap 'rm -f "$skeleton"' EXIT
wasm-tools print --skeleton "$file" >"$skeleton"
if grep -n '(import ' "$skeleton"; then
  echo "error: $file or a nested core module contains imports" >&2
  exit 1
fi

for forbidden in \
  wasi_snapshot_preview1 \
  'wasi:' \
  __wbindgen_placeholder__ \
  __wbindgen_externref_xform__ \
  wasm-bindgen; do
  if LC_ALL=C grep -aF -- "$forbidden" "$file" >/dev/null; then
    echo "error: $file contains forbidden marker $forbidden" >&2
    exit 1
  fi
done

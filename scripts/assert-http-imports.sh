#!/usr/bin/env bash
# Source-only HTTP variant: exact external authority, never a weakened offline assertion.
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
file=${1:?usage: assert-http-imports.sh <core-or-component.wasm>}
temporary=$(mktemp -d)
trap 'rm -rf "$temporary"' EXIT
wasm-tools validate "$file"
wasm-tools print --skeleton "$file" >"$temporary/skeleton"
for forbidden in wasi_snapshot_preview1 'wasi:' __wbindgen_placeholder__ __wbindgen_externref_xform__ wasm-bindgen; do
  if LC_ALL=C grep -aF -- "$forbidden" "$file" >/dev/null; then
    echo "error: HTTP variant contains forbidden marker $forbidden" >&2; exit 1
  fi
done
if head -1 "$temporary/skeleton" | grep -q '^(module'; then
  # The raw guest has exactly one import. Componentizer-generated internal adapters are checked
  # by validating the final component and its complete, exact external WIT below.
  python3 - "$temporary/skeleton" <<'PY'
import pathlib, re, sys
text = pathlib.Path(sys.argv[1]).read_text()
imports = [line.strip() for line in text.splitlines() if '(import ' in line]
assert len(imports) == 1, imports
assert re.fullmatch(r'\(import "dekopon:http/client@1\.0\.0" "send" \(func .*', imports[0]), imports
PY
else
  wasm-tools component wit -j "$file" >"$temporary/actual.json"
  wasm-tools component wit -j "$root/wit/http/http.wit" >"$temporary/http.json"
  python3 "$root/scripts/assert-http-wit.py" "$temporary/actual.json" "$temporary/http.json"
fi

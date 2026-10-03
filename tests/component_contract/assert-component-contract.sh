#!/usr/bin/env bash
set -euo pipefail
file=${1:?usage: assert-component-contract.sh <component.wasm>}
wasm-tools validate "$file"
temporary=$(mktemp)
trap 'rm -f "$temporary"' EXIT
wasm-tools component wit "$file" >"$temporary"
python3 - "$temporary" <<'PY'
from pathlib import Path
import sys
text = Path(sys.argv[1]).read_text()
assert 'dekopon:stdio/streams@0.1.0' in text, 'missing typed stdio import'
assert 'dekopon:http/client@1.2.0' in text, 'missing broker HTTP import'
for imported in ('dekopon:clock/wall@1.1.0', 'dekopon:clock/monotonic@1.1.0', 'dekopon:random/source@0.1.0'):
    assert imported in text, f'missing toolkit host import {imported}'
assert 'wasi:' not in text and 'wasi_snapshot_preview1' not in text, 'ambient WASI import'
assert 'dekopon:provider/provider@0.3.0' not in text, 'old provider world'
PY

#!/usr/bin/env bash
set -euo pipefail
file=${1:?usage: assert-component-contract.sh <component.wasm>}
wasm-tools validate "$file"
temporary=$(mktemp)
trap 'rm -f "$temporary"' EXIT
wasm-tools component wit "$file" >"$temporary"
python3 - "$temporary" <<'PY'
from pathlib import Path
import re
import sys
text = Path(sys.argv[1]).read_text()
world = text.split('world root {', 1)[1].split('\n}', 1)[0]
declarations = re.findall(r'(?m)^\s*import\b[^\n]*', world)
assert len(re.findall(r'\bimport\b', world)) == len(declarations), 'unparsed root-world import'
expected = {
    'dekopon:stdio/streams@0.1.0',
    'dekopon:http/client@1.2.0',
    'dekopon:clock/wall@1.1.0',
    'dekopon:clock/monotonic@1.1.0',
    'dekopon:random/source@0.1.0',
}
assert len(declarations) == len(expected) and {line.strip() for line in declarations} == {
    f'import {name};' for name in expected
}, f'imports differ: {declarations}'
assert 'wasi:' not in text and 'wasi_snapshot_preview1' not in text, 'ambient WASI import'
assert 'dekopon:provider/provider@0.3.0' not in text, 'old provider world'
PY

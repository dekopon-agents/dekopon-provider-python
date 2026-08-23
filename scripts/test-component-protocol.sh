#!/usr/bin/env bash
set -euo pipefail
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)/lib-component.sh" "$@"
[[ -f "$component" ]] || { echo "error: missing $component" >&2; exit 1; }

result=$(invoke --input '{"script":"print(\"hello\")\nresult = {\"answer\": 6 * 7}"}')
jq -e '
  .provider == "python" and .capability == "python.eval" and
  .output == {"ok":true,"stdout":"hello\n","stdoutTruncated":false,"result":{"answer":42}}
' <<<"$result" >/dev/null

syntax=$(invoke --input '{"script":"if:"}')
jq -e '.output.ok == false and .output.error.kind == "syntax" and .output.error.type == "SyntaxError"' \
  <<<"$syntax" >/dev/null
runtime=$(invoke --input '{"script":"print(\"before\")\nraise ValueError(\"boom\")"}')
jq -e '.output.ok == false and .output.stdout == "before\n" and .output.error == {"kind":"runtime","type":"ValueError","message":"boom"}' \
  <<<"$runtime" >/dev/null
oversized_result=$(invoke --input '{"script":"result = \"x\" * 131073"}')
jq -e '.output.ok == false and .output.error.kind == "result"' <<<"$oversized_result" >/dev/null
oversized_yaml=$(invoke --input '{"script":"import yaml\nresult = yaml.safe_load(\"x\" * 70000)"}')
jq -e '.output.ok == false and .output.error.kind == "yaml"' <<<"$oversized_yaml" >/dev/null

large_input=$(mktemp)
trap 'rm -f "$large_input"' EXIT
python3 - "$large_input" <<'PY'
import json, sys
json.dump({"script": "print('é' * 40000, end='')\nresult = None"}, open(sys.argv[1], "w"))
PY
stdout=$(invoke --input-file "$large_input")
jq -e '.output.ok == true and .output.stdoutTruncated == true and (.output.stdout | utf8bytelength) <= 65536' \
  <<<"$stdout" >/dev/null

if invoke --input '{"script":1}' >/tmp/dekopon-python-invalid.out 2>/tmp/dekopon-python-invalid.err; then
  echo "error: malformed capability input unexpectedly succeeded" >&2
  exit 1
fi
grep -Eq 'invalid-input|one string field named script' /tmp/dekopon-python-invalid.err

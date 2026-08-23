#!/usr/bin/env bash
set -euo pipefail
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)/lib-component.sh" "$@"

input=$(mktemp)
trap 'rm -f "$input"' EXIT
python3 - "$input" <<'PY'
import json, sys
script = r'''
import yaml
cases = {
    "directive": "%YAML 1.2\n---\na: b",
    "anchor": "a: &x [1]\nb: *x",
    "alias": "a: *missing",
    "tag": "a: !thing b",
    "duplicate": "a: 1\na: 2",
    "merge": "<<: value",
    "complexKey": "? [a, b]\n: value",
    "multiple": "---\na: b\n---\nc: d",
    "nonFinite": "a: .inf",
    "integerRange": "a: 9007199254740992",
}
rejected = {}
for name, source in cases.items():
    try:
        yaml.safe_load(source)
    except yaml.YAMLError:
        rejected[name] = True
    else:
        rejected[name] = False
safe = yaml.safe_load("date: 2025-02-03\narray: [null, true, 2.5]")
dumped = yaml.safe_dump({"a": [1, 2], "date": "2025-02-03"})
result = {"rejected": rejected, "safe": safe, "roundTrip": yaml.safe_load(dumped)}
'''
json.dump({"script": script}, open(sys.argv[1], "w"))
PY
output=$(invoke --input-file "$input")
jq -e '
  .output.ok == true and
  ([.output.result.rejected[]] | all) and
  .output.result.safe == {"date":"2025-02-03","array":[null,true,2.5]} and
  .output.result.roundTrip == {"a":[1,2],"date":"2025-02-03"}
' <<<"$output" >/dev/null

#!/usr/bin/env bash
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
component=${1:-"$root/python-provider.wasm"}
raw="$root/target/wasm32-unknown-unknown/release/dekopon_python_provider.wasm"
cache="$root/target/dekopon-run-measure-cache"
mkdir -p "$cache"

bytes=$(wc -c <"$component" | tr -d ' ')
sha=$(sha256sum "$component" | awk '{print $1}')
raw_bytes=$(wc -c <"$raw" | tr -d ' ')

# Pin a measured normal startup bracket for this exact build.
if dekopon-run -vv invoke --provider "$component" --compile-cache "$cache" --fuel 50000000 \
  --timeout-ms 5000 python.eval --input '{"script":"result = 2"}' >/tmp/python-measure-low.out 2>/tmp/python-measure-low.err; then
  echo "error: 50M-fuel measurement probe unexpectedly succeeded" >&2
  exit 1
fi
if ! grep -Eqi 'OutOfFuel|all fuel consumed|fuel[^[:alnum:]]*(exhausted|consumed)' \
  /tmp/python-measure-low.err; then
  echo "error: 50M-fuel probe failed for a reason other than fuel exhaustion" >&2
  cat /tmp/python-measure-low.err >&2
  exit 1
fi
low_fuel="out-of-fuel"
normal=$(dekopon-run invoke --provider "$component" --compile-cache "$cache" --fuel 500000000 \
  --timeout-ms 5000 --repeat 3 python.eval --input '{"script":"result = 2"}')

cold_log=/tmp/dekopon-python-cold-time.txt
rm -rf /tmp/dekopon-python-cold-cache
mkdir -p /tmp/dekopon-python-cold-cache
if /usr/bin/time -l true >/dev/null 2>&1; then
  /usr/bin/time -l dekopon-run invoke --provider "$component" \
    --compile-cache /tmp/dekopon-python-cold-cache --fuel 500000000 --timeout-ms 5000 \
    python.eval --input '{"script":"result = 2"}' >/tmp/dekopon-python-cold.json 2>"$cold_log"
else
  /usr/bin/time -v dekopon-run invoke --provider "$component" \
    --compile-cache /tmp/dekopon-python-cold-cache --fuel 500000000 --timeout-ms 5000 \
    python.eval --input '{"script":"result = 2"}' >/tmp/dekopon-python-cold.json 2>"$cold_log"
fi

wasm-tools print --skeleton "$raw" > /tmp/dekopon-python-core-skeleton.wat
python3 - "$bytes" "$raw_bytes" "$sha" "$low_fuel" "$cold_log" <<'PY'
import json, pathlib, sys
record = {
    "componentBytes": int(sys.argv[1]),
    "rawCoreBytes": int(sys.argv[2]),
    "sha256": sys.argv[3],
    "fuel50000000": sys.argv[4],
    "normalFuel": 500_000_000,
    "dedicatedImmediateFuel": 500_000_000,
    "selectedBrokerFuel": 1_000_000_000,
    "coldTimeRaw": pathlib.Path(sys.argv[5]).read_text(errors="replace")[-8192:],
}
pathlib.Path("/tmp/dekopon-python-measurements.json").write_text(json.dumps(record, indent=2) + "\n")
PY
jq '{timing, output}' <<<"$normal"
cat /tmp/dekopon-python-measurements.json
printf 'core declarations: /tmp/dekopon-python-core-skeleton.wat\n'

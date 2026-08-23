#!/usr/bin/env bash
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
component=${1:-"$root/python-provider.wasm"}
cache="$root/target/dekopon-run-compile-cache"
mkdir -p "$cache"
base=(dekopon-run -vv invoke --provider "$component" --compile-cache "$cache" --max-input-bytes 1048576 --max-output-bytes 786432)

# Working dedicated profile.
"${base[@]}" --max-memory-bytes 67108864 --fuel 500000000 --timeout-ms 5000 \
  python.eval --input '{"script":"result = 2"}' >/tmp/dekopon-python-resource-ok.out
jq -e '.output.ok == true and .output.result == 2' /tmp/dekopon-python-resource-ok.out >/dev/null

# The immediate default fuel is known to stop exact RustPython during startup.
if "${base[@]}" --max-memory-bytes 67108864 --fuel 10000000 --timeout-ms 5000 \
  python.eval --input '{"script":"result = 2"}' >/tmp/dekopon-python-low-fuel.out 2>/tmp/dekopon-python-low-fuel.err; then
  echo "error: 10M-fuel invocation unexpectedly succeeded" >&2
  exit 1
fi
grep -Eqi 'fuel|OutOfFuel|all fuel consumed' /tmp/dekopon-python-low-fuel.err

# Epoch deadline interruption must terminate guest code that never returns.
if "${base[@]}" --max-memory-bytes 67108864 --fuel 8000000000 --timeout-ms 50 \
  python.eval --input '{"script":"while True: pass"}' >/tmp/dekopon-python-deadline.out 2>/tmp/dekopon-python-deadline.err; then
  echo "error: infinite loop unexpectedly succeeded" >&2
  exit 1
fi
grep -Eqi 'deadline|epoch|interrupt' /tmp/dekopon-python-deadline.err

# The selected 64 MiB profile reached guest execution above. Under that same profile, a hostile
# allocation substantially larger than the ceiling must terminate as a host resource error rather
# than a structured provider result.
if "${base[@]}" --max-memory-bytes 67108864 --fuel 1000000000 --timeout-ms 5000 \
  python.eval --input '{"script":"result = bytearray(100000000)"}' >/tmp/dekopon-python-memory.out 2>/tmp/dekopon-python-memory.err; then
  echo "error: allocation beyond the selected memory profile unexpectedly succeeded" >&2
  exit 1
fi
grep -Eqi 'memory|grow|limit|allocation|resource|unreachable' /tmp/dekopon-python-memory.err

# Python recursion is a structured guest error under the working host profile.
"${base[@]}" --max-memory-bytes 67108864 --fuel 500000000 --timeout-ms 5000 \
  python.eval --input '{"script":"def f(): return f()\nf()"}' >/tmp/dekopon-python-recursion.out
jq -e '.output.ok == false and .output.error.type == "RecursionError"' \
  /tmp/dekopon-python-recursion.out >/dev/null

# Oversized/adversarial regex work must either finish in the value envelope or be stopped by the
# host; it must never outlive the 250 ms authorization-equivalent deadline.
if "${base[@]}" --max-memory-bytes 67108864 --fuel 8000000000 --timeout-ms 250 \
  python.eval --input '{"script":"import re\nresult = bool(re.search(\"(a+)+$\", \"a\" * 20000 + \"!\"))"}' \
  >/tmp/dekopon-python-regex.out 2>/tmp/dekopon-python-regex.err; then
  jq -e '.output.ok == true or .output.ok == false' /tmp/dekopon-python-regex.out >/dev/null
else
  grep -Eqi 'deadline|epoch|interrupt|fuel|memory|resource' /tmp/dekopon-python-regex.err
fi

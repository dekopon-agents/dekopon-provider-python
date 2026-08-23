#!/usr/bin/env bash
set -euo pipefail
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)/lib-component.sh" "$@"

input=$(mktemp)
trap 'rm -f "$input"' EXIT
python3 - "$input" <<'PY'
import json, sys
script = r'''
import json
import re
import yaml
supported = [json.loads('{"x": 1}')["x"], re.search(r"b+", "abbc").group(0), yaml.safe_load("x: 2")["x"]]
denied = {}
for name in ("sys", "os", "pathlib", "time", "random", "secrets", "socket", "ssl", "sqlite3", "subprocess", "threading", "ctypes", "tkinter", "webbrowser"):
    try:
        __import__(name)
    except ImportError:
        denied[name] = True
    else:
        denied[name] = False
builtins_denied = []
for expression in ("open('x')", "input()", "breakpoint()", "eval('1')", "exec('x=1')", "compile('1', 'x', 'eval')"):
    try:
        if expression.startswith("open"):
            open("x")
        elif expression.startswith("input"):
            input()
        elif expression.startswith("breakpoint"):
            breakpoint()
        elif expression.startswith("eval"):
            eval("1")
        elif expression.startswith("exec"):
            exec("x=1")
        else:
            compile("1", "x", "eval")
    except Exception:
        builtins_denied.append(True)
    else:
        builtins_denied.append(False)
result = {"supported": supported, "denied": denied, "builtinsDenied": builtins_denied}
'''
json.dump({"script": script}, open(sys.argv[1], "w"))
PY
output=$(invoke --input-file "$input")
jq -e '
  .output.ok == true and
  .output.result.supported == [1, "bb", 2] and
  ([.output.result.denied[]] | all) and
  (.output.result.builtinsDenied | all)
' <<<"$output" >/dev/null

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

recovery_denied = []
try:
    recovery_denied.append(re.enum.sys.modules.get("_dekopon_policy") is None)
except (AttributeError, KeyError):
    recovery_denied.append(True)
for module in (json, re, re.search.__globals__["_compiler"]):
    recovery_denied.extend([
        not hasattr(module, "__loader__"),
        not hasattr(module, "__spec__"),
    ])
for namespace in (
    re.search.__globals__,
    re.RegexFlag.__new__.__globals__,
    json.loads.__globals__,
    json.JSONDecoder.decode.__globals__,
):
    recovery_denied.extend([
        "sys" not in namespace,
        "_original_import" not in namespace,
        "_original_eval" not in namespace,
        "_original_compile" not in namespace,
    ])
builtins_view = re.search.__globals__["__builtins__"]
if type(builtins_view) is dict:
    recovery_denied.extend(name not in builtins_view for name in ("eval", "exec", "compile", "open"))
else:
    recovery_denied.extend(not hasattr(builtins_view, name) for name in ("eval", "exec", "compile", "open"))
try:
    subclasses = object.__subclasses__()
except AttributeError:
    recovery_denied.append(True)
else:
    recovered = False
    for loader in subclasses:
        if loader.__name__ in ("BuiltinImporter", "FrozenImporter"):
            try:
                loader.load_module("sys")
            except Exception:
                pass
            else:
                recovered = True
    recovery_denied.append(not recovered)
result = {
    "supported": supported,
    "denied": denied,
    "builtinsDenied": builtins_denied,
    "recoveryDenied": recovery_denied,
}
'''
json.dump({"script": script}, open(sys.argv[1], "w"))
PY
output=$(invoke --input-file "$input")
jq -e '
  .output.ok == true and
  .output.result.supported == [1, "bb", 2] and
  ([.output.result.denied[]] | all) and
  (.output.result.builtinsDenied | all) and
  (.output.result.recoveryDenied | all)
' <<<"$output" >/dev/null

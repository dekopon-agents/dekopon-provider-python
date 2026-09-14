#!/usr/bin/env python3
"""Compare the entire HTTP interface structurally to the byte-pinned guest WIT."""
import json
import sys

actual, expected = [json.load(open(path, encoding="utf-8")) for path in sys.argv[1:]]
assert len(actual["worlds"]) == 1
world = actual["worlds"][0]
assert list(world["imports"]) == ["dekopon:http/client@1.0.0"], world["imports"]
assert world["imports"]["dekopon:http/client@1.0.0"] == {"interface": {"id": 0}}
assert sorted(world["exports"]) == ["describe", "invoke", "run-command"]
assert len(actual["interfaces"]) == 1


def without_docs(value):
    if isinstance(value, dict):
        return {k: without_docs(v) for k, v in value.items() if k not in ("docs", "package")}
    if isinstance(value, list):
        return [without_docs(v) for v in value]
    return value


assert without_docs(actual["interfaces"]) == without_docs(expected["interfaces"])
# Export argv/stdin append two anonymous types after the HTTP closure.
assert without_docs(actual["types"][:len(expected["types"])] ) == without_docs(expected["types"])
for name, params in [("describe", []), ("invoke", [
    {"name": "capability", "type": "string"}, {"name": "input-json", "type": "string"}
])]:
    function = world["exports"][name]["function"]
    assert function["params"] == params
    assert function["result"] == "string"
command = world["exports"]["run-command"]["function"]
assert command["result"] == "string"
assert [p["name"] for p in command["params"]] == ["argv", "stdin"]
assert actual["types"][command["params"][0]["type"]]["kind"] == {"list": "string"}
assert actual["types"][command["params"][1]["type"]]["kind"] == {"option": "string"}
assert len(actual["types"]) == len(expected["types"]) + 2

#!/usr/bin/env python3
"""Compare the entire HTTP and clock interfaces structurally to the byte-pinned guest WIT."""
import json
import sys

actual, expected, clock = [json.load(open(path, encoding="utf-8")) for path in sys.argv[1:]]
assert len(actual["worlds"]) == 1
world = actual["worlds"][0]
assert world["name"] == "root"
assert len(actual["interfaces"]) == 2
assert world["imports"] == {
    f"interface-{i}": {"interface": {"id": i}} for i in range(2)
}, world["imports"]
assert sorted(world["exports"]) == ["describe", "invoke", "run-command"]
assert len(actual["packages"]) == 3
assert sorted(p["name"] for p in actual["packages"]) == [
    "dekopon:clock@1.0.0", "dekopon:http@1.0.0", "root:component",
]
http_id = None
for i, interface in enumerate(actual["interfaces"]):
    package = actual["packages"][interface["package"]]
    name = interface["name"]
    assert (package["name"], name) in [
        ("dekopon:http@1.0.0", "client"), ("dekopon:clock@1.0.0", "wall"),
    ]
    assert package == {"name": package["name"], "interfaces": {name: i}, "worlds": {}}
    if name == "client":
        http_id = i
root_package = next(p for p in actual["packages"] if p["name"] == "root:component")
assert root_package == {"name": "root:component", "interfaces": {}, "worlds": {"root": 0}}
assert http_id is not None


def without_docs(value):
    if isinstance(value, dict):
        return {k: without_docs(v) for k, v in value.items() if k not in ("docs", "package")}
    if isinstance(value, list):
        return [without_docs(v) for v in value]
    return value


assert without_docs(actual["interfaces"][http_id]) == without_docs(expected["interfaces"][0])
assert without_docs(actual["interfaces"][1 - http_id]) == without_docs(clock["interfaces"][0])
assert clock["types"] == []
# Clock has no types; HTTP type indices are unchanged, but its interface owner may move.
for value in expected["types"]:
    if value.get("owner") == {"interface": 0}:
        value["owner"] = {"interface": http_id}
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

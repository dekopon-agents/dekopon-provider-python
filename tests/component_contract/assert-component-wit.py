#!/usr/bin/env python3
"""Exact combined external authority and buffered HTTP contract against local WIT mirrors."""
import copy
import json
import sys
from http_contract import buffered_http_contract

actual, mirror = [json.load(open(path, encoding="utf-8")) for path in sys.argv[1:]]
http = buffered_http_contract(mirror)


def strip_docs(value):
    if isinstance(value, dict):
        return {k: strip_docs(v) for k, v in value.items() if k not in ("docs", "package")}
    if isinstance(value, list):
        return [strip_docs(v) for v in value]
    return value


assert len(actual["worlds"]) == 1
world = actual["worlds"][0]
assert world["name"] == "root"
assert world["imports"] == {
    f"interface-{i}": {"interface": {"id": i}} for i in range(4)
}, world["imports"]
assert actual["packages"] == [
    {"name": "dekopon:clock@1.1.0", "interfaces": {"monotonic": 0, "wall": 1}, "worlds": {}},
    {"name": "dekopon:random@0.1.0", "interfaces": {"source": 2}, "worlds": {}},
    {"name": "dekopon:http@1.1.0", "interfaces": {"client": 3}, "worlds": {}},
    {"name": "root:component", "interfaces": {}, "worlds": {"root": 0}},
]
assert len(actual["interfaces"]) == 4
mirrors = {(mirror["packages"][i]["name"], name): mirror["interfaces"][interface]
           for i in range(len(mirror["packages"]))
           for name, interface in mirror["packages"][i]["interfaces"].items()}
for index, (package, name) in enumerate([
    ("dekopon:clock@1.1.0", "monotonic"),
    ("dekopon:clock@1.1.0", "wall"),
]):
    assert strip_docs(actual["interfaces"][index]) == strip_docs(mirrors[package, name])
source = actual["interfaces"][2]
expected_source = copy.deepcopy(mirrors["dekopon:random@0.1.0", "source"])
expected_source["functions"]["get-random-bytes"]["result"] = 0
assert strip_docs(source) == strip_docs(expected_source)
assert actual["types"][0]["kind"] == {"list": "u8"}
assert source["functions"]["get-random-bytes"]["result"] == 0

# The sole extra type before the buffered HTTP closure is random's list<u8>. The HTTP mirror
# includes stream/asset types pruned by componentization, so compare its full reachable send graph.
http_types = http["types"]

def shift(value):
    if isinstance(value, int):
        return value + 1
    if isinstance(value, dict):
        return {k: shift(v) for k, v in value.items()}
    if isinstance(value, list):
        return [shift(v) for v in value]
    return value

expected_client = shift(http["interfaces"][0])
expected_client["package"] = 2
assert strip_docs(actual["interfaces"][3]) == strip_docs(expected_client)
for index, expected in enumerate(http_types, start=1):
    expected = shift(expected)
    if expected["owner"] is not None:
        expected["owner"] = {"interface": 3}
    assert strip_docs(actual["types"][index]) == strip_docs(expected)
assert len(actual["types"]) == len(http_types) + 3
assert sorted(world["exports"]) == ["describe", "invoke", "run-command"]
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

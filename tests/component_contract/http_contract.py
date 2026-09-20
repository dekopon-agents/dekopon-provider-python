"""Select the buffered send type closure from the published HTTP 1.1.0 mirror.

The componentizer prunes unused stream/asset imports. Compare the remaining full
contract, without copying an obsolete HTTP 1.0.0 WIT or allowing extra authority.
"""
import copy


def buffered_http_contract(document):
    package = next(p for p in document["packages"] if p["name"] == "dekopon:http@1.1.0")
    interface_id = package["interfaces"]["client"]
    interface = document["interfaces"][interface_id]
    function = interface["functions"]["send"]
    reachable = set()

    def visit(value):
        if isinstance(value, int):
            if value not in reachable:
                reachable.add(value)
                visit(document["types"][value]["kind"])
        elif isinstance(value, dict):
            for key, item in value.items():
                if key != "docs":
                    visit(item)
        elif isinstance(value, list):
            for item in value:
                visit(item)

    visit(function)
    indices = {old: new for new, old in enumerate(sorted(reachable))}

    def remap(value):
        if isinstance(value, int):
            return indices[value]
        if isinstance(value, dict):
            return {key: remap(item) for key, item in value.items() if key != "docs"}
        if isinstance(value, list):
            return [remap(item) for item in value]
        return value

    types = []
    for index in sorted(reachable):
        original = document["types"][index]
        owner = original["owner"]
        assert owner in (None, {"interface": interface_id}), owner
        types.append({
            "name": original["name"],
            "kind": remap(original["kind"]),
            "owner": None if owner is None else {"interface": 0},
        })
    return {
        "types": types,
        "interfaces": [{
            "name": "client",
            "types": {name: indices[index] for name, index in interface["types"].items()
                      if index in reachable},
            "functions": {"send": remap(copy.deepcopy(function))},
            "package": 0,
        }],
    }

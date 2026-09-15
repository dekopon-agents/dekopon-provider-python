#!/usr/bin/env python3
"""Negative regressions for the shipped component's exact authority and WIT contract."""
import copy
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent.parent


class ComponentContractTests(unittest.TestCase):
    def test_raw_guest_rejects_missing_extra_and_wasi_imports(self):
        send = '(import "dekopon:http/client@1.0.0" "send" (func))'
        clock = '(import "dekopon:clock/wall@1.0.0" "now-unix-millis" (func (result i64)))'
        with tempfile.TemporaryDirectory() as directory:
            core = Path(directory) / "guest.wasm"
            for imports, accepted in [
                (send + clock, True),
                (send, False),
                (clock, False),
                ("", False),
                (send + clock + '(import "extra" "call" (func))', False),
                ('(import "wasi:cli/run@0.2.0" "run" (func))', False),
                (send.replace('"send"', '"other"') + clock, False),
                (send + clock.replace("now-unix-millis", "other"), False),
            ]:
                subprocess.run(
                    ["wasm-tools", "parse", "-o", str(core), "-"],
                    input=f"(module {imports})", text=True, check=True,
                )
                result = subprocess.run(
                    [str(ROOT / "tests/component_contract/assert-component-contract.sh"), str(core)],
                    capture_output=True, text=True,
                )
                self.assertEqual(result.returncode == 0, accepted, result.stderr)

    def test_full_wit_rejects_authority_and_signature_drift(self):
        expected = json.loads(subprocess.check_output(
            ["wasm-tools", "component", "wit", "-j", str(ROOT / "wit/deps/http.wit")],
            text=True,
        ))
        clock = json.loads(subprocess.check_output(
            ["wasm-tools", "component", "wit", "-j", str(ROOT / "wit/deps/clock.wit")],
            text=True,
        ))
        actual = copy.deepcopy(expected)
        actual["interfaces"].extend(copy.deepcopy(clock["interfaces"]))
        actual["interfaces"][1]["package"] = 1
        count = len(actual["types"])
        actual["types"].extend([
            {"name": None, "kind": {"list": "string"}, "owner": None},
            {"name": None, "kind": {"option": "string"}, "owner": None},
        ])
        exports = {}
        for name, params in [
            ("describe", []),
            ("invoke", [{"name": "capability", "type": "string"},
                        {"name": "input-json", "type": "string"}]),
            ("run-command", [{"name": "argv", "type": count},
                             {"name": "stdin", "type": count + 1}]),
        ]:
            exports[name] = {"function": {"params": params, "result": "string"}}
        actual["worlds"] = [{"name": "root", "imports": {
            "interface-0": {"interface": {"id": 0}},
            "interface-1": {"interface": {"id": 1}},
        }, "exports": exports}]
        actual["packages"] = [
            {"name": "dekopon:http@1.0.0", "interfaces": {"client": 0}, "worlds": {}},
            {"name": "dekopon:clock@1.0.0", "interfaces": {"wall": 1}, "worlds": {}},
            {"name": "root:component", "interfaces": {}, "worlds": {"root": 0}},
        ]
        cases = [(actual, True)]
        for mutation in [
            lambda x: x["worlds"][0]["imports"].clear(),
            lambda x: x["worlds"][0]["imports"].pop("interface-0"),
            lambda x: x["worlds"][0]["imports"].pop("interface-1"),
            lambda x: x["interfaces"][1].update(name="other"),
            lambda x: x["interfaces"][1]["functions"].update(other={}),
            lambda x: x["interfaces"][1]["functions"]["now-unix-millis"].update(result="u32"),
            lambda x: x["interfaces"][1]["functions"]["now-unix-millis"].update(
                params=[{"name": "extra", "type": "u64"}]),
            lambda x: x["interfaces"][0]["functions"]["send"].update(result="string"),
            lambda x: x["worlds"][0]["imports"].update({"extra": {"function": {}}}),
            lambda x: x["packages"][0].update(name="wasi:http@0.2.0"),
            lambda x: x["interfaces"][0].update(name="other"),
            lambda x: x["worlds"][0]["exports"]["invoke"]["function"].update(result="bool"),
            lambda x: x["types"][-1].update(kind={"list": "string"}),
            lambda x: x["types"].append({"kind": {"list": "u8"}}),
        ]:
            changed = copy.deepcopy(actual)
            mutation(changed)
            cases.append((changed, False))
        with tempfile.TemporaryDirectory() as directory:
            expected_path = Path(directory) / "expected.json"
            actual_path = Path(directory) / "actual.json"
            expected_path.write_text(json.dumps(expected))
            clock_path = Path(directory) / "clock.json"
            clock_path.write_text(json.dumps(clock))
            for fixture, accepted in cases:
                actual_path.write_text(json.dumps(fixture))
                result = subprocess.run(
                    ["python3", str(ROOT / "tests/component_contract/assert-component-wit.py"),
                     str(actual_path), str(expected_path), str(clock_path)], capture_output=True, text=True,
                )
                self.assertEqual(result.returncode == 0, accepted, result.stderr)


if __name__ == "__main__":
    unittest.main()

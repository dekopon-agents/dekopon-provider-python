#!/usr/bin/env python3
"""Negative authority and full WIT shape fixtures for the built combined component."""
import copy
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent.parent
CHECKER = ROOT / "tests/component_contract/assert-component-contract.sh"
WIT_CHECKER = ROOT / "tests/component_contract/assert-component-wit.py"
RAW = [
    '(import "dekopon:http/client@1.1.0" "send" (func))',
    '(import "dekopon:clock/monotonic@1.1.0" "now-nanos" (func))',
    '(import "dekopon:clock/wall@1.1.0" "now-unix-millis" (func))',
    '(import "dekopon:random/source@0.1.0" "get-random-bytes" (func))',
]


class ComponentContractTests(unittest.TestCase):
    def test_raw_guest_rejects_missing_extra_and_wasi_imports(self):
        with tempfile.TemporaryDirectory() as directory:
            core = Path(directory) / "guest.wasm"
            for imports, accepted in [
                (RAW, True),
                (RAW[:-1], False),
                (RAW + ['(import "extra" "call" (func))'], False),
                (RAW + ['(import "wasi:cli/run@0.2.0" "run" (func))'], False),
                ([RAW[0].replace('"send"', '"stream"')] + RAW[1:], False),
                ([RAW[0].replace('@1.1.0', '@1.0.0')] + RAW[1:], False),
                (RAW[:-1] + [RAW[-1].replace('get-random-bytes', 'not-random')], False),
                (RAW[:-2] + [RAW[-2].replace('now-unix-millis', 'now-nanos'), RAW[-1]], False),
            ]:
                subprocess.run(["wasm-tools", "parse", "-o", str(core), "-"],
                               input=f"(module {' '.join(imports)})", text=True, check=True)
                result = subprocess.run([str(CHECKER), str(core)], capture_output=True, text=True)
                self.assertEqual(result.returncode == 0, accepted, result.stderr)

    def test_full_wit_rejects_authority_and_signature_drift(self):
        component = os.environ["DEKOPON_PROVIDER_COMPONENT"]
        actual = json.loads(subprocess.check_output(
            ["wasm-tools", "component", "wit", "-j", component], text=True))
        with tempfile.TemporaryDirectory() as directory:
            mirror_path = Path(directory) / "mirror.json"
            actual_path = Path(directory) / "actual.json"
            mirror_path.write_bytes(subprocess.check_output(
                ["wasm-tools", "component", "wit", "-j", str(ROOT / "wit")]))
            cases = [(actual, True)]
            for mutation in [
                lambda x: x["worlds"][0]["imports"].pop("interface-0"),
                lambda x: x["worlds"][0]["imports"].update({"extra": {"function": {}}}),
                lambda x: x["packages"][0].update(name="wasi:clock@1.1.0"),
                lambda x: x["interfaces"][2]["functions"]["get-random-bytes"].update(result="string"),
                lambda x: x["interfaces"][3]["functions"].update(stream={}),
                lambda x: x["worlds"][0]["exports"]["invoke"]["function"].update(result="bool"),
                lambda x: x["types"][-1].update(kind={"list": "string"}),
                lambda x: x["types"].append({"kind": {"list": "u8"}}),
            ]:
                changed = copy.deepcopy(actual)
                mutation(changed)
                cases.append((changed, False))
            for fixture, accepted in cases:
                actual_path.write_text(json.dumps(fixture))
                result = subprocess.run(["python3", str(WIT_CHECKER), str(actual_path),
                                         str(mirror_path)], capture_output=True, text=True)
                self.assertEqual(result.returncode == 0, accepted, result.stderr)


if __name__ == "__main__":
    unittest.main()

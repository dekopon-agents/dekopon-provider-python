# Dekopon Python provider

An import-free WebAssembly component exposing one read-only, High-risk capability:
`python.eval`. Version 0.1.0 embeds **RustPython 0.5.0 exactly**, creates a fresh interpreter per
call, captures bounded stdout in Rust, and returns only a bounded JSON-shaped result.

> **Release status: prototype / publication HOLD.** The implementation is executable and host
> tested. Do not create `v0.1.0` or enable publication until the LGPL-3.0-only Malachite static-Wasm
> distribution question in `THIRD_PARTY_NOTICES.md` has an owner/legal decision. The release
> workflow additionally requires the repository variable `PROVIDER_PYTHON_RELEASE_APPROVED=true`.

## Build

Required versions are Rust 1.97.0, target `wasm32-unknown-unknown`, and wasm-tools 1.236.1.
The component is generated and ignored; it must never be committed.

```console
./scripts/build-component.sh
sha256sum --check python-provider.wasm.sha256
wasm-tools validate python-provider.wasm
./scripts/assert-zero-core-imports.sh python-provider.wasm
```

`python-provider.wasm` and its checksum are release products, not source files. CI rejects any
tracked `*.wasm`.

## Exact CLI use

RustPython VM startup needs more than Dekopon immediate mode's 10,000,000-fuel default. Always name
the dedicated profile explicitly:

```console
dekopon-run inspect \
  --max-memory-bytes 67108864 \
  --max-input-bytes 1048576 \
  --max-output-bytes 1048576 \
  --fuel 500000000 \
  --timeout-ms 5000 \
  --provider ./python-provider.wasm

dekopon-run invoke \
  --max-memory-bytes 67108864 \
  --max-input-bytes 1048576 \
  --max-output-bytes 786432 \
  --fuel 500000000 \
  --timeout-ms 5000 \
  --provider ./python-provider.wasm \
  python.eval \
  --input '{"script":"result = sum(i * i for i in range(5))"}'
```

Expected capability output:

```json
{
  "ok": true,
  "stdout": "",
  "stdoutTruncated": false,
  "result": 30
}
```

`dekopon-run` wraps this under its provider/capability/timing JSON. With the unmodified immediate
10,000,000-fuel default, invocation fails safely with a host `OutOfFuel` error; that profile is not
supported for this component.

For multiline source, avoid shell escaping:

```console
cat > /tmp/python-eval.json <<'JSON'
{
  "script": "import json\nprint('Python 3')\nresult = json.loads('{\"answer\": 42}')"
}
JSON

dekopon-run invoke \
  --provider ./python-provider.wasm \
  --fuel 500000000 --timeout-ms 5000 --max-output-bytes 786432 \
  python.eval --input-file /tmp/python-eval.json
```

## API

Input is exactly:

```json
{"script": "Python 3 source"}
```

`script` is required, must be a string, and is limited to 65,536 UTF-8 bytes. Unknown fields are
rejected before constructing a VM. Source executes as `Mode::Exec`; assign the desired return value
to `result` (predeclared as `None`).

Success data is exactly:

```json
{"ok":true,"stdout":"text","stdoutTruncated":false,"result":null}
```

Script-level failure data is exactly:

```json
{
  "ok": false,
  "stdout": "text before failure",
  "stdoutTruncated": false,
  "error": {"kind": "runtime", "type": "ValueError", "message": "bounded detail"}
}
```

Kinds are `syntax`, `runtime`, `yaml`, or `result`. No traceback, locals, or stderr are returned.
Unknown capability and malformed provider input instead use the SDK's stable provider-failure
envelope. Host resource traps remain host errors.

### Safe result model

Allowed values are null, bool, finite f64, integers in ±9,007,199,254,740,991, valid UTF-8 strings,
exact lists/tuples, and exact dicts with exact string keys. Maximum depth is 32, maximum node count
is 10,000 (mapping keys count), and the encoded result is at most 131,072 bytes. Cycles, subclasses,
custom conversion hooks, and unsupported objects are rejected.

### Supported modules

Only these public import roots are compatibility promises:

- `json` — RustPython's frozen Python JSON module and native acceleration;
- `re` — RustPython's Python regular-expression module / `_sre` implementation;
- `yaml` — this provider's native constrained facade with exactly `safe_load(str)`,
  `safe_dump(safe_value)`, and `YAMLError`.

Example:

```python
import json
import re
import yaml

config = yaml.safe_load("name: dekopon\nretries: 2")
assert re.fullmatch(r"[a-z]+", config["name"])
print(json.dumps(config, sort_keys=True))
result = {"config": config, "yaml": yaml.safe_dump(config)}
```

The YAML subset rejects directives, anchors, aliases, tags, merges, duplicate/complex/non-string
keys, multiple documents, non-finite/out-of-range numbers, and excess size/depth/nodes before any
alias expansion. Timestamp-looking plain scalars such as `2025-02-03` remain strings.

This is RustPython 0.5.0 with the tested module/value subset, not CPython conformance, arbitrary
stdlib/package compatibility, pip, or persistence.

## Denied authority and determinism

The final component and every nested core have zero imports. There is no WASI, JS/browser, host
environment, filesystem, network, HTTP/storage, clock, entropy, subprocess, dynamic loading, or
generic provider dispatch. Imports including `sys`, `os`, `time`, `random`, `secrets`, `socket`,
`ssl`, `sqlite3`, `subprocess`, `threading`, `ctypes`, `tkinter`, and `webbrowser` are denied.
`open`, `input`, and `breakpoint` are absent; guest `compile`, `eval`, and `exec` are denied.

The VM hash seed and custom getrandom backend are deterministic. The backend is non-cryptographic
and is not exposed to Python. Determinism does not make adversarial scripts safe: host fuel,
deadline, memory, admission, and container limits are mandatory.

## Limits and operations

See [SECURITY.md](SECURITY.md) for the complete provider/host split and
[docs/deployment-profile.md](docs/deployment-profile.md) for measured size, latency, fuel floor,
RSS, and the selected broker profile. Generic provider calls, registry lookup, proposal submission,
commands, persistence, and privileged imports are deliberately absent.

## Validation

```console
cargo +1.97.0 fmt --all -- --check
cargo +1.97.0 clippy --locked --all-targets -- -D warnings
cargo +1.97.0 test --locked --all-targets
cargo +1.93.0 check --locked --all-targets
cargo deny check licenses advisories bans sources
./scripts/validate.sh
```

## License

Repository code: MIT OR Apache-2.0. Embedded third-party terms, mandatory notices, unmaintained
advisories, and the publication hold are documented in `THIRD_PARTY_NOTICES.md` and `deny.toml`.

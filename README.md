# Dekopon Python provider

A WebAssembly component with broker-granted HTTP exposing one read-only, High-risk capability:
`python.eval`. It embeds **RustPython 0.5.0 exactly**, creates a fresh interpreter per call,
captures bounded stdout in Rust, and returns only a bounded JSON-shaped result. A Dekopon shell
reaches it through the `python` command word. Version 0.4.0 targets `dekopon-provider-sdk` 0.15.0
and exports `run-command` from `dekopon:provider/provider-cli@0.3.0`; an 0.11-era host will not
load it.

> **Release status: owner-approved; mechanical publication interlock remains.** The owner accepted
> the exact LGPL dependencies and corresponding-source/relink design for this standalone optional
> provider. This records a project policy choice, not attorney review. Publication still requires
> the per-repository variable `PROVIDER_PYTHON_RELEASE_APPROVED=true`, an annotated tag, and every
> transactional release check in `RELEASE_COMPLIANCE.md`.

## Broker-granted HTTP

The supported release/OCI component is **`python-provider.wasm`**, exposing **`python.eval`**
and command word **`python`**. Cargo feature `http` is default-on and keeps Dekopon-specific
HTTP code clearly separated; disabling defaults is a developer customization, not a supported
CI or distribution variant. Ordinary Cargo builds include `dekopon_requests`.
The sole external import is **`dekopon:http/client@1.0.0`**. A real broker must link it even for
pure scripts, which succeed without HTTP grants. Bare empty-linker Wasmtime cannot instantiate it.

Configure the broker's route/constraint set for `python.eval` with an explicit `http` grant:
`allowedHosts` (exact authorities, including effective nondefault port), `allowedMethods` (`GET`
and/or `HEAD`), `maxRequests`, `maxRequestBytes`, and `maxResponseBytes`; keep
`allowPlaintextLoopback: false` in production. Retain the fuel/memory/timeout/output profile below.
Do not attach credentials or secret-use bindings. The host enforces this **invocation grant on
every request**, including destinations computed by the script; there is no new Cedar decision,
nested proposal engine, or generic dispatch per call. Absent grants deny all calls. Host request
budget exhaustion stays exhausted even when Python catches its exception. Host DNS/IP validation,
HTTPS, byte/deadline limits, and redirect refusal remain authoritative. Plain HTTP is only available
for explicitly granted loopback authorities with explicit ports and the opt-in flag (used by tests).

```python
import dekopon_requests as requests

base = "https://example.test"
index = requests.get(base + "/index")
index.raise_for_status()
total = 0
for path in index.json():
    child = requests.get(base + path)
    child.raise_for_status()
    total += child.json()["value"]
print(total)
result = total
```

The native facade is intentionally not the pip `requests` package:

- Only `get(url)` and `head(url)`, with a UTF-8 string URL of at most 8,192 bytes; no optional
  arguments, headers, body, credentials, cookies, sessions, proxies, retries, or redirect following.
- `Response.status_code`, `ok` (status below 400), `content` (bytes), `text` (UTF-8 with replacement),
  `json()` (strict JSON projected through the existing safe-value limits), and `raise_for_status()`
  (raises at status 400 or above). HEAD content is empty. Redirect statuses are returned unchanged.
- Buffered response bodies are additionally capped at 131,072 bytes; the host must bound the whole
  response before it crosses the import. `json()` does not perform requests-style charset detection.
- `RequestException` is the common base; `HTTPError` and `JSONDecodeError` derive from it. Host
  failures expose only stable WIT codes such as `denied`, `host-call-limit`, or `response-too-large`,
  never host diagnostic text or URLs. Provider-generated exceptions carry short bounded messages.
- Runtime output is the existing JSON `ok/stdout/stdoutTruncated/result` or error envelope, **not an
  OS exit status**. Stdout and result limits do not increase for HTTP.

The source bundle, all-features SBOM, modified-Malachite offline relink test, feature-graph gate,
and exact component-contract/reproducibility checks cover this one shipped HTTP component.
Offline relinking means network-disconnected rebuilding from vendors, not a networkless runtime.
`tests/requests.rs` uses FakeBroker for no-grant denial and its real registry with published
`AuthorizationGate`/HTTP constraints for the controlled-server tests. It does not test Cedar policy
selection; it tests production host enforcement of preauthorized grants without a transport mock.

## Build

Required versions are Rust 1.98.1, target `wasm32-unknown-unknown`, wasm-tools 1.259.0, and
cargo-cyclonedx 0.5.9 when producing release source/SBOM assets. The component and all compliance
artifacts are generated and ignored; they must never be committed.

```console
./scripts/build-component.sh
sha256sum --check python-provider.wasm.sha256
wasm-tools validate python-provider.wasm
./scripts/assert-component-contract.sh python-provider.wasm
```

`python-provider.wasm` and its checksum are generated release products, not source files. CI
rejects any tracked `*.wasm`. `build-component.sh` builds a clean source snapshot at a fixed
canonical path under a scrubbed environment because RustPython 0.5.0's build script otherwise
freezes every visible build variable into `_sysconfigdata` (including accidental credentials). It
retains the ordinary default target for that standalone snapshot and global sccache; no compiler
wrapper, `CARGO_TARGET_DIR`, or incremental setting is replaced. The pinned
`rustpython-derive-impl 0.5.0` source patch sorts `py_freeze!` module traversal and every
map/set-backed macro token emission instead of compiling randomly seeded collection order. The gate
scans the resulting component for sensitive environment keys and compares the complete frozen build
input, raw core, and final component across independent builds.

The official Wasm is always distributed with a versioned corresponding-source/relink archive and
CycloneDX SBOM. The archive carries this exact application source, WIT and lockfile plus the
complete versioned source of every Cargo dependency and an offline source replacement. Build and
verify it with pinned `cargo-cyclonedx` 0.5.9:

```console
version=$(cargo metadata --locked --no-deps --format-version 1 |
  jq -er '.packages[] | select(.name == "dekopon-python-provider") | .version')
./scripts/build-source-bundle.sh dist
./scripts/test-source-bundle-reproducibility.sh dist
./scripts/test-source-bundle-relink.sh \
  "dist/dekopon-python-provider-$version-relink-source.tar.gz" \
  "dist/dekopon-python-provider-$version.cdx.json"
```

See [RELINKING.md](RELINKING.md) for recipient modification, rebuild, componentization, and
installation instructions. Generated archives, SBOMs, vendor trees, checksums, temporary build
trees, and Wasm remain ignored and absent from Git.

## The `python` command word

In a Dekopon shell the provider is the `python` program. The script comes inline with `-c`, piped
with `-`, or piped with no flag at all, usually as a here-document:

```console
python -c 'result = sum(i * i for i in range(5))'
python - <<'EOF' | jq .result
import yaml
result = yaml.safe_load("retries: 2")
EOF
python <<'EOF' | jq .result
import yaml
result = yaml.safe_load("retries: 2")
EOF
```

All three propose `python.eval` with exactly the [API](#api) input, `{"script": ...}`, and the
broker authorizes and runs that proposal like a direct call. The guest parses the argv itself with
the SDK's clap and constructs no VM to do it:

| argv | Answer |
|---|---|
| `--help`, `-h`, `--version`, `-V` | rendered on stdout, status 0 |
| `-c CODE` | proposes `{"script": CODE}`; a piped value is ignored |
| `-` with a piped value | proposes `{"script": <piped value>}` |
| `-` with nothing piped | declined: `python -: nothing was piped in`, a usage error at status 2 |
| nothing, with a non-empty piped value | proposes `{"script": <piped value>}`, identical to `-` |
| nothing, with nothing piped or an empty pipe | clap's usage error on stderr, status 2 |
| `-c` with `-`, a file name, extra arguments | clap's usage error on stderr, status 2 |

There is no `python FILE` and no `sys.argv`: the component has no filesystem and the capability
takes only a script, named by `-c`, `-`, or nothing at all when something non-empty is piped
in — `python <<'EOF' … EOF` matches CPython's own read of a non-tty stdin when given no file.
`src/commands.rs` pins the help page byte for byte.

## Running it

Use a Dekopon broker that links the published HTTP interface. The real-host smoke, protocol,
resource and HTTP-grant suites all exercise the same shipped component:

```console
./scripts/test-broker-testkit.sh python-provider.wasm
./scripts/test-requests.sh python-provider.wasm
```

`dekopon-provider-sdk-testkit`'s `FakeBroker` provides that host, including fuel, deadline,
and memory limits. For example, a pure script needs no HTTP grant:

```rust
let broker = FakeBroker::builder()
    .component("python-provider.wasm")
    .provider("python")
    .host_limits(BrokerHostLimits {
        max_memory_bytes: 64 * 1024 * 1024,
        fuel: 1_000_000_000,
        max_timeout: Duration::from_secs(5),
        ..BrokerHostLimits::default()
    })
    .timeout_ms(5_000)
    .max_output_bytes(786_432)
    .build()
    .await?;
let output = broker
    .invoke("python.eval", json!({"script": "result = sum(i * i for i in range(5))"}))
    .await?;
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

RustPython VM startup needs far more than Dekopon immediate mode's 10,000,000-fuel default; under
that default the invocation fails safely with a host `OutOfFuel` error. Name the dedicated profile
above explicitly.

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

Kinds are `syntax`, `runtime`, `yaml`, or `result`; HTTP facade exceptions use `runtime`. No traceback, locals, or stderr are returned.
Unknown capability and malformed provider input instead use the SDK's stable provider-failure
envelope. Host resource traps remain host errors.

### Safe result model

Allowed values are null, bool, finite f64, integers in ±9,007,199,254,740,991, valid UTF-8 strings,
exact lists/tuples, and exact dicts with exact string keys. Maximum depth is 32, maximum node count
is 10,000 (mapping keys count), and the encoded result is at most 131,072 bytes. Cycles, subclasses,
custom conversion hooks, and unsupported objects are rejected.

### Supported modules

Only these exact public module names are compatibility promises; direct imports of private
submodules such as `re._parser` and `json.decoder` are denied:

- `json` — RustPython's frozen Python JSON module and native acceleration;
- `re` — RustPython's Python regular-expression module / `_sre` implementation;
- `yaml` — this provider's native constrained facade with exactly `safe_load(str)`,
  `safe_dump(safe_value)`, and `YAMLError`;
- `dekopon_requests` — bounded GET/HEAD under the invocation HTTP grant described above.

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

The exact component contract allows only Dekopon HTTP. There is no WASI, JS/browser, host
environment, filesystem, raw socket, storage, clock, entropy, subprocess, dynamic loading, or
generic provider dispatch. Imports including `sys`, `os`, `time`, `random`, `secrets`, `socket`,
`ssl`, `sqlite3`, `subprocess`, `threading`, `ctypes`, `tkinter`, and `webbrowser` are denied.
`open`, `input`, and `breakpoint` are absent; guest `compile`, `eval`, and `exec` are denied.

The VM hash seed and custom getrandom backend are deterministic. The backend is non-cryptographic
and is not exposed to Python. Determinism does not make adversarial scripts safe: host fuel,
deadline, memory, admission, and container limits are mandatory.

## Limits and operations

See [SECURITY.md](SECURITY.md) for the complete provider/host split and
[docs/deployment-profile.md](docs/deployment-profile.md) for measured size, latency, fuel floor,
RSS, and the selected broker profile. A script deliberately gets no generic provider calls, registry
lookup, proposal submission, shell commands, persistence, or privileged imports.

## Validation

```console
cargo +1.98.1 fmt --all -- --check
cargo +1.98.1 clippy --locked --all-targets -- -D warnings
cargo +1.98.1 test --locked --all-targets
cargo deny check licenses advisories bans sources
./scripts/validate.sh
./scripts/prepare-release-assets.sh
./scripts/test-source-bundle-reproducibility.sh dist
./scripts/test-source-bundle-relink.sh
```

## License and corresponding source

Original source authored by this project remains **MIT OR Apache-2.0** (`LICENSE-MIT` and
`LICENSE-APACHE`). The distributed combined Wasm embeds four Malachite 0.9.2 packages under
**LGPL-3.0-only**. This does not relicense the original project source, but the embedded code and
combined distribution carry the applicable third-party terms. Prominent notices and exact package
checksums are in `THIRD_PARTY_NOTICES.md`; verbatim GNU texts are in `LICENSE-LGPL-3.0` and
`LICENSE-GPL-3.0` (with `LICENSE-LGPL-2.1` for the locked `r-efi` source packages).

Every binary release provides freely accessible exact corresponding source and relinking material
both as GitHub Release assets and at
`ghcr.io/dekopon-agents/provider-python-source:<version>`. See `RELINKING.md`. No `latest` tag is
published.

### Sticky host HTTP refusals

Policy violations (including absent/wrong grants), malformed host requests,
and host byte/call-budget exhaustion mark the invocation rejected. Although `send` returns a typed
error to Python, the real broker checks that sticky state after guest execution and returns
`HostCallRejected` instead of a successful guest envelope, even if the script catches the exception.
Transport/protocol failures and provider-local JSON/status/body-limit errors do not replenish any
budget but are ordinary bounded guest exceptions. A `maxResponseBytes` host refusal is a host error;
the provider's smaller body cap is checked only after a host-accepted response.

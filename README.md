# Dekopon Python provider

A WebAssembly component with broker-granted HTTP exposing one read-only, High-risk capability:
`python.eval`. The **0.7.0-alpha.1 candidate** embeds RustPython 0.5.0, DataFusion
55.1.0, ndarray 0.17.2, statrs 0.19.1 and SmartCore 0.6.14 in one component. It creates a fresh
interpreter per call, captures bounded stdout, and returns only a bounded JSON-shaped result.
The `python` command word remains pure. The candidate host/SDK dependency is core PR 354
revision `820ee7a521201828ed42171f1ead5e75026595d2` (0.23.0); the DataFusion
55.1.0 no-JS port is pinned to the public fork commit
`cf3778098ad3ea283ecd8ee2a991be7d9a29750c`. The SDK/host remains unreleased;
production brokers without the clock/random imports cannot run this component.

> **Release status: owner-approved.** The owner accepted the exact LGPL dependencies for this
> standalone optional provider; this records a project policy choice, not attorney review. LGPL is
> satisfied by the public tagged source, the reproducible build in
> [`dekopon-agents/provider-workflows`](https://github.com/dekopon-agents/provider-workflows), and
> the CycloneDX SBOM the shared release workflow publishes. Publication runs through that shared
> workflow: pushing an annotated `v*` tag is the only trigger.

## Broker-granted HTTP

The supported release/OCI component is **`python-provider.wasm`**, exposing **`python.eval`**
and command word **`python`**. Cargo feature `http` is default-on and keeps Dekopon-specific
HTTP code clearly separated; disabling defaults is a developer customization, not a supported
CI or distribution variant. Ordinary Cargo builds include `dekopon_requests`.
The combined component imports exactly `dekopon:http/client@1.1.0.send`,
`dekopon:clock/monotonic@1.1.0.now-nanos`, `dekopon:clock/wall@1.1.0.now-unix-millis`, and
`dekopon:random/source@0.1.0.get-random-bytes`. The latter three power SQL internals and
getrandom 0.3; the VM hash seed remains deliberately fixed. There is no Python-visible time,
random, filesystem, or socket API. A candidate broker host with these imports must load the
component, even for pure scripts; HTTP is still invocation-granted. Bare empty-linker Wasmtime
cannot instantiate it.

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
- Every request carries the fixed `User-Agent: dekopon-provider-python/<version>`; public APIs
  such as crates.io refuse requests without one.
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

The shared workflow must reproduce the pinned combined candidate before alpha publication.
Provider-owned `tests/component_contract.rs` checks the exact four-import authority and WIT shape.
`tests/requests.rs` uses FakeBroker for no-grant denial and its real registry with published
`AuthorizationGate`/HTTP constraints for the controlled-server tests. It does not test Cedar policy
selection; it tests production host enforcement of preauthorized grants without a transport mock.

## Combined in-memory alpha facade

`dekopon_tables.query(sql, tables)` takes one SELECT query and a dict of named row-array tables
(e.g. `{'people': [{'id': 1, 'group': 'a'}]}`). Each nonempty table has matching column keys in
every row and homogeneous bool, signed integer, finite float or text columns; nulls are allowed
with a non-null type witness. Names are short ASCII identifiers. It returns
`{'columns': [...], 'rows': [[...], ...]}` of bounded JSON-safe values. Up to 4 tables, 8 columns
per table, 256 input rows total, 128 KiB table JSON, 4 KiB SQL, 256 output rows, 16 result
columns and 128 KiB result; output types outside the safe scalar/date projection are errors.
DDL, DML, multi-statement SQL, COPY and external table scans are refused. The per-call DataFusion
runtime has disk manager disabled, one query partition and an 8 MiB query memory pool (not a
whole-process allocation ceiling). No table survives an invocation; no CSV/path/Parquet scans,
persistent dataframe object, or binary export is exposed.

`dekopon_numeric` provides `vector_add`, `dot`, `matrix_multiply`, `column_means`, `normal_cdf`,
`linear_regression` (training-set predictions) and `kmeans` (training-set labels; explicit u64
seed). Inputs are bounded exact list/tuple numeric arrays with finite values of magnitude <=1e6.
See source constants for shape/work bounds; output is JSON-safe, not a NumPy/sklearn model.
Neither module permits direct import of backend packages. The original result/stdout/HTTP ceilings
and guarded import registry remain unchanged.

This is an **alpha engine integration**, not the completed toolkit in external `SCOPE.md`:
in-memory streams/collections/math/decimal/CSV/date parsing, URL/HTML/Unicode/fuzzy extraction,
compression, general table construction/Arrow/Parquet/CSV bytes, chart/report generation,
spreadsheet/PDF/NLP/chemistry/embedding/model workflows, asset input/output and notebook package
API compatibility remain unimplemented. SQL wall time is statement-stable and does not introduce
`datetime.now`. No Turso or path storage is included.

## Build

Required versions are Rust 1.98.1, target `wasm32-unknown-unknown`, and wasm-tools 1.259.0. The
component is generated and ignored; it must never be committed. Build, lint, dependency-policy,
and component checks all run through the shared
[`dekopon-agents/provider-workflows`](https://github.com/dekopon-agents/provider-workflows) CI
(`ci / validate`), which `.github/workflows/ci.yml` calls. To build and test locally:

```console
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo deny --all-features check bans licenses sources advisories
# The supported shared CI builds, componentizes, verifies and reproduces the release artifact.
# For local source validation use ordinary Cargo with the worktree's default target; do not
# execute local build scripts that override compiler, wrapper or target settings.
```

`../provider-workflows/build.sh` is a sibling checkout of the shared workflows repository (see its
own README for the exact clone step CI uses); it writes `python-provider.wasm` and its checksum,
and componentizes it. The shared workflow validates it. Two RustPython 0.5.0 crates are vendored under `patches/` with one
reproducibility fix each: `rustpython-derive-impl` sorts `py_freeze!` module traversal and every
map/set-backed macro token emission instead of compiling randomly seeded collection order, and
`rustpython-vm`'s build script no longer freezes every visible build variable (paths, and
potentially credentials) into `_sysconfigdata` or stamps the crate with the containing repository's
`git describe`. With both, the shared workflow's byte-for-byte rebuild from a clean checkout matches.

The official Wasm is always distributed with a CycloneDX SBOM, generated by the shared release
workflow as a release asset rather than a local or tracked file. There is no local `scripts/`
directory or `build.sh` in this repository anymore.

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
DEKOPON_PROVIDER_COMPONENT=$PWD/python-provider.wasm cargo test --locked --test broker --test requests
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

Formatting, clippy, `cargo deny`, the reproducible component build, and the test suite are all
gated by the shared `ci / validate` workflow rather than local scripts; see [Build](#build) for
the local build and test commands. Component tests require `DEKOPON_PROVIDER_COMPONENT` and fail
when it is unset; their contract fixtures also require `python3` and the pinned `wasm-tools`.

## License and corresponding source

Original source authored by this project remains **MIT OR Apache-2.0** (`LICENSE-MIT` and
`LICENSE-APACHE`). The distributed combined Wasm embeds four Malachite 0.9.2 packages under
**LGPL-3.0-only** and `r-efi` under **LGPL-2.1-or-later**; `deny.toml`'s named exceptions disclose
them. This does not relicense the original project source, but the embedded code and combined
distribution carry the applicable third-party terms. Verbatim GNU texts are in `LICENSE-LGPL-3.0`
and `LICENSE-GPL-3.0` (with `LICENSE-LGPL-2.1` for the locked `r-efi` source packages).

Corresponding source is the public tagged source tree itself, reproducibly rebuilt by the shared
`ci / validate` and release workflows; the CycloneDX SBOM published with each release lists every
embedded package. No `latest` tag is published.

### Sticky host HTTP refusals

Policy violations (including absent/wrong grants), malformed host requests,
and host byte/call-budget exhaustion mark the invocation rejected. Although `send` returns a typed
error to Python, the real broker checks that sticky state after guest execution and returns
`HostCallRejected` instead of a successful guest envelope, even if the script catches the exception.
Transport/protocol failures and provider-local JSON/status/body-limit errors do not replenish any
budget but are ordinary bounded guest exceptions. A `maxResponseBytes` host refusal is a host error;
the provider's smaller body cap is checked only after a host-accepted response.

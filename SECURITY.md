# Security policy

## Supported version

Security fixes are accepted for the newest released minor line. The owner has accepted the exact
LGPL-3.0-only Malachite packages and the corresponding-source/relinkability design for this
standalone optional provider. That is a project policy decision, not a claim of attorney review. The
repository variable and immutable release gates remain mechanical publication controls.

Report suspected vulnerabilities privately through GitHub's security-advisory interface for
`dekopon-agents/dekopon-provider-python`. Do not include secrets, production scripts, or private
provider outputs in a public issue.

## Broker-granted HTTP boundary

Default-on Cargo feature `http` keeps the Dekopon-specific code boundary and adds only the
`dekopon:http/client@1.0.0` external import through published `dekopon-provider-http`. The native
`dekopon_requests` module exposes GET/HEAD only. It implements no socket/transport, dispatcher,
proposal engine, redirects, retry, cookie jar, ambient proxy, or credential API. Every call is
checked against the host's existing invocation grant (not a new Cedar decision). The host enforces
exact destination/method, DNS/IP policy, cumulative call budget, bounded request/response and timeout;
catching an exception cannot replenish the host budget. No grant denies all requests. Do not bind
credentials or secret-use authority to this capability. The provider cannot narrow a compromised
host; admit the component digest and configure the host limits explicitly.

URLs are capped at 8,192 UTF-8 bytes, response bodies at 131,072 bytes after the host's own
pre-import byte bound, and JSON uses the existing safe-value limits. Host exception messages are
reduced to stable error codes; HTTP status and JSON errors use short static diagnostics. Binary
content stays bytes; text decodes UTF-8 with replacement. Stdout and complete capability output
retain their existing bounds. Data failures may follow successful network requests and do not imply
rollback. Resource traps stay host errors. There are no runtime OS-exit semantics.

There is one supported release/OCI component, `python-provider.wasm`, one capability,
`python.eval`, and command word `python`. `scripts/assert-component-contract.sh` validates the
sole raw guest import and exact full external WIT, rejecting WASI and extra authority.
Componentizer adapter imports are internal to the validated component. Corresponding-source,
SBOM, network-disconnected vendor relinking and byte reproduction cover this HTTP component.
Disabling default features is only a developer customization, not a distribution branch.

## Authority boundary

The security boundary is the validated component plus a correctly configured Dekopon host, not the
Python import hook:

- the component imports only `dekopon:http/client@1.0.0`, linked by the real broker;
- there is no WASI adapter, JavaScript/browser binding, environment, filesystem, raw socket,
  storage, clock, entropy, subprocess, dynamic-library, or provider-dispatch import;
- `allow_external_library` is false and the exact public import names are `json`, `re`, `yaml`, and
  `dekopon_requests` only; private dependency modules are preloaded below the guest-visible import guard;
- `open`, `input`, `breakpoint`, `compile`, `eval`, and `exec` are removed after trusted frozen
  modules are preloaded; no original privileged callable is retained on a Python-reachable object;
- the Python-visible module registry is replaced with the four exact public modules, and denied
  transitive module references are removed from loaded module namespaces;
- `sys`, `os`, `pathlib`, `time`, `random`, `secrets`, `socket`, `ssl`, `sqlite3`, `subprocess`,
  `threading`, `ctypes`, `tkinter`, and `webbrowser` are denied;
- the `python` command word's `run-command` export only parses argv. It renders help and usage
  errors, or returns a `python.eval` proposal that the host authorizes exactly like a direct call;
  it constructs no VM and grants nothing.

Python introspection is not a capability boundary. A script might find implementation objects or
consume CPU/memory, but host HTTP grants remain authoritative for every request. Pure scripts need no HTTP grant. Admit only the
trusted release digest; compilation happens outside invocation fuel, linear-memory, and deadline
limits.

## Provider-enforced limits

Each invocation constructs a fresh RustPython 0.5.0 VM and scope, fixes the hash seed, sets Python
recursion to 200, initializes `result = None`, and compiles the supplied source only as
`Mode::Exec`. State is never reused.

| Value | Enforced limit |
|---|---:|
| script | 65,536 UTF-8 bytes |
| captured stdout | 65,536 UTF-8 bytes, boundary-safe truncation |
| YAML input | 65,536 UTF-8 bytes |
| YAML output | 65,536 UTF-8 bytes through a bounded writer |
| safe-value depth | 32 |
| safe-value nodes (keys included) | 10,000 |
| safe result encoding | 131,072 bytes |
| integer range | ±9,007,199,254,740,991 |
| diagnostic message | 2,048 UTF-8 bytes |
| complete capability response | 786,432 bytes |

Safe results are exactly null, bool, finite f64, safe integers, UTF-8 strings, exact list/tuple
arrays, and exact string-keyed dicts. Cycles, subclasses used as containers, custom conversion
hooks, non-finite floats, and unsupported objects are rejected.

The YAML loader tokenizes and parses before construction, rejects directives, aliases, anchors,
tags, merge keys, duplicate keys, complex/non-string keys, multiple documents, non-finite or
out-of-range numbers, excess depth/nodes, and oversized text. Timestamp-like plain scalars remain
strings. The dumper first applies the same safe-value walk and can construct no tag or alias.

The custom `getrandom 0.3.4` backend is deterministic and non-cryptographic. It exists only for VM
internals, while the VM uses an explicit fixed hash seed. No Python entropy surface is exposed.
Host fuel and deadlines, rather than hash randomization, bound adversarial algorithms.

RustPython 0.5.0's build script copies its complete build environment into frozen
`_sysconfigdata`. The release builder therefore compiles a clean fixed-path source snapshot under
an explicit non-secret environment and rejects sensitive key markers in the artifact. Its complete
local `rustpython-derive-impl 0.5.0` patch also replaces randomized `py_freeze!` module ordering and
map/set-backed macro token emission with ordered traversal; it does not remove or rewrite Python or
Rust code or data. Running a plain release Cargo build is useful as a compile gate but is **not** an
approved distributable build;
only `scripts/build-component.sh` produces the scrubbed component.

Every official component is bound by checksums and OCI annotations to a versioned corresponding-
source archive. That archive includes the exact provider source and complete vendored lockfile
closure, uses offline Cargo source replacement, and documents how to modify Malachite and relink a
new component. CI performs that modification and clean offline rebuild. The source archive/SBOM
and component are generated release products and are never trusted merely because they exist in a
working tree; release gates verify their bytes, manifests, and anonymous retrieval paths.

## Host-enforced termination

Provider code does **not** enforce instruction fuel, wall time, or linear memory and cannot turn a
Wasmtime trap into a data envelope. Fuel exhaustion, epoch/Tokio deadline cancellation, memory
allocation failure, and host input/output refusal remain host execution errors.

An empty-linker immediate host cannot instantiate this component. Use the real broker with
explicit HTTP linking and the dedicated profile in `docs/deployment-profile.md`: 64 MiB per
memory, 1,000,000,000 fuel, 5,000 ms timeout, and 786,432-byte output limit. The 10,000,000
and 50,000,000 fuel probes intentionally fail safely during VM startup in real-host tests.

Broker defaults retain the same memory/table/count/input/output ceilings, provide 8,000,000,000
fuel, and accept authorization timeouts no greater than 30 seconds. Every invocation gets a fresh
store, async yields occur at most every `min(fuel, 10,000)` units, and Tokio applies the timeout.
The broker linker implements Dekopon HTTP/storage interfaces; this component imports only HTTP.
Use a 5,000 ms authorization timeout and 786,432-byte output authorization.

A 64 MiB memory limit is per linear memory, not process RSS. With no `maxTotalMemoryBytes`, it is
not an aggregate process bound. Compiled code, host allocations, and up to four memories sit outside
that number. Size broker connection count, aggregate admission, and container memory from the
measurements in `docs/deployment-profile.md`.

## Failure model

Malformed capability input and unknown capabilities are stable SDK `ProviderError` failures.
Python syntax, runtime, YAML, and result-conversion failures return bounded data with
`ok: false`; tracebacks, locals, and stderr are omitted. Host fuel/deadline/memory/output failures
trap outside that envelope. Failures can follow completed HTTP requests; they do not imply rollback.
Filesystem and generic provider calls remain unavailable.

### Sticky host HTTP refusals

Policy violations (including absent/wrong grants), malformed host requests,
and host byte/call-budget exhaustion mark the invocation rejected. Although `send` returns a typed
error to Python, the real broker checks that sticky state after guest execution and returns
`HostCallRejected` instead of a successful guest envelope, even if the script catches the exception.
Transport/protocol failures and provider-local JSON/status/body-limit errors do not replenish any
budget but are ordinary bounded guest exceptions. A `maxResponseBytes` host refusal is a host error;
the provider's smaller body cap is checked only after a host-accepted response.

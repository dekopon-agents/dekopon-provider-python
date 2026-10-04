# Security policy

## Supported version

Security fixes are accepted for the newest released minor line. The owner has accepted the exact
LGPL-3.0-only Malachite packages and the corresponding-source/relinkability design for this
standalone optional provider. That is a project policy decision, not a claim of attorney review. The
shared release workflow gates remain mechanical publication controls.

Report suspected vulnerabilities privately through GitHub's security-advisory interface for
`dekopon-agents/dekopon-provider-python`. Do not include secrets, production scripts, or private
provider outputs in a public issue.

## Broker-granted HTTP boundary

Default-on Cargo feature `http` keeps the Dekopon-specific HTTP code boundary. The typed
provider world imports stdio, HTTP, both clocks and OS entropy (the complete list is below). The native
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
`python.eval`, and command word `python`. `tests/component_contract.rs` validates the
decoded component imports, rejecting WASI and the old provider world.
Componentizer adapter imports are internal to the validated component. Corresponding-source,
SBOM and shared byte reproduction cover this combined component.
Disabling default features is only a developer customization, not a distribution branch.

## Authority boundary

The security boundary is the validated component plus a correctly configured Dekopon host, not the
Python import hook:

- the component imports exactly `dekopon:stdio/streams@0.1.0`,
  `dekopon:http/client@1.1.0`, `dekopon:clock/wall@1.1.0`,
  `dekopon:clock/monotonic@1.1.0`, `dekopon:random/source@0.1.0`, and
  `dekopon:spawn/run@0.1.0`;
- there is no WASI adapter, JavaScript/browser binding, environment, filesystem, raw socket,
  storage, OS-subprocess, dynamic-library, or generic provider-dispatch import; the clocks and
  entropy are available to native code through broker handles, not Python import authority;
- `allow_external_library` is false and the exact public import names are `json`, `re`, `yaml`,
  `dekopon_requests`, `dekopon_subshell`, `dekopon_tables`, and `dekopon_numeric`; private
  dependency modules are preloaded below the guest-visible import guard;
- `open`, `input`, `breakpoint`, `compile`, `eval`, and `exec` are removed after trusted frozen
  modules are preloaded; no original privileged callable is retained on a Python-reachable object;
- the Python-visible module registry is replaced with the seven exact public modules, and denied
  transitive module references are removed from loaded module namespaces;
- `sys`, `os`, `pathlib`, `time`, `random`, `secrets`, `socket`, `ssl`, `sqlite3`, `subprocess`,
  `threading`, `ctypes`, `tkinter`, and `webbrowser` are denied;
- the `python` command word's `run-command` export only parses argv. It renders help and usage
  errors, or returns a `python.eval` proposal that the host authorizes exactly like a direct call;
  it constructs no VM and grants nothing. Bare/`-` proposals carry only an stdin marker; the
  capability reads at most 65,537 script bytes after broker authorization. HTTP and spawn callbacks
  access invocation-scoped handles, which drop guards clear on all exits.

Python introspection is not a capability boundary. A script might find implementation objects or
consume CPU/memory, but host HTTP grants remain authoritative for every request, and child
scripts inherit the same agent, grants, tree deadline and call budget. `dekopon_subshell.run` is
not an OS subprocess: it requires spawn authority and each child call remains host-authorized.
Pure scripts need neither HTTP nor child-script grants. Admit only the
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
| child script / child stdout | 65,536 UTF-8 bytes each; child stdout boundary-safe truncation |
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

The custom `getrandom 0.3.4` backend reaches the broker's OS entropy through an invocation-scoped
SDK `Random` handle; without an installed handle it fails rather than falling back. The VM hash
seed remains explicitly fixed and independent. No Python entropy surface is exposed. Host fuel
and deadlines, rather than hash randomization, bound adversarial algorithms.

The vendored `rustpython-vm 0.5.0` build-script patch writes an empty `_sysconfigdata` table
instead of freezing the build environment, and uses constant git stamps. Ordinary Cargo builds
therefore no longer embed ambient build variables. A source regression checks that patch.
The `rustpython-derive-impl 0.5.0` patch orders frozen module and macro traversal. The shared
`provider-workflows/build.sh` uses the checked-in wasm `rustflags` and reproducible compiler
settings; CI and release rebuild independently and compare bytes.

Corresponding source is the public tagged tree, with the CycloneDX SBOM listing the locked
packages. Shared release gates verify the component checksum, provenance and published bytes.
There is no provider-local source-bundle or canonical-environment build pipeline.

## Host-enforced termination

Provider code does **not** enforce instruction fuel, wall time, or linear memory and cannot turn a
Wasmtime trap into a data envelope. Fuel exhaustion, epoch/Tokio deadline cancellation, memory
allocation failure, and host input/output refusal remain host execution errors.

An empty-linker immediate host cannot instantiate this component. Use the real broker with
linking of the six declared interfaces and the Pi-limit evidence in
`docs/deployment-profile.md`: 64 MiB per memory, 24,000,000,000 fuel, 300,000 ms timeout,
and 12,582,912-byte input/output limits. The 10,000,000
and 50,000,000 fuel probes intentionally fail safely during VM startup in real-host tests.

Broker defaults retain the memory/table/count and 1 MiB input/output ceilings, provide
8,000,000,000 fuel and a 30-second timeout; the Pi explicitly configures higher fuel, timeout
and I/O ceilings. Every invocation gets a fresh
store, async yields occur at most every `min(fuel, 10,000)` units, and Tokio applies the timeout.
The broker linker provides the six declared interfaces; this component imports no storage.
Grant only the intended HTTP authority and retain explicit host resource ceilings.

A 64 MiB memory limit is per linear memory, not process RSS. The Pi configures
`maxTotalMemoryBytes: 268435456` as an aggregate guest reservation, not an RSS bound. Compiled code, host allocations, and up to four memories sit outside
that number. Size broker connection count, aggregate admission, and container memory from the
measurements in `docs/deployment-profile.md`.

## Failure model

Malformed capability input and unknown capabilities are stable SDK `ProviderError` failures.
Python syntax, runtime, YAML, and result-conversion failures return bounded data with
`ok: false`; tracebacks, locals, and stderr are omitted. Host fuel/deadline/memory/output failures
trap outside that envelope. Failures can follow completed HTTP requests; they do not imply rollback.
Filesystem and generic provider calls remain unavailable. A child script may run through the
scoped spawn handle only. `run(script, stdin=None|INHERIT)` accepts no `input=` bytes; inherit
hands the child the rest of the parent's stdin, which the host may read ahead, so the parent
must not read stdin afterwards. Child stdout is capped at 65,536 bytes with a truncation flag;
non-empty child output ends in a newline, child stderr is empty in core 0.33.0, and child
panic is returned as status 70. Nonzero status is data, not a Python exception. Invalid
stdin raises `TypeError`; absent/refused spawn or read failure raises `SubshellError`.

### Sticky host HTTP refusals

Policy violations (including absent/wrong grants), malformed host requests,
and host byte/call-budget exhaustion mark the invocation rejected. Although `send` returns a typed
error to Python, the real broker checks that sticky state after guest execution and returns
`HostCallRejected` instead of a successful guest envelope, even if the script catches the exception.
Transport/protocol failures and provider-local JSON/status/body-limit errors do not replenish any
budget but are ordinary bounded guest exceptions. A `maxResponseBytes` host refusal is a host error;
the provider's smaller body cap is checked only after a host-accepted response.

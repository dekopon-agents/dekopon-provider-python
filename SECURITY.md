# Security policy

## Supported version

Security fixes are accepted for the unreleased 0.1.x line. **v0.1.0 publication is currently on
hold** because the statically linked LGPL-3.0-only Malachite packages do not yet have an approved
source/relinkability distribution design.

Report suspected vulnerabilities privately through GitHub's security-advisory interface for
`dekopon-agents/dekopon-provider-python`. Do not include secrets, production scripts, or private
provider outputs in a public issue.

## Authority boundary

The security boundary is the validated component plus a correctly configured Dekopon host, not the
Python import hook:

- the component and every nested core module have zero imports;
- there is no WASI adapter, JavaScript/browser binding, environment, filesystem, network, HTTP,
  storage, clock, entropy, subprocess, dynamic-library, or provider-dispatch import;
- `allow_external_library` is false and the public import roots are `json`, `re`, and `yaml` only;
- `open`, `input`, and `breakpoint` are removed; guest `compile`, `eval`, and `exec` are guarded and
  denied outside trusted frozen-module initialization;
- `sys`, `os`, `pathlib`, `time`, `random`, `secrets`, `socket`, `ssl`, `sqlite3`, `subprocess`,
  `threading`, `ctypes`, `tkinter`, and `webbrowser` are denied.

Python introspection is not a capability boundary. A script might find implementation objects or
consume CPU/memory, but a zero-import store gives those objects no host authority. Admit only the
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
an explicit non-secret environment and rejects sensitive key markers in the artifact. Running a
plain release Cargo build is useful as a compile gate but is **not** an approved distributable
build; only `scripts/build-component.sh` produces the scrubbed component.

## Host-enforced termination

Provider code does **not** enforce instruction fuel, wall time, or linear memory and cannot turn a
Wasmtime trap into a data envelope. Fuel exhaustion, epoch/Tokio deadline cancellation, memory
allocation failure, and host input/output refusal remain host execution errors.

Dekopon 0.11.1 immediate defaults are 67,108,864 bytes per memory, four memories, 100,000 table
elements, 16 tables, 64 core instances, 1,048,576-byte input, 1,048,576-byte manifest/output,
10,000,000 fuel, and 30 seconds. It serializes execution, creates a fresh store, uses an empty
linker, and interrupts deadlines by epoch. Memory/input/output/fuel/timeout have CLI flags; table
and instance ceilings do not.

The exact final RustPython artifact consumes more than the immediate host's 10,000,000-fuel
default during VM startup. That default therefore fails safely with `OutOfFuel` before user code.
Use the documented dedicated profile (`--fuel 500000000`, `--timeout-ms 5000`) rather than treating
the default as supported.

Broker defaults retain the same memory/table/count/input/output ceilings, provide 8,000,000,000
fuel, and accept authorization timeouts no greater than 30 seconds. Every invocation gets a fresh
store, async yields occur at most every `min(fuel, 10,000)` units, and Tokio applies the timeout.
The broker linker implements only Dekopon HTTP/storage interfaces; this component imports neither.
Use a 5,000 ms authorization timeout and 786,432-byte output authorization.

A 64 MiB memory limit is per linear memory, not process RSS. With no `maxTotalMemoryBytes`, it is
not an aggregate process bound. Compiled code, host allocations, and up to four memories sit outside
that number. Size broker connection count, aggregate admission, and container memory from the
measurements in `docs/deployment-profile.md`.

## Failure model

Malformed capability input and unknown capabilities are stable SDK `ProviderError` failures.
Python syntax, runtime, YAML, and result-conversion failures return bounded data with
`ok: false`; tracebacks, locals, and stderr are omitted. Host fuel/deadline/memory/output failures
trap outside that envelope. None of these failures imply that network, filesystem, or another
provider was contacted: the component has no import through which that could occur.

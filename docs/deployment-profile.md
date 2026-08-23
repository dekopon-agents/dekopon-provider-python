# Deployment profile (v0.1.0 release candidate)

This profile separates guest-enforced data bounds from host-enforced execution termination. It is
for the exact locked Rust 1.97.0 / wasm-tools 1.236.1 artifact and must be regenerated after a
source, lockfile, compiler, or componentizer change.

## Selected host settings

| Setting | Selected value | Owner |
|---|---:|---|
| per-memory maximum | 67,108,864 bytes | broker `hostLimits` |
| memories / tables / instances | 4 / 16 / 64 | broker `hostLimits` defaults |
| table elements | 100,000 | broker `hostLimits` default |
| invocation input ceiling | 1,048,576 bytes | broker `hostLimits` |
| manifest/output host ceiling | 1,048,576 bytes | broker `hostLimits` |
| dedicated fuel | 1,000,000,000 | broker `hostLimits` (global) |
| global timeout ceiling | 30,000 ms | broker `hostLimits` |
| capability timeout | 5,000 ms | authorization constraint |
| capability output | 786,432 bytes | authorization constraint |
| HTTP / storage | none | component imports none |

The broker's default 2 MiB frame exceeds the required output-plus-64-KiB margin; the protocol hard
maximum is 16 MiB. `hostLimits` is all-or-nothing global configuration. Per-capability policy may
narrow timeout/output/HTTP/storage, but not memory or fuel.

Immediate CLI tests use 500,000,000 fuel, 5,000 ms, 64 MiB per memory, and 786,432 output bytes.
The immediate host's 10,000,000-fuel default is intentionally tested as a safe failure and is not a
working profile for RustPython startup.

## Measured artifact

Measured on the final review build with Rust 1.97.0, wasm-tools 1.236.1, and `dekopon-run 0.11.1`
on an Apple-silicon Mac (2026-08-23):

| Measurement | Result |
|---|---:|
| raw core | 20,304,110 bytes |
| component | 20,303,790 bytes |
| SHA-256 | `5f1938396794af6cbd522bd031e0a729cca59413742f98cda36a30cac308f713` |
| component/core imports | 0 / 0 |
| core memories | 1, minimum 150 pages (9,830,400 bytes), host-capped |
| core tables | 1, fixed 6,015 funcrefs |
| 50,000,000 fuel | `OutOfFuel` during startup |
| 100,000,000 fuel | normal `result = 2` success |
| three warm calls at 100M | 11.38–14.72 ms, 12.75 ms mean |
| cold CLI process | 1.85 s real |
| cold maximum resident set size (`time -l`) | 598,540,288 bytes |
| cold Darwin peak-memory-footprint counter | 447,791,968 bytes |

`./scripts/measure-final-artifact.sh python-provider.wasm` writes the machine-readable record to
`/tmp/dekopon-python-measurements.json`, captures raw core declarations, repeats the fuel bracket,
and measures cold/warm execution. CI uploads that record with its ignored review artifact. The
older feasibility baseline (23.3 MB, 2.13 s cold CLI, 567,230,464-byte peak host RSS) is superseded
for this source tree and was never a release quota.

## Admission and process memory

A 64 MiB limit applies to each linear memory, not process RSS. `maxTotalMemoryBytes` defaults to
absent and reserves one `maxMemoryBytes` unit per live store; it does not account for four memories,
compiled code, Cranelift/component compilation, or host allocations. Compilation is also outside
fuel and invocation deadlines.

The measured single cold compiler process already reached roughly 599 MB RSS. Until
platform-specific RSS and concurrency load tests establish a tighter number, budget at least
**768 MiB plus admitted concurrent guest reservations** for one compiler/connection profile; do
not derive a container limit from the 64 MiB store ceiling alone.

1. admit only the trusted component digest;
2. use a persistent broker-owned Wasmtime compilation cache;
3. set `maxTotalMemoryBytes` to `maxConnections × 67,108,864` or lower;
4. size the container above measured compiled-artifact RSS plus that admitted guest reservation;
5. keep connection count low enough that concurrent cold compilation cannot OOM the process.

RPi latency and aggregate concurrency are deployment measurements, not inferred from the Mac
measurement. Publication remains held until those measurements and the LGPL policy decision are
reviewed by the owner.

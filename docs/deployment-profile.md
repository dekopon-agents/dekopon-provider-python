# Deployment profile

This profile separates guest-enforced data bounds from host-enforced execution termination. It is
for the exact locked Rust 1.98.1 / wasm-tools 1.259.0 artifact and must be regenerated after a
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

`tests/broker.rs` drives the real broker host at 1,000,000,000 fuel, 5,000 ms, 64 MiB per memory,
and 786,432 output bytes. The immediate host's 10,000,000-fuel default, and 50,000,000, are
intentionally asserted as safe failures and are not working profiles for RustPython startup.

## Measured artifact

Measured on the v0.4.0 release run's `ubuntu-24.04` build, the release platform, with Rust 1.98.1
and wasm-tools 1.259.0 (2026-09-13), on the 0.15.0 SDK; this is the exact artifact
`ghcr.io/dekopon-agents/provider-python:0.4.0`'s `application/wasm` layer ships. The component
bytes are reproducible per platform: every Linux checkout at this commit measures the same sizes
and digest. A local macOS build of the same commit is 492 bytes larger (component 20,574,471
bytes), with its own digest `7c32d685f20620691cb9716ec6bc4e1bad269ad29462641d37021b54c150c017`,
and this release needs the same 151-page memory minimum on both platforms.

| Measurement | Result |
|---|---:|
| raw core | 20,574,031 bytes |
| component | 20,573,979 bytes |
| SHA-256 | `d6d0e15c8354e1fc4df5c410cfc0fa25ac11e6f06b972a822176adffca3ee9a5` |
| component/core imports | 0 / 0 |
| core memories | 1, minimum 151 pages (9,895,936 bytes), host-capped |
| core tables | 1, fixed 6,091 funcrefs |
| 10,000,000 fuel | `OutOfFuel` during startup |
| 50,000,000 fuel | `OutOfFuel` during startup |
| 1,000,000,000 fuel | normal `result = 2` success |

The measured artifact table above is a manually recorded snapshot, not a generated file; there is
no local measurement script anymore. The fuel bracket in the table is asserted against the real
broker host by `tests/broker.rs`.

## Admission and process memory

A 64 MiB limit applies to each linear memory, not process RSS. `maxTotalMemoryBytes` defaults to
absent and reserves one `maxMemoryBytes` unit per live store; it does not account for four memories,
compiled code, Cranelift/component compilation, or host allocations. Compilation is also outside
fuel and invocation deadlines.

A single cold compiler process was measured at up to roughly 591 MB RSS on the v0.1.0 build, when
a command-line host still existed to measure it under `/usr/bin/time`. The component has grown by
about 270 KB (1.3%) since, mostly clap for the `python` command word, so that figure is an estimate
for this build rather than a bound. Until platform-specific RSS and
concurrency load tests establish a tighter number, budget at least
**768 MiB plus admitted concurrent guest reservations** for one compiler/connection profile; do
not derive a container limit from the 64 MiB store ceiling alone.

1. admit only the trusted component digest;
2. use a persistent broker-owned Wasmtime compilation cache;
3. set `maxTotalMemoryBytes` to `maxConnections × 67,108,864` or lower;
4. size the container above measured compiled-artifact RSS plus that admitted guest reservation;
5. keep connection count low enough that concurrent cold compilation cannot OOM the process.

RPi latency and aggregate concurrency are deployment measurements, not inferred from the Mac
measurement. The owner has accepted the LGPL distribution decision; RPi measurements do not
reopen it or hold publication. They remain a separate production-deployment/admission gate.

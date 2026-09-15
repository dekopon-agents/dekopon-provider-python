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
| HTTP | narrow invocation grant; absent denies requests | real broker HTTP linker |
| wall time | invoke-only, no new grant or budget | broker clock linker |
| storage | none | no storage import |

The broker's default 2 MiB frame exceeds the required output-plus-64-KiB margin; the protocol hard
maximum is 16 MiB. `hostLimits` is all-or-nothing global configuration. Per-capability policy may
narrow timeout/output/HTTP/storage, but not memory or fuel.

`tests/broker.rs` drives the real broker host at 1,000,000,000 fuel, 5,000 ms, 64 MiB per memory,
and 786,432 output bytes. The immediate host's 10,000,000-fuel default, and 50,000,000, are
intentionally asserted as safe failures and are not working profiles for RustPython startup.

## Measured artifact

The HTTP + clock component replaces the earlier HTTP-only and v0.4.0 zero-import artifacts. Do not reuse its
size, digest, table declarations or memory minimum as measurements of this build. The exact-head
CI review artifact contains the current size/digest record. Bytes are reproducible per platform,
not promised identical across macOS and Linux.

The contract gate enforces exactly two external interfaces (`dekopon:http/client@1.0.0` and
`dekopon:clock/wall@1.0.0`) and two raw core function imports (`send` and `now-unix-millis`). `tests/broker.rs` asserts 10M/50M fuel failures and normal operation
at 1G fuel and 64 MiB; `tests/requests.rs` exercises real multi-request grants on the same artifact.
Pure scripts require no HTTP grant, but all invocations require HTTP and clock linking by the broker.
`dekopon_date.now_unix_millis()` returns exact integer UTC Unix epoch milliseconds; the clock is
invoke-only, requires no grant, and may move backward. Neither preload nor command/describe reads it.

The shared CI log records the component size, imports and checksum; the uploaded component and
checksum sidecar identify the exact build. The fuel bracket is asserted by `tests/broker.rs`.

## Admission and process memory

A 64 MiB limit applies to each linear memory, not process RSS. `maxTotalMemoryBytes` defaults to
absent and reserves one `maxMemoryBytes` unit per live store; it does not account for four memories,
compiled code, Cranelift/component compilation, or host allocations. Compilation is also outside
fuel and invocation deadlines.

A single cold compiler process was measured at up to roughly 591 MB RSS on the v0.1.0 build, when
a command-line host still existed to measure it under `/usr/bin/time`. That historical figure is not a measurement or bound for the current HTTP + clock component. Until platform-specific RSS and
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

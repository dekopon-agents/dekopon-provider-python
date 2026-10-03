# Deployment profile — v0.7.0 combined toolkit

The provider combines a fresh RustPython interpreter, in-memory DataFusion SQL and bounded
numeric functions. It has not been deployed to the Pi. The broker's configured limits and
measured component properties below are different kinds of evidence: an artifact file size is not
a linear-memory bound, and a per-memory cap is not a container-RSS or aggregate-admission cap.

## Decoded import contract and artifact

`wasm-tools component wit python-provider.wasm` decodes exactly these imports:

| Interface | Use |
|---|---|
| `dekopon:stdio/streams@0.1.0` | typed stdin/stdout |
| `dekopon:http/client@1.2.0` | authorized `dekopon_requests` GET/HEAD |
| `dekopon:clock/wall@1.1.0` | DataFusion raw wall-clock import |
| `dekopon:clock/monotonic@1.1.0` | DataFusion raw monotonic import |
| `dekopon:random/source@0.1.0` | invocation-scoped SDK OS entropy |

No WASI, storage, filesystem, process or socket import is present. The clocks are declared
because DataFusion imports them; the provider does not call their handles. The custom getrandom
backend holds the random handle only while `python.eval` runs and has no fallback source. The
RustPython hash seed remains fixed independently.

| Exact shared-build artifact | Value |
|---|---:|
| Size | 49,349,203 bytes (47.063 MiB) |
| SHA-256 | `ca63a6e178a0334833cb0f3c64fa5ac6494608bc5aaccdc70c6e23f26788ea05` |
| Artifact ceiling | 67,108,864 bytes (64 MiB); passes |

The release profile uses `opt-level = "s"`, fat LTO, one codegen unit, abort-on-panic and symbol
stripping. This digest identifies the measured local component; publication verifies the build
and checksum again. Core SDK/testkit are pinned to `0f93bcbbd1a2031f6693d06f66aa4ec012a01877`
and the DataFusion fork to `cf3778098ad3ea283ecd8ee2a991be7d9a29750c`.

## Broker profile and workload evidence

The Pi's `broker.d/host.yaml` sets 24,000,000,000 fuel, 300,000 ms maximum timeout and
12,582,912-byte input/output ceilings. It leaves the per-memory limit at the broker's default
67,108,864 bytes and sets aggregate `maxTotalMemoryBytes` to 268,435,456 bytes. Per-capability
HTTP constraints still authorize individual requests; pure scripts need no HTTP grant. The
in-memory SQL session disables disk spill, uses an explicit 8 MiB query pool, and accepts only one
SELECT over supplied tables; this pool does not include all Arrow and RustPython allocations.

The ignored `tests/profile.rs` workload test uses the real typed broker testkit at precisely those
fuel, timeout and I/O ceilings, first with 64 MiB per memory and, only upon failure, 128 MiB.
Both workloads construct a fresh interpreter: a supplied-table join/group/window SQL query and a
numeric batch with ndarray, statrs and SmartCore operations. Run with:

```console
DEKOPON_PROVIDER_COMPONENT=$PWD/python-provider.wasm cargo test -p dekopon-python-provider --locked --test profile -- --ignored --nocapture
```

| Workload | 64 MiB per memory | 128 MiB if needed |
|---|---|---|
| Join/group/window SQL | PASS | Not needed |
| Numeric batch | PASS | Not needed |

Both workloads passed at 64 MiB; 128 MiB was not run because the fallback condition did not
occur. These outcomes inform the owner's homelab limit decision; they do not change the Pi's
configuration or cut the toolkit. Host fuel, epoch deadline and memory traps remain host errors,
not guest JSON failures. The legacy v0.6.3 suites still cover small scripts, HTTP grants, sandbox
restrictions, fuel and deadline refusal under their own test settings.

## Admission and process memory

A per-memory 64 MiB cap applies separately to each Wasm linear memory. Compiled component code,
Cranelift, host allocations and multiple memories contribute to process RSS independently.
`maxTotalMemoryBytes` reserves guest memory across admitted stores; it is not an RSS ceiling. The
historical ~591 MB cold-compiler RSS observation was for the v0.1.0 build, not this component; do
not use it as a current measurement or derive a container limit from the artifact size. Pi RSS,
concurrency and cold-compile latency require deployment-specific measurement before changing
admission or container memory. No homelab change is made by this PR.

# Deployment profile — v0.8.0 combined toolkit and child scripts

The default provider combines a fresh RustPython interpreter, bounded numeric functions and
scoped child shell scripts. In-memory DataFusion SQL requires the optional `tables` feature. It has not been deployed to the Pi. The
broker's configured limits and measured component properties below are different kinds of evidence: an artifact file size is not
a linear-memory bound, and a per-memory cap is not a container-RSS or aggregate-admission cap.

## Decoded import contract and artifact

`wasm-tools component wit python-provider.wasm` decodes exactly these imports:

| Interface | Use |
|---|---|
| `dekopon:stdio/streams@0.1.0` | typed stdin/stdout |
| `dekopon:http/client@1.1.0` | authorized `dekopon_requests` GET/HEAD |
| `dekopon:clock/wall@1.1.0` | invocation-only SDK wall clock (`time.time()`) |
| `dekopon:clock/monotonic@1.1.0` | invocation-only SDK monotonic clock (`time.monotonic()`) |
| `dekopon:random/source@0.1.0` | invocation-scoped SDK OS entropy |
| `dekopon:spawn/run@0.1.0` | invocation-scoped `dekopon_subshell` child scripts |
| `dekopon:storage/durable-files@0.1.1` | separately authorized SQL capability; interim metadata probe only, no open or write |

No WASI, OS process or socket import is present. The separate fail-closed
`python.sqlite-eval` capability adds a durable-files import, but `python.eval`
remains read-only; no storage rollout is included in this interim build. Spawn invokes a Dekopon child
shell script on the same invocation authority, not an OS process import; it supplies no
new grants. With `INHERIT`, the child takes the rest of parent stdin (which may be read
ahead); do not read parent stdin after it returns. Non-empty child stdout ends in a
newline, stderr is empty in core 0.33.0, and a child panic returns status 70. The clocks are
read through SDK handles only during invocation; metadata never reads a clock. The custom getrandom
backend holds the random handle only while `python.eval` or `python.sqlite-eval` runs and has no fallback source. The
RustPython hash seed remains fixed independently.

| Historical v0.8.0 combined shared-build artifact (before default-off SQL) | Value |
|---|---:|
| Size | 49,370,395 bytes (47.083 MiB) |
| SHA-256 | `eba65311fffcdb1dc844b748a571f2deb7faa8383eb4d70325076f7cd427b9d3` |
| Artifact ceiling | 67,108,864 bytes (64 MiB); passes |

Current default and all-feature artifact hashes are feature-specific and must be verified
against their matching built-component tests; the historical hash above is not either build.

The release profile uses `opt-level = "s"`, fat LTO, one codegen unit, abort-on-panic and symbol
stripping. This is the local shared-build digest, not a published release digest; release validates the artifact and checksum again. Core SDK/testkit are pinned to the published 0.33.0 crates
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

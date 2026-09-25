# Opt-in engine composition experiment

This is an experimental, default-off build; it is **not** the shipped provider. Run from the provider repository root. Keep generated Wasm, WIT dumps, hashes and logs outside this repository (e.g. in a sibling design directory); never share Cargo targets or set a rustc wrapper. The fixture crates have independent default targets and committed locks. Use the pinned task-local WAC v0.11.0 executable (built from its locked source) for `WAC`.

```sh
# Set OUT to an absolute directory outside the repository, WAC to the pinned task-local binary.
RUSTFLAGS="$(cat rustflags)" cargo build --locked --release --target wasm32-unknown-unknown --features engine-swap
wasm-tools component new target/wasm32-unknown-unknown/release/dekopon_python_provider.wasm -o "$OUT/python.wasm"
shasum -a 256 "$OUT/python.wasm" > "$OUT/python-before.sha256"
for engine in a b; do
  (cd "experiments/engine-swap/engine-$engine" && cargo build --locked --release --target wasm32-unknown-unknown)
  wasm-tools component new "experiments/engine-swap/engine-$engine/target/wasm32-unknown-unknown/release/python_engine_$engine.wasm" -o "$OUT/engine-$engine.wasm"
  "$WAC" compose experiments/engine-swap/compose.wac --dep "experiment:python=$OUT/python.wasm" --dep "experiment:engine=$OUT/engine-$engine.wasm" -o "$OUT/composed-$engine.wasm"
  wasm-tools validate "$OUT/composed-$engine.wasm"
  wasm-tools component wit "$OUT/composed-$engine.wasm" > "$OUT/composed-$engine.wit"
done
shasum -a 256 "$OUT/python.wasm" > "$OUT/python-after.sha256"
cmp "$OUT/python-before.sha256" "$OUT/python-after.sha256"
cmp "$OUT/composed-a.wit" "$OUT/composed-b.wit"
DEKOPON_ENGINE_A="$OUT/composed-a.wasm" DEKOPON_ENGINE_B="$OUT/composed-b.wasm" DEKOPON_ENGINE_RAW="$OUT/python.wasm" cargo test --locked --features engine-swap --test engine_swap
```

`compose.wac` binds the *only* engine import explicitly and passes through HTTP as the sole outer import. The same path `$OUT/python.wasm` is supplied to both compositions, with its hash checked before and after. The focused Rust tests inspect the exact shipped external contract, reject unlinked raw facade loading, invoke both composed variants via FakeBroker, and exercise resource creation/get/drop, fresh invocation state, and engine-owned fuel loop and memory growth. The 64 MiB host memory bound is per memory; this test does **not** assert an aggregate bound across composed memories. Use a separate default-feature build and existing `broker`/`component_contract` tests to check the unchanged shipped provider. No all-features default test run is intended.

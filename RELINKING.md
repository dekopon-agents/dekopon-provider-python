# Corresponding source and relinking guide

## Important license notice

The distributed `python-provider.wasm` is a combined WebAssembly component that embeds the four
Malachite 0.9.2 crates listed in `THIRD_PARTY_NOTICES.md` under **LGPL-3.0-only**. The original
Dekopon Python provider source remains **MIT OR Apache-2.0**; those permissive terms do not replace
the terms that apply to embedded third-party code. `LICENSE-LGPL-3.0` and `LICENSE-GPL-3.0` contain
the complete applicable GNU license texts. No attorney review is claimed.

Every v0.1.0 binary distribution is paired with
`dekopon-python-provider-0.1.0-relink-source.tar.gz`. It contains the exact provider application
source and WIT, `Cargo.lock`, build configuration and scripts, notices and license texts, and the
complete versioned Cargo source for every locked registry dependency. The archive is sufficient to
change Malachite and rebuild/componentize the provider without fetching a crate.

## Obtain and verify v0.1.0 source

The same bytes are available without authentication from both durable locations:

- the `v0.1.0` GitHub Release at
  <https://github.com/dekopon-agents/dekopon-provider-python/releases/tag/v0.1.0>;
- `ghcr.io/dekopon-agents/provider-python-source:0.1.0`, for example with
  `oras pull ghcr.io/dekopon-agents/provider-python-source:0.1.0`.

Do not use a `latest` tag; none is published. Verify and unpack:

```console
sha256sum --check --strict \
  dekopon-python-provider-0.1.0-relink-source.tar.gz.sha256
tar -xzf dekopon-python-provider-0.1.0-relink-source.tar.gz
cd dekopon-python-provider-0.1.0
python3 scripts/source-file-manifest.py verify .
python3 scripts/verify-vendored-source.py .
```

`SOURCE_MANIFEST.json` identifies the exact Git revision, tools, and LGPL package checksums.
`SOURCE_FILE_SHA256SUMS` covers every file in the pristine unpacked tree. The separately published
CycloneDX 1.5 SBOM is byte-identical to the copy inside the archive.

## Pinned prerequisites

Install Rust 1.97.0 with `wasm32-unknown-unknown` and `wasm-tools` 1.236.1 before going offline:

```console
rustup toolchain install 1.97.0 --profile minimal
rustup target add wasm32-unknown-unknown --toolchain 1.97.0
cargo +1.97.0 install wasm-tools --version 1.236.1 --locked
```

The checked-in toolchain file and build script reject other Rust or wasm-tools versions. The
archive's `.cargo/config.toml` replaces crates.io with `vendor/` and sets Cargo offline. It neither
sets `CARGO_TARGET_DIR` nor replaces a user's configured compiler cache.

## Modify Malachite and relink

A recipient may modify the LGPL-covered source. Cargo vendor records per-file hashes, so refresh
the hash for each intentionally changed file. This harmless example exercises the exact relinking
path used by CI:

```console
printf '\n// my harmless Malachite modification\n' \
  >> vendor/malachite-base-0.9.2/src/lib.rs
python3 scripts/refresh-vendored-checksum.py \
  vendor/malachite-base-0.9.2 src/lib.rs

./scripts/build-component.sh
wasm-tools validate python-provider.wasm
./scripts/assert-zero-core-imports.sh python-provider.wasm
sha256sum --check --strict python-provider.wasm.sha256
```

The same process applies to `malachite-bigint-0.9.2`, `malachite-nz-0.9.2`, and
`malachite-q-0.9.2`. You may make substantive changes, replace files throughout those package
directories, and refresh each affected checksum. For a structurally different fork, place its
source in the tree, point a `[patch.crates-io]` entry at that local path, and update `Cargo.lock`
with `cargo +1.97.0 update --offline`; all build inputs must remain local.

`build-component.sh` compiles the modified graph at a scrubbed canonical path, invokes
`wasm-tools component new`, validates the result, and proves that the resulting component and its
nested core modules have no imports. A changed component normally has a different digest and is
not the official release artifact.

## Install a modified component

No signing key, installation key, or proprietary linker is required. Point a Dekopon 0.11.1 host
at the rebuilt file and authorize its new digest under your own deployment policy:

```console
dekopon-run inspect \
  --provider ./python-provider.wasm \
  --fuel 500000000 --timeout-ms 5000 \
  --max-memory-bytes 67108864 \
  --max-input-bytes 1048576 --max-output-bytes 1048576
```

The resource and security profile in `README.md` and `SECURITY.md` remains necessary. Do not label a
modified build as the official Dekopon v0.1.0 binary or expect its checksum to match the release.

## Permission and retention

Provider files authored by this project may be copied and modified under either `LICENSE-MIT` or
`LICENSE-APACHE`. Malachite modifications and redistribution remain governed by
`LICENSE-LGPL-3.0` together with `LICENSE-GPL-3.0`; vendored packages retain their own notices and
license files. The release archive and source OCI artifact are corresponding-source distribution
materials and should be retained with any copy of the official Wasm.

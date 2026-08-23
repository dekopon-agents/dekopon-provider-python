# Third-party notices

`dekopon-python-provider` source written in this repository is licensed under **MIT OR
Apache-2.0**. The component also statically contains third-party code under other terms. This file
is an inventory, not legal advice and not a substitute for the referenced license texts.

## Publication hold: Malachite

The exact RustPython 0.5.0 graph necessarily contains these exact packages:

- `malachite-base 0.9.2`
- `malachite-bigint 0.9.2`
- `malachite-nz 0.9.2`
- `malachite-q 0.9.2`

They are **LGPL-3.0-only** and are statically linked into the WebAssembly component. The project's
permissive-only release policy has not approved a corresponding-source/relinkability distribution
design. Consequently v0.1.0 publication is held and the release workflow is disabled unless the
repository owner explicitly records approval. The narrow `cargo-deny` exceptions inventory these
packages; they are not a claim that distributing the current two technical assets is compliant.
Upstream source is <https://github.com/mhogrefe/malachite> and the crates.io source archives are
identified exactly by `Cargo.lock`.

`r-efi 5.3.0` and `r-efi 6.0.0` are `LGPL-2.1-or-later` target-support dependencies selected in the
cross-platform lock/test graph. They do not introduce a component import. They remain explicitly
inventoried rather than hidden by a blanket LGPL allowance.

## RustPython and Python library material

- `rustpython-vm 0.5.0` and `rustpython-stdlib 0.5.0`, together with the synchronized
  `rustpython-* 0.5.0` implementation crates, are from
  <https://github.com/RustPython/RustPython> and use RustPython's MIT terms.
- `rustpython-pylib 0.5.0` embeds Python library material under the Python Software Foundation
  License / `Python-2.0.1`. Its authoritative text is `Lib/PSF-LICENSE` in that exact crate source.
- `rustpython-doc 0.5.0` carries `Python-2.0.1`; `deny.toml` pins the detected license-file hash.
- `pymath` is `PSF-2.0`.

RustPython includes code and data derived from CPython, Unicode, and other projects. Their notices
and license files in the exact crates.io source archives remain applicable. The lockfile pins all
synchronized RustPython codegen/common/compiler/compiler-core/derive/doc/literal/pylib/
sre-engine/stdlib/vm/wtf8 crates to 0.5.0.

## YAML

`yaml-rust2 0.12.0` is Copyright Yuheng Chen, Ethiraric, David Aguilar, and contributors, licensed
under **MIT OR Apache-2.0**. The provider uses it with default features disabled. Upstream:
<https://github.com/Ethiraric/yaml-rust2>.

## Unicode and BSD-style material

The graph contains Unicode data and implementations under `Unicode-3.0`, `Unicode-DFS-2016`,
BSD-2-Clause, BSD-2-Clause-Views, BSD-3-Clause, ISC, and PSF terms. In particular the exact
`unic-* 0.9.0` family (including `unic-char-property`) and `unicode_names2` data are retained. The complete machine-checked license
inventory is produced by:

```console
cargo deny list
cargo deny check licenses
```

The `unic-*` family is unmaintained (the exact advisories are listed in `deny.toml`) but unavoidable
in RustPython 0.5.0. Those targeted acknowledgements are reviewed separately from license policy.

## Known unmaintained transitive code

`paste 1.0.15` is transitively required by the exact RustPython/Malachite graph and is covered by
`RUSTSEC-2024-0436`. The `unic-*` advisories are individually named in `deny.toml`. There is no
blanket advisory ignore; upgrading or replacing these dependencies requires moving away from the
exact tested RustPython 0.5.0 graph.

## Reproducing the inventory

Run `./scripts/assert-lock-and-feature-graph.sh Cargo.lock` and
`./scripts/check-third-party-notices.sh Cargo.lock THIRD_PARTY_NOTICES.md`. `Cargo.lock` is the
authority for exact package versions and checksums. No generated `.wasm` is source-controlled.

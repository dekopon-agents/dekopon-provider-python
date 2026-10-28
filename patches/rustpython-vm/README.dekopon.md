# Dekopon reproducibility and sandbox patch

This directory is the complete `rustpython-vm 0.5.0` crates.io source
(original archive SHA-256 `1880770161cef896bf9c7e065dae574e3b4c3cf6172e485f1ce86f0483816ab2`),
with one provider-local reproducibility fix in `build.rs` and one sandbox fix in `src/stdlib/_abc.rs`, under RustPython's MIT license.

The upstream build script writes every environment variable of the build process into
`_sysconfigdata` (`sysvars! { ... }`) and stamps the crate with `git describe` of whatever
repository contains it. Both put machine-local paths and checkout state into the shipped
component, so two builds of one commit differ and the component embeds the build directory.
The patched script writes an empty table and constant git stamps.

One sandbox fix is also applied, in `src/stdlib/_abc.rs`: step 6 of `_abc_subclasscheck` reads
the ABC's internal `subclasses` list instead of calling `cls.__subclasses__()` by attribute. The
provider removes the Python-visible `type.__subclasses__` after trusted setup, and the attribute
call then failed every ABC check that missed the caches (`random.sample`, `collections.Counter`,
`statistics.mode`). The list never reaches Python. Nothing else changed.

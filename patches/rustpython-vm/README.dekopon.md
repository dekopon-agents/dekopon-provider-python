# Dekopon reproducibility patch

This directory is the complete `rustpython-vm 0.5.0` crates.io source
(original archive SHA-256 `1880770161cef896bf9c7e065dae574e3b4c3cf6172e485f1ce86f0483816ab2`),
with one provider-local reproducibility fix in `build.rs` under RustPython's MIT license.

The upstream build script writes every environment variable of the build process into
`_sysconfigdata` (`sysvars! { ... }`) and stamps the crate with `git describe` of whatever
repository contains it. Both put machine-local paths and checkout state into the shipped
component, so two builds of one commit differ and the component embeds the build directory.
The patched script writes an empty table and constant git stamps. Nothing else changed.

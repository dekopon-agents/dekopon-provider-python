/// Deterministic, non-cryptographic getrandom backend for RustPython internals on the import-free
/// Wasm target. No Python entropy API is exposed. The VM hash seed is configured separately and
/// explicitly; this symbol exists only because transitive internals require the upstream ABI.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
#[unsafe(no_mangle)]
unsafe extern "Rust" fn __getrandom_v03_custom(
    destination: *mut u8,
    length: usize,
) -> Result<(), getrandom::Error> {
    if destination.is_null() && length != 0 {
        return Err(getrandom::Error::UNEXPECTED);
    }
    for index in 0..length {
        // SAFETY: getrandom's custom-backend contract provides a writable region of `length`
        // bytes. The null/non-zero case was rejected above and each offset is in that region.
        unsafe {
            destination
                .add(index)
                .write((index as u8).wrapping_mul(31).wrapping_add(0x5a));
        }
    }
    Ok(())
}

/// getrandom 0.3's custom backend is unique to the embedding guest. VM hash seeding remains
/// explicitly fixed and independent; SQL UUIDs and other native entropy use this host source.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
#[unsafe(no_mangle)]
unsafe extern "Rust" fn __getrandom_v03_custom(
    destination: *mut u8,
    length: usize,
) -> Result<(), getrandom::Error> {
    if destination.is_null() && length != 0 {
        return Err(getrandom::Error::UNEXPECTED);
    }
    let mut offset = 0;
    while offset < length {
        let request = (length - offset).min(4096);
        let bytes = crate::bindings::dekopon::random::source::get_random_bytes(request as u32);
        assert_eq!(bytes.len(), request, "host random response length mismatch");
        // SAFETY: getrandom supplies a writable `length`-byte region; offset + request <= length.
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), destination.add(offset), request) };
        offset += request;
    }
    Ok(())
}

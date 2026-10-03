use dekopon_provider_sdk::provider::Random;
use std::cell::RefCell;

thread_local! {
    static CURRENT: RefCell<Option<Random>> = const { RefCell::new(None) };
}

pub(crate) struct EntropyScope(Option<Random>);

impl EntropyScope {
    pub(crate) fn install(random: Random) -> Self {
        Self(CURRENT.with(|current| current.replace(Some(random))))
    }
}

impl Drop for EntropyScope {
    fn drop(&mut self) {
        CURRENT.with(|current| {
            current.replace(self.0.take());
        });
    }
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
#[unsafe(no_mangle)]
unsafe extern "Rust" fn __getrandom_v03_custom(
    destination: *mut u8,
    length: usize,
) -> Result<(), getrandom::Error> {
    if destination.is_null() && length != 0 {
        return Err(getrandom::Error::UNEXPECTED);
    }
    CURRENT.with(|current| {
        let borrowed = current.borrow();
        let random = borrowed.as_ref().ok_or(getrandom::Error::UNEXPECTED)?;
        if length == 0 {
            return Ok(());
        }
        let mut bytes = vec![0; length];
        random.fill(&mut bytes);
        // SAFETY: getrandom supplies a writable region of `length` bytes; the initialized
        // temporary owns precisely that many bytes and a null pointer is used only for zero.
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), destination, length) };
        Ok(())
    })
}

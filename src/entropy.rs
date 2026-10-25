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

pub(crate) fn active() -> bool {
    CURRENT.with(|current| current.borrow().is_some())
}

pub(crate) fn draw(destination: &mut [u8]) -> Result<(), ()> {
    CURRENT.with(|current| {
        let borrowed = current.borrow();
        let random = borrowed.as_ref().ok_or(())?;
        #[cfg(test)]
        READS.with(|reads| reads.set(reads.get() + 1));
        random.fill(destination);
        Ok(())
    })
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn fill_random02(destination: &mut [u8]) -> Result<(), getrandom02::Error> {
    draw(destination).map_err(|()| getrandom02::Error::UNSUPPORTED)
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
getrandom02::register_custom_getrandom!(fill_random02);

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
#[unsafe(no_mangle)]
unsafe extern "Rust" fn __getrandom_v03_custom(
    destination: *mut u8,
    length: usize,
) -> Result<(), getrandom::Error> {
    if destination.is_null() && length != 0 {
        return Err(getrandom::Error::UNEXPECTED);
    }
    let mut bytes = vec![0; length];
    draw(&mut bytes).map_err(|()| getrandom::Error::UNEXPECTED)?;
    if length != 0 {
        // SAFETY: getrandom supplies a writable region of `length` bytes; the initialized
        // temporary owns precisely that many bytes and a null pointer is used only for zero.
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), destination, length) };
    }
    Ok(())
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
#[unsafe(no_mangle)]
unsafe extern "Rust" fn __getrandom_v04_custom(
    destination: *mut u8,
    length: usize,
) -> Result<(), getrandom04::Error> {
    if destination.is_null() && length != 0 {
        return Err(getrandom04::Error::UNEXPECTED);
    }
    let mut bytes = vec![0; length];
    draw(&mut bytes).map_err(|()| getrandom04::Error::UNEXPECTED)?;
    if length != 0 {
        // SAFETY: getrandom supplies a writable region of length bytes.
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), destination, length) };
    }
    Ok(())
}

#[cfg(test)]
thread_local! {
    pub(crate) static READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[rustpython_vm::pymodule(name = "_dekopon_entropy")]
pub(crate) mod entropy_module {
    use rustpython_vm::{PyResult, VirtualMachine, builtins::PyBytesRef};

    #[pyfunction]
    fn draw(length: isize, vm: &VirtualMachine) -> PyResult<PyBytesRef> {
        let length =
            usize::try_from(length).map_err(|_| vm.new_value_error("negative entropy length"))?;
        let mut bytes = vec![0; length];
        super::draw(&mut bytes)
            .map_err(|()| vm.new_runtime_error("entropy unavailable outside invocation"))?;
        Ok(vm.ctx.new_bytes(bytes))
    }
}

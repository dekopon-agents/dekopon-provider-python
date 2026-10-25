//! Broker-only primitive PRNG, adapted from rustpython-stdlib 0.5.0 src/random.rs.
//! Keep the MT19937 algorithm/state format; replace OS seeding with lazy broker entropy.

pub(crate) use _random::module_def;

#[rustpython_vm::pymodule]
mod _random {
    use itertools::Itertools;
    use malachite_bigint::{BigInt, BigUint, Sign};
    use mt19937::MT19937;
    use num_traits::{Signed, Zero};
    use rand_core::RngCore;
    use rustpython_common::lock::PyMutex;
    use rustpython_vm::types::DefaultConstructor;
    use rustpython_vm::{
        PyObjectRef, PyPayload, PyRef, PyResult, VirtualMachine,
        builtins::{PyInt, PyTupleRef},
        function::OptionalOption,
        pyclass,
        types::{Constructor, Initializer},
    };

    #[pyattr]
    #[pyclass(name = "Random")]
    #[derive(Debug, PyPayload, Default)]
    struct PyRandom {
        rng: PyMutex<Option<MT19937>>,
    }

    impl DefaultConstructor for PyRandom {}

    impl Initializer for PyRandom {
        type Args = OptionalOption;

        fn init(zelf: PyRef<Self>, x: Self::Args, vm: &VirtualMachine) -> PyResult<()> {
            zelf.seed(x, vm)
        }
    }

    #[pyclass(flags(BASETYPE), with(Constructor, Initializer))]
    impl PyRandom {
        fn with_rng<R>(
            &self,
            vm: &VirtualMachine,
            f: impl FnOnce(&mut MT19937) -> R,
        ) -> PyResult<R> {
            if !crate::entropy::active() {
                return Err(vm.new_runtime_error("entropy unavailable outside invocation"));
            }
            let mut rng = self.rng.lock();
            if rng.is_none() {
                let mut bytes = [0; 32];
                crate::entropy::draw(&mut bytes)
                    .map_err(|()| vm.new_runtime_error("entropy unavailable outside invocation"))?;
                let key: Vec<_> = bytes
                    .chunks_exact(4)
                    .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
                    .collect();
                *rng = Some(MT19937::new_with_slice_seed(&key));
            }
            Ok(f(rng.as_mut().unwrap()))
        }

        #[pymethod]
        fn random(&self, vm: &VirtualMachine) -> PyResult<f64> {
            self.with_rng(vm, mt19937::gen_res53)
        }

        #[pymethod]
        fn seed(&self, n: OptionalOption<PyObjectRef>, vm: &VirtualMachine) -> PyResult<()> {
            *self.rng.lock() = match n.flatten() {
                Some(n) => {
                    // Fallback to using hash if object isn't Int-like.
                    let (_, mut key) = match n.downcast::<PyInt>() {
                        Ok(n) => n.as_bigint().abs(),
                        Err(obj) => BigInt::from(obj.hash(vm)?).abs(),
                    }
                    .to_u32_digits();
                    if cfg!(target_endian = "big") {
                        key.reverse();
                    }
                    let key = if key.is_empty() { &[0] } else { key.as_slice() };
                    Some(MT19937::new_with_slice_seed(key))
                }
                None => None,
            };
            Ok(())
        }

        #[pymethod]
        fn getrandbits(&self, k: PyObjectRef, vm: &VirtualMachine) -> PyResult<BigInt> {
            let k_int = k.try_index(vm)?;
            let k_bigint = k_int.as_bigint();
            if k_bigint.is_negative() {
                return Err(vm.new_value_error("number of bits must be non-negative"));
            }
            let k: isize = k_int
                .try_to_primitive(vm)
                .map_err(|_| vm.new_overflow_error("getrandbits: number of bits too large"))?;
            match k {
                0 => Ok(BigInt::zero()),
                mut k => {
                    self.with_rng(vm, |rng| {
                        let mut gen_u32 = |k| {
                            let r = rng.next_u32();
                            if k < 32 { r >> (32 - k) } else { r }
                        };

                        let words = (k - 1) / 32 + 1;
                        let word_array = (0..words)
                            .map(|_| {
                                let word = gen_u32(k);
                                k = k.wrapping_sub(32);
                                word
                            })
                            .collect::<Vec<_>>();

                        let uint = BigUint::new(word_array);
                        // very unlikely but might as well check
                        let sign = if uint.is_zero() {
                            Sign::NoSign
                        } else {
                            Sign::Plus
                        };
                        BigInt::from_biguint(sign, uint)
                    })
                }
            }
        }

        #[pymethod]
        fn getstate(&self, vm: &VirtualMachine) -> PyResult<PyTupleRef> {
            self.with_rng(vm, |rng| {
                vm.new_tuple(
                    rng.get_state()
                        .iter()
                        .copied()
                        .chain([rng.get_index() as u32])
                        .map(|i| vm.ctx.new_int(i).into())
                        .collect::<Vec<PyObjectRef>>(),
                )
            })
        }

        #[pymethod]
        fn setstate(&self, state: PyTupleRef, vm: &VirtualMachine) -> PyResult<()> {
            let state: &[_; mt19937::N + 1] = state
                .as_slice()
                .try_into()
                .map_err(|_| vm.new_value_error("state vector is the wrong size"))?;
            let (index, state) = state.split_last().unwrap();
            let index: usize = index.try_to_value(vm)?;
            if index > mt19937::N {
                return Err(vm.new_value_error("invalid state"));
            }
            let state: [u32; mt19937::N] = state
                .iter()
                .map(|i| i.try_to_value(vm))
                .process_results(|it| it.collect_array())?
                .unwrap();
            let mut rng = MT19937::new_with_slice_seed(&[0]);
            rng.set_state(&state);
            rng.set_index(index);
            *self.rng.lock() = Some(rng);
            Ok(())
        }
    }
}

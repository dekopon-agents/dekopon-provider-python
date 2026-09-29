//! Small, bounded numeric Python facade. This is not NumPy or scikit-learn compatibility.
//!
//! `vector_add(a, b)`, `dot(a, b)`, `matrix_multiply(a, b)`,
//! `column_means(matrix)`, `normal_cdf(x, mean, std_dev)`,
//! `linear_regression(features, targets)` (in-sample predictions), and
//! `kmeans(features, clusters, seed)` (in-sample integer labels).
//! Inputs are exact Python lists/tuples of exact ints/floats, not arbitrary iterables.
//! Values must be finite and have magnitude <= 1e6. All outputs are JSON-safe
//! lists/numbers. A seed is mandatory; clustering never requests ambient entropy.

use ndarray::Array2;
use num_traits::ToPrimitive;
use rustpython_vm::{
    AsObject, PyObjectRef, PyResult, VirtualMachine,
    builtins::{PyFloat, PyInt, PyList, PyTuple},
};
use smartcore::{
    cluster::kmeans::{KMeans, KMeansParameters},
    linalg::basic::matrix::DenseMatrix,
    linear::linear_regression::{LinearRegression, LinearRegressionParameters},
};
use statrs::distribution::{ContinuousCDF, Normal};

const MAX_VECTOR: usize = 4096;
const MAX_MATRIX_ROWS: usize = 64;
const MAX_MATRIX_COLUMNS: usize = 64;
const MAX_MATRIX_ELEMENTS: usize = 4096;
const MAX_MATRIX_WORK: usize = 262_144;
const MAX_ML_ROWS: usize = 128;
const MAX_ML_FEATURES: usize = 8;
const MAX_KMEANS_CLUSTERS: usize = 8;
const MAX_KMEANS_ITERATIONS: usize = 25;
const MAX_MAGNITUDE: f64 = 1_000_000.0;

#[rustpython_vm::pymodule(name = "dekopon_numeric")]
pub(crate) mod numeric_module {
    use super::*;

    #[pyfunction]
    fn vector_add(a: PyObjectRef, b: PyObjectRef, vm: &VirtualMachine) -> PyResult<PyObjectRef> {
        let a = vector(&a, MAX_VECTOR, vm)?;
        let b = vector(&b, MAX_VECTOR, vm)?;
        if a.len() != b.len() {
            return Err(vm.new_value_error("vectors must have equal lengths"));
        }
        Ok(float_list(
            a.iter()
                .zip(&b)
                .map(|(x, y)| checked(x + y, vm))
                .collect::<PyResult<Vec<_>>>()?,
            vm,
        ))
    }

    #[pyfunction]
    fn dot(a: PyObjectRef, b: PyObjectRef, vm: &VirtualMachine) -> PyResult<f64> {
        let a = vector(&a, MAX_VECTOR, vm)?;
        let b = vector(&b, MAX_VECTOR, vm)?;
        if a.len() != b.len() {
            return Err(vm.new_value_error("vectors must have equal lengths"));
        }
        checked(a.iter().zip(&b).map(|(x, y)| x * y).sum(), vm)
    }

    #[pyfunction]
    fn matrix_multiply(
        a: PyObjectRef,
        b: PyObjectRef,
        vm: &VirtualMachine,
    ) -> PyResult<PyObjectRef> {
        let a = matrix(
            &a,
            MAX_MATRIX_ROWS,
            MAX_MATRIX_COLUMNS,
            MAX_MATRIX_ELEMENTS,
            vm,
        )?;
        let b = matrix(
            &b,
            MAX_MATRIX_ROWS,
            MAX_MATRIX_COLUMNS,
            MAX_MATRIX_ELEMENTS,
            vm,
        )?;
        if a[0].len() != b.len() {
            return Err(vm.new_value_error("matrix inner dimensions must agree"));
        }
        if a.len() * b.len() * b[0].len() > MAX_MATRIX_WORK {
            return Err(vm.new_value_error("matrix multiplication work limit exceeded"));
        }
        let left = Array2::from_shape_vec((a.len(), a[0].len()), a.into_iter().flatten().collect())
            .map_err(|_| vm.new_value_error("invalid left matrix shape"))?;
        let right =
            Array2::from_shape_vec((b.len(), b[0].len()), b.into_iter().flatten().collect())
                .map_err(|_| vm.new_value_error("invalid right matrix shape"))?;
        let result = left.dot(&right);
        let rows: Vec<Vec<f64>> = result
            .outer_iter()
            .map(|row| row.iter().map(|x| checked(*x, vm)).collect())
            .collect::<PyResult<_>>()?;
        Ok(vm
            .ctx
            .new_list(rows.into_iter().map(|row| float_list(row, vm)).collect())
            .into())
    }

    #[pyfunction]
    fn column_means(a: PyObjectRef, vm: &VirtualMachine) -> PyResult<PyObjectRef> {
        let a = matrix(
            &a,
            MAX_MATRIX_ROWS,
            MAX_MATRIX_COLUMNS,
            MAX_MATRIX_ELEMENTS,
            vm,
        )?;
        Ok(float_list(
            (0..a[0].len())
                .map(|column| {
                    checked(
                        a.iter().map(|row| row[column]).sum::<f64>() / a.len() as f64,
                        vm,
                    )
                })
                .collect::<PyResult<Vec<_>>>()?,
            vm,
        ))
    }

    #[pyfunction]
    fn normal_cdf(x: f64, mean: f64, std_dev: f64, vm: &VirtualMachine) -> PyResult<f64> {
        for number in [x, mean, std_dev] {
            valid_number(number, vm)?;
        }
        if std_dev <= 0.0 {
            return Err(vm.new_value_error("standard deviation must be positive"));
        }
        let distribution = Normal::new(mean, std_dev)
            .map_err(|_| vm.new_value_error("invalid normal distribution"))?;
        checked(distribution.cdf(x), vm)
    }

    #[pyfunction]
    fn linear_regression(
        features: PyObjectRef,
        targets: PyObjectRef,
        vm: &VirtualMachine,
    ) -> PyResult<PyObjectRef> {
        let features = ml_matrix(&features, vm)?;
        let targets = vector(&targets, MAX_ML_ROWS, vm)?;
        if targets.len() != features.len() || features.len() <= features[0].len() {
            return Err(vm.new_value_error(
                "regression requires matching targets and more rows than features",
            ));
        }
        Ok(float_list(
            regression(&features, &targets)
                .map_err(|e| vm.new_value_error(e))?
                .into_iter()
                .map(|v| checked(v, vm))
                .collect::<PyResult<Vec<_>>>()?,
            vm,
        ))
    }

    #[pyfunction]
    fn kmeans(
        features: PyObjectRef,
        clusters: usize,
        seed: u64,
        vm: &VirtualMachine,
    ) -> PyResult<PyObjectRef> {
        let features = ml_matrix(&features, vm)?;
        if !(2..=MAX_KMEANS_CLUSTERS).contains(&clusters) || clusters > features.len() {
            return Err(
                vm.new_value_error("clusters must be between 2 and 8 and no more than rows")
            );
        }
        let mut distinct = features.clone();
        distinct.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        distinct.dedup();
        if distinct.len() < clusters {
            return Err(vm.new_value_error("clusters exceed the number of distinct rows"));
        }
        let labels = seeded_kmeans(&features, clusters, seed).map_err(|e| vm.new_value_error(e))?;
        Ok(vm
            .ctx
            .new_list(
                labels
                    .into_iter()
                    .map(|label| vm.ctx.new_int(label).into())
                    .collect(),
            )
            .into())
    }
}

fn float_list(numbers: Vec<f64>, vm: &VirtualMachine) -> PyObjectRef {
    vm.ctx
        .new_list(
            numbers
                .into_iter()
                .map(|n| vm.ctx.new_float(n).into())
                .collect(),
        )
        .into()
}

fn items<T>(
    object: &PyObjectRef,
    vm: &VirtualMachine,
    f: impl FnOnce(&[PyObjectRef]) -> PyResult<T>,
) -> PyResult<T> {
    if object.class().is(vm.ctx.types.list_type) {
        let list = object
            .downcast_ref::<PyList>()
            .ok_or_else(|| vm.new_type_error("invalid list"))?;
        f(&list.borrow_vec())
    } else if object.class().is(vm.ctx.types.tuple_type) {
        let tuple = object
            .downcast_ref::<PyTuple>()
            .ok_or_else(|| vm.new_type_error("invalid tuple"))?;
        f(tuple.as_slice())
    } else {
        Err(vm.new_type_error("expected an exact list or tuple"))
    }
}

fn valid_number(number: f64, vm: &VirtualMachine) -> PyResult<f64> {
    if !number.is_finite() || number.abs() > MAX_MAGNITUDE {
        Err(vm.new_value_error("numbers must be finite and within +/-1000000"))
    } else {
        Ok(number)
    }
}

fn checked(number: f64, vm: &VirtualMachine) -> PyResult<f64> {
    if number.is_finite() {
        Ok(number)
    } else {
        Err(vm.new_value_error("non-finite numeric result"))
    }
}

fn number(object: &PyObjectRef, vm: &VirtualMachine) -> PyResult<f64> {
    let n = if object.class().is(vm.ctx.types.float_type) {
        object.downcast_ref::<PyFloat>().map(|n| n.to_f64())
    } else if object.class().is(vm.ctx.types.int_type) {
        object
            .downcast_ref::<PyInt>()
            .and_then(|n| n.as_bigint().to_f64())
    } else {
        None
    }
    .ok_or_else(|| vm.new_type_error("expected an exact int or float"))?;
    valid_number(n, vm)
}

fn vector(object: &PyObjectRef, max: usize, vm: &VirtualMachine) -> PyResult<Vec<f64>> {
    items(object, vm, |slice| {
        if slice.is_empty() || slice.len() > max {
            return Err(vm.new_value_error(format!("vector length must be between 1 and {max}")));
        }
        slice.iter().map(|item| number(item, vm)).collect()
    })
}

fn matrix(
    object: &PyObjectRef,
    max_rows: usize,
    max_cols: usize,
    max_elements: usize,
    vm: &VirtualMachine,
) -> PyResult<Vec<Vec<f64>>> {
    items(object, vm, |rows| {
        if rows.is_empty() || rows.len() > max_rows {
            return Err(vm.new_value_error(format!("matrix rows must be between 1 and {max_rows}")));
        }
        let mut result = Vec::with_capacity(rows.len());
        let mut width = None;
        for row in rows {
            let values = vector(row, max_cols, vm)?;
            if let Some(expected) = width {
                if values.len() != expected {
                    return Err(vm.new_value_error("matrix rows must have equal lengths"));
                }
            } else {
                if rows.len() * values.len() > max_elements {
                    return Err(vm.new_value_error("matrix element limit exceeded"));
                }
                width = Some(values.len());
            }
            result.push(values);
        }
        Ok(result)
    })
}

fn ml_matrix(object: &PyObjectRef, vm: &VirtualMachine) -> PyResult<Vec<Vec<f64>>> {
    matrix(
        object,
        MAX_ML_ROWS,
        MAX_ML_FEATURES,
        MAX_ML_ROWS * MAX_ML_FEATURES,
        vm,
    )
}

fn regression(features: &Vec<Vec<f64>>, targets: &Vec<f64>) -> Result<Vec<f64>, String> {
    let x = DenseMatrix::from_2d_vec(features).map_err(|e| e.to_string())?;
    let model: LinearRegression<f64, f64, DenseMatrix<f64>, Vec<f64>> =
        LinearRegression::fit(&x, targets, LinearRegressionParameters::default())
            .map_err(|e| e.to_string())?;
    model.predict(&x).map_err(|e| e.to_string())
}

fn seeded_kmeans(
    features: &Vec<Vec<f64>>,
    clusters: usize,
    seed: u64,
) -> Result<Vec<usize>, String> {
    let x = DenseMatrix::from_2d_vec(features).map_err(|e| e.to_string())?;
    let params = KMeansParameters {
        seed: Some(seed),
        ..Default::default()
    }
    .with_k(clusters)
    .with_max_iter(MAX_KMEANS_ITERATIONS);
    let model: KMeans<f64, usize, _, Vec<usize>> =
        KMeans::fit(&x, params).map_err(|e| e.to_string())?;
    model.predict(&x).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ndarray_matrix_and_statrs_distribution() {
        let matrix = Array2::from_shape_vec((2, 2), vec![1., 2., 3., 4.]).unwrap();
        assert_eq!(matrix.dot(&matrix)[[0, 0]], 7.);
        let normal = Normal::new(0., 1.).unwrap();
        assert!((normal.cdf(0.) - 0.5).abs() < 1e-12);
    }

    #[test]
    fn regression_predicts_training_targets() {
        let features = vec![vec![1., 1.], vec![2., 1.], vec![3., 1.], vec![4., 1.]];
        let predictions = regression(&features, &vec![3., 5., 7., 9.]).unwrap();
        assert!((predictions[0] - 3.).abs() < 1e-6);
    }

    #[test]
    fn clustering_is_repeatable_for_explicit_seed() {
        let features = vec![vec![0., 0.], vec![0., 1.], vec![10., 10.], vec![10., 11.]];
        let labels = seeded_kmeans(&features, 2, 42).unwrap();
        assert_eq!(labels, seeded_kmeans(&features, 2, 42).unwrap());
        assert_eq!(labels[0], labels[1]);
        assert_eq!(labels[2], labels[3]);
        assert_ne!(labels[0], labels[2]);
    }
}

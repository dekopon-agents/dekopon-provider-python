//! Native buffered HTTP facade. Only the broker import supplies transport or authority.
use rustpython_vm::{
    PyPayload, PyResult, VirtualMachine,
    builtins::{PyBaseExceptionRef, PyTypeRef, PyUtf8StrRef},
};

const URL_BYTES: usize = 8_192;
const BODY_BYTES: usize = 131_072;

#[rustpython_vm::pymodule(name = "dekopon_requests")]
pub(crate) mod requests_module {
    use super::*;
    use rustpython_vm::{builtins::PyBytesRef, pyclass};

    #[pyattr(name = "RequestException", once)]
    fn error(vm: &VirtualMachine) -> PyTypeRef {
        crate::exception::immutable_exception_type(
            vm,
            "dekopon_requests",
            "RequestException",
            vm.ctx.exceptions.exception_type.to_owned(),
        )
    }

    #[pyattr(name = "HTTPError", once)]
    fn http_error(vm: &VirtualMachine) -> PyTypeRef {
        crate::exception::immutable_exception_type(vm, "dekopon_requests", "HTTPError", error(vm))
    }

    #[pyattr(name = "JSONDecodeError", once)]
    fn json_error(vm: &VirtualMachine) -> PyTypeRef {
        crate::exception::immutable_exception_type(
            vm,
            "dekopon_requests",
            "JSONDecodeError",
            error(vm),
        )
    }

    #[pyattr]
    #[pyclass(module = "dekopon_requests", name = "Response")]
    #[derive(Debug, PyPayload)]
    pub(crate) struct Response {
        status: u16,
        body: Vec<u8>,
    }

    #[pyclass]
    impl Response {
        #[pygetset]
        const fn status_code(&self) -> u16 {
            self.status
        }

        #[pygetset]
        const fn ok(&self) -> bool {
            self.status < 400
        }

        #[pygetset]
        fn content(&self, vm: &VirtualMachine) -> PyBytesRef {
            vm.ctx.new_bytes(self.body.clone())
        }

        #[pygetset]
        fn text(&self) -> String {
            String::from_utf8_lossy(&self.body).into_owned()
        }

        #[pymethod]
        fn json(&self, vm: &VirtualMachine) -> PyResult {
            // serde_json's default recursion ceiling bounds parsing before the provider's tighter
            // safe-value walk. No Python decoder hooks or custom coercions are invoked.
            let value: serde_json::Value = serde_json::from_slice(&self.body)
                .map_err(|_| exception(vm, json_error(vm), "invalid JSON response"))?;
            validate_json_numbers(&value).map_err(|()| {
                exception(vm, json_error(vm), "JSON exceeds the safe-value limits")
            })?;
            let value = crate::value::safe_json_to_py(&value, vm);
            crate::value::py_to_safe_json(&value, vm)
                .map_err(|_| exception(vm, json_error(vm), "JSON exceeds the safe-value limits"))?;
            Ok(value)
        }

        #[pymethod]
        fn raise_for_status(&self, vm: &VirtualMachine) -> PyResult<()> {
            if self.status >= 400 {
                return Err(exception(
                    vm,
                    http_error(vm),
                    &format!("HTTP status {}", self.status),
                ));
            }
            Ok(())
        }
    }

    #[pyfunction]
    fn get(url: PyUtf8StrRef, vm: &VirtualMachine) -> PyResult<Response> {
        request("GET", url.as_str(), vm)
    }

    #[pyfunction]
    fn head(url: PyUtf8StrRef, vm: &VirtualMachine) -> PyResult<Response> {
        request("HEAD", url.as_str(), vm)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn response_status_bytes_utf8_and_safe_json_are_bounded() {
            std::thread::Builder::new()
                .stack_size(32 * 1024 * 1024)
                .spawn(|| {
                    crate::eval::interpreter().enter(|vm| {
                        let response = Response {
                            status: 200,
                            body: br#"{"answer":42}"#.to_vec(),
                        };
                        assert_eq!(response.status_code(), 200);
                        assert!(response.ok());
                        assert!(response.raise_for_status(vm).is_ok());
                        let value = response.json(vm).expect("JSON");
                        assert_eq!(
                            crate::value::py_to_safe_json(&value, vm).unwrap(),
                            serde_json::json!({"answer":42})
                        );
                        assert_eq!(response.content(vm).as_bytes(), response.body);
                        for body in [
                            b"{".to_vec(),
                            b"NaN".to_vec(),
                            b"9007199254740992".to_vec(),
                            b"[".repeat(130),
                        ] {
                            assert!(Response { status: 200, body }.json(vm).is_err());
                        }
                        let response = Response {
                            status: 404,
                            body: vec![255],
                        };
                        assert!(!response.ok());
                        assert_eq!(response.text(), "\u{fffd}");
                        assert!(response.raise_for_status(vm).is_err());
                        assert!(request("GET", "", vm).is_err());
                        assert!(request("GET", &"x".repeat(URL_BYTES + 1), vm).is_err());
                    })
                })
                .unwrap()
                .join()
                .unwrap();
        }

        #[test]
        fn native_exception_factories_ignore_guest_attributes_across_interpreters() {
            std::thread::Builder::new()
                .stack_size(32 * 1024 * 1024)
                .spawn(|| {
                    for _ in 0..3 {
                        crate::eval::interpreter().enter(|vm| {
                            let scope = vm.new_scope_with_builtins();
                            vm.run_code_string(
                                scope.clone(),
                                "import dekopon_requests",
                                "<response-type-init>".to_owned(),
                            )
                            .unwrap();
                            scope
                                .globals
                                .set_item(
                                    "response",
                                    vm.new_pyobj(Response {
                                        status: 404,
                                        body: b"invalid".to_vec(),
                                    }),
                                    vm,
                                )
                                .unwrap();
                            vm.run_code_string(
                                scope,
                                r#"
import dekopon_requests as r
base = r.RequestException
for name, call in [('RequestException', lambda: r.get('')),
                   ('HTTPError', response.raise_for_status),
                   ('JSONDecodeError', response.json)]:
    original = getattr(r, name)
    assert issubclass(original, base)
    assert not hasattr(original, 'marker')
    try:
        original.marker = 'leak'
    except TypeError:
        pass
    else:
        raise AssertionError('mutable class')
    class Malicious(original):
        def __new__(cls, *args):
            raise AssertionError('guest constructor')
        def __init__(self, *args):
            raise AssertionError('guest initializer')
    for replacement in [original, int, 42, Malicious, None]:
        if replacement is None:
            delattr(r, name)
        else:
            setattr(r, name, replacement)
        try:
            call()
        except original as error:
            assert type(error) is original
        else:
            raise AssertionError('missing error')
"#,
                                "<exception-regression>".to_owned(),
                            )
                            .unwrap();
                        });
                    }
                })
                .unwrap()
                .join()
                .unwrap();
        }

        #[test]
        fn json_integer_tokens_never_round_into_floats() {
            std::thread::Builder::new()
                .stack_size(32 * 1024 * 1024)
                .spawn(|| {
                    crate::eval::interpreter().enter(|vm| {
                        for token in [
                            "18446744073709551617",
                            "-9223372036854775809",
                            "9007199254740992",
                            "-9007199254740992",
                            "1e400",
                        ] {
                            for body in [token.to_owned(), format!("{{\"nested\":[{token}]}}")] {
                                assert!(
                                    Response {
                                        status: 200,
                                        body: body.into_bytes()
                                    }
                                    .json(vm)
                                    .is_err(),
                                    "{token}"
                                );
                            }
                        }
                        for token in [
                            "9007199254740991",
                            "-9007199254740991",
                            "9007199254740990",
                            "-9007199254740990",
                            "9007199254740992.0",
                            "-9007199254740992.0",
                            "1e30",
                            "1.25",
                        ] {
                            let value = Response {
                                status: 200,
                                body: token.as_bytes().to_vec(),
                            }
                            .json(vm)
                            .expect(token);
                            let value = crate::value::py_to_safe_json(&value, vm).unwrap();
                            if token.contains(['.', 'e']) {
                                assert!(value.as_number().unwrap().is_f64(), "{token}");
                            } else {
                                assert_eq!(value.as_i64().unwrap(), token.parse::<i64>().unwrap());
                            }
                        }
                    });
                })
                .unwrap()
                .join()
                .unwrap();
        }

        #[test]
        fn http_wit_matches_the_published_guest_binding() {
            assert_eq!(
                include_str!("../wit/http/http.wit"),
                dekopon_provider_http::HTTP_WIT
            );
        }
    }

    fn request(method: &str, url: &str, vm: &VirtualMachine) -> PyResult<Response> {
        if url.len() > URL_BYTES {
            return Err(exception(vm, error(vm), "URL exceeds 8192 UTF-8 bytes"));
        }
        let request = dekopon_provider_http::Request::new(method, url)
            .map_err(|_| exception(vm, error(vm), "invalid URL"))?;
        let response = dekopon_provider_http::send(request)
            // Stable code only: never copy remote text, URL, or potentially sensitive host detail.
            .map_err(|failure| exception(vm, error(vm), failure.code.as_str()))?;
        if response.body.len() > BODY_BYTES {
            return Err(exception(vm, error(vm), "response exceeds 131072 bytes"));
        }
        Ok(Response {
            status: response.status,
            body: response.body,
        })
    }
}

fn exception(vm: &VirtualMachine, class: PyTypeRef, message: &str) -> PyBaseExceptionRef {
    vm.new_exception_msg(class, message.to_owned().into())
}

// arbitrary_precision preserves integer tokens that serde_json would otherwise round to f64.
// Validate before safe_json_to_py; decimal/exponent tokens remain legitimate finite floats.
fn validate_json_numbers(value: &serde_json::Value) -> Result<(), ()> {
    use serde_json::Value;
    match value {
        Value::Number(number) => {
            let token = number.to_string();
            if token.contains(['.', 'e', 'E']) {
                number
                    .as_f64()
                    .filter(|value| value.is_finite())
                    .ok_or(())?;
            } else {
                let integer = number.as_i64().ok_or(())?;
                let max = crate::limits::MAX_SAFE_INTEGER;
                if !(-max..=max).contains(&integer) {
                    return Err(());
                }
            }
            Ok(())
        }
        Value::Array(values) => values.iter().try_for_each(validate_json_numbers),
        Value::Object(values) => values.values().try_for_each(validate_json_numbers),
        _ => Ok(()),
    }
}

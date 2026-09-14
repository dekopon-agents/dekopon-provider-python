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
            if self.body.len() > BODY_BYTES {
                return Err(exception(
                    vm,
                    json_error(vm),
                    "JSON exceeds the safe-value limits",
                ));
            }
            validate_json_integer_lexemes(&self.body).map_err(|()| {
                exception(vm, json_error(vm), "JSON exceeds the safe-value limits")
            })?;
            // serde remains the JSON grammar authority, with its default recursion ceiling.
            // No synthetic-number/raw-value features or guest decoder hooks are enabled.
            let value: serde_json::Value = serde_json::from_slice(&self.body)
                .map_err(|_| exception(vm, json_error(vm), "invalid JSON response"))?;
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
        include!("../tests/fixtures/requests_json_objects.rs");

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
                    for iteration in 0..3 {
                        crate::eval::interpreter().enter(|vm| {
                            let scope = vm.new_scope_with_builtins();
                            scope
                                .globals
                                .set_item("iteration", vm.new_pyobj(iteration), vm)
                                .unwrap();
                            vm.run_string(
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
                            vm.run_string(
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
    annotations = original.__annotations__
    assert annotations.get('marker', 0) == iteration
    annotations['marker'] = iteration + 1
    bases, mro = original.__bases__, original.__mro__
    for attribute, value in [('__bases__', (Exception,)), ('__mro__', (Exception,))]:
        try:
            setattr(original, attribute, value)
        except (TypeError, AttributeError):
            pass
        else:
            raise AssertionError('mutable native layout')
    assert original.__bases__ == bases and original.__mro__ == mro
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
                            "-1e400",
                            "99999999999999999999999999999999999999999999999999",
                        ] {
                            for body in [
                                token.to_owned(),
                                format!("{{\"nested\":[{token}]}}"),
                                format!("{{\"$serde_json::private::Number\":{token}}}"),
                                format!(r#"["\\",{token}]"#),
                                format!(r#"["\"",{token}]"#),
                            ] {
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
                            "1E+30",
                            "1e-400",
                            "-0",
                        ] {
                            let value = Response {
                                status: 200,
                                body: token.as_bytes().to_vec(),
                            }
                            .json(vm)
                            .expect(token);
                            let value = crate::value::py_to_safe_json(&value, vm).unwrap();
                            if token.contains(['.', 'e', 'E']) || token == "-0" {
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
        fn json_reserved_key_objects_preserve_json_semantics() {
            std::thread::Builder::new()
                .stack_size(32 * 1024 * 1024)
                .spawn(|| {
                    crate::eval::interpreter().enter(|vm| {
                        for (body, expected) in json_object_cases() {
                            let value = Response {
                                status: 200,
                                body: body.as_bytes().to_vec(),
                            }
                            .json(vm)
                            .expect(body);
                            assert_eq!(
                                crate::value::py_to_safe_json(&value, vm).unwrap(),
                                expected,
                                "{body}"
                            );
                        }
                    });
                })
                .unwrap()
                .join()
                .unwrap();
        }

        #[test]
        fn response_json_enforces_body_depth_node_limits_and_grammar() {
            use crate::limits::{MAX_DEPTH, MAX_NODES};
            std::thread::Builder::new()
                .stack_size(32 * 1024 * 1024)
                .spawn(|| {
                    crate::eval::interpreter().enter(|vm| {
                        for (body, accepted) in [
                            (
                                format!(
                                    "{}0{}",
                                    "[".repeat(MAX_DEPTH - 1),
                                    "]".repeat(MAX_DEPTH - 1)
                                ),
                                true,
                            ),
                            (
                                format!("{}0{}", "[".repeat(MAX_DEPTH), "]".repeat(MAX_DEPTH)),
                                false,
                            ),
                            (format!("[{}0]", "0,".repeat(MAX_NODES - 2)), true),
                            (format!("[{}0]", "0,".repeat(MAX_NODES - 1)), false),
                            (
                                format!(
                                    "{{{}}}",
                                    (0..MAX_NODES / 2)
                                        .map(|i| format!("\"{i}\":0"))
                                        .collect::<Vec<_>>()
                                        .join(",")
                                ),
                                false,
                            ),
                            (format!("\"{}\"", "x".repeat(BODY_BYTES - 2)), true),
                            (format!("\"{}\"", "x".repeat(BODY_BYTES - 1)), false),
                            ("[".repeat(BODY_BYTES), false),
                            (format!("{}0{}", "[".repeat(1000), "]".repeat(1000)), false),
                            ("1 2".to_owned(), false),
                            ("01".to_owned(), false),
                            ("[1,]".to_owned(), false),
                            ("1e".to_owned(), false),
                            ("--1".to_owned(), false),
                            (r#""\q""#.to_owned(), false),
                            (r#"{"x":1e400}"#.to_owned(), false),
                        ] {
                            assert_eq!(
                                Response {
                                    status: 200,
                                    body: body.as_bytes().to_vec()
                                }
                                .json(vm)
                                .is_ok(),
                                accepted,
                                "body length {}",
                                body.len()
                            );
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

// Preflight only, not a JSON parser: serde subsequently validates all grammar and UTF-8.
// Scan the bounded body once, skipping strings (including escaped quotes/backslashes), so integer
// lexemes cannot silently become rounded f64 values. Decimal/exponent tokens are left to serde's
// finite-float decoding. No serde features that reinterpret actual object keys are needed.
fn validate_json_integer_lexemes(body: &[u8]) -> Result<(), ()> {
    let max = crate::limits::MAX_SAFE_INTEGER.to_string();
    let mut index = 0;
    let mut in_string = false;
    while let Some(&byte) = body.get(index) {
        if in_string {
            match byte {
                b'\\' => index += 1, // The escaped byte cannot end the string.
                b'"' => in_string = false,
                _ => {}
            }
            index += 1;
        } else if byte == b'"' {
            in_string = true;
            index += 1;
        } else if matches!(byte, b'-' | b'0'..=b'9') {
            let start = index;
            while body
                .get(index)
                .is_some_and(|byte| matches!(byte, b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E'))
            {
                index += 1;
            }
            let token = &body[start..index];
            if !token.iter().any(|byte| matches!(byte, b'.' | b'e' | b'E')) {
                let magnitude = token.strip_prefix(b"-").unwrap_or(token);
                if magnitude.len() > max.len()
                    || (magnitude.len() == max.len() && magnitude > max.as_bytes())
                {
                    return Err(());
                }
            }
        } else {
            index += 1;
        }
    }
    Ok(())
}

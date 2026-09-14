//! Native buffered HTTP facade. Only the broker import supplies transport or authority.
use rustpython_vm::{
    PyPayload, PyResult, TryFromObject, VirtualMachine,
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
        vm.ctx.new_exception_type(
            "dekopon_requests",
            "RequestException",
            Some(vec![vm.ctx.exceptions.exception_type.to_owned()]),
        )
    }

    #[pyattr(name = "HTTPError", once)]
    fn http_error(vm: &VirtualMachine) -> PyTypeRef {
        vm.ctx
            .new_exception_type("dekopon_requests", "HTTPError", Some(vec![error(vm)]))
    }

    #[pyattr(name = "JSONDecodeError", once)]
    fn json_error(vm: &VirtualMachine) -> PyTypeRef {
        vm.ctx
            .new_exception_type("dekopon_requests", "JSONDecodeError", Some(vec![error(vm)]))
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
                .map_err(|_| exception(vm, "JSONDecodeError", "invalid JSON response"))?;
            let value = crate::value::safe_json_to_py(&value, vm);
            crate::value::py_to_safe_json(&value, vm).map_err(|_| {
                exception(vm, "JSONDecodeError", "JSON exceeds the safe-value limits")
            })?;
            Ok(value)
        }

        #[pymethod]
        fn raise_for_status(&self, vm: &VirtualMachine) -> PyResult<()> {
            if self.status >= 400 {
                return Err(exception(
                    vm,
                    "HTTPError",
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
        fn http_wit_matches_the_published_guest_binding() {
            assert_eq!(
                include_str!("../wit/http/http.wit"),
                dekopon_provider_http::HTTP_WIT
            );
        }
    }

    fn request(method: &str, url: &str, vm: &VirtualMachine) -> PyResult<Response> {
        if url.len() > URL_BYTES {
            return Err(exception(
                vm,
                "RequestException",
                "URL exceeds 8192 UTF-8 bytes",
            ));
        }
        let request = dekopon_provider_http::Request::new(method, url)
            .map_err(|_| exception(vm, "RequestException", "invalid URL"))?;
        let response = dekopon_provider_http::send(request)
            // Stable code only: never copy remote text, URL, or potentially sensitive host detail.
            .map_err(|error| exception(vm, "RequestException", error.code.as_str()))?;
        if response.body.len() > BODY_BYTES {
            return Err(exception(
                vm,
                "RequestException",
                "response exceeds 131072 bytes",
            ));
        }
        Ok(Response {
            status: response.status,
            body: response.body,
        })
    }
}

fn exception(vm: &VirtualMachine, name: &'static str, message: &str) -> PyBaseExceptionRef {
    let class = vm
        .sys_module
        .get_attr("modules", vm)
        .and_then(|modules| modules.get_item("dekopon_requests", vm))
        .and_then(|module| module.get_attr(name, vm))
        .and_then(|class| PyTypeRef::try_from_object(vm, class));
    match class {
        Ok(class) => vm.new_exception_msg(class, message.to_owned().into()),
        Err(_) => vm.new_runtime_error(message.to_owned()),
    }
}

//! One constrained RustPython capability; the broker authorizes each invocation before the VM runs.
mod capture;
mod commands;
mod entropy;
mod eval;
mod exception;
mod limits;
mod policy;
#[cfg(feature = "http")]
mod requests;
mod value;
mod yaml;

use dekopon_provider_sdk::provider::{
    self, Capability, Code, Failure, Http, Proposal, Provider, Stdout, Usage,
};
use dekopon_provider_sdk::{EffectKind, RiskLevel};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    borrow::Cow,
    fmt,
    io::{Read, Write},
};

use crate::limits::SCRIPT_BYTES;

pub(crate) const COMMAND_WORD: &str = "python";

/// The Python command provider.
pub struct PythonProvider;
/// Execute one script in a fresh constrained interpreter.
pub struct Eval;

/// An exact, bounded script or an authorized request to read the script from stdin.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EvalInput {
    #[schemars(length(max = 65_536))]
    script: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    stdin_script: bool,
}

#[derive(Debug)]
pub struct PythonError {
    code: Code,
    message: Cow<'static, str>,
}
impl PythonError {
    fn new(code: Code, message: impl Into<Cow<'static, str>>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}
impl fmt::Display for PythonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl Failure for PythonError {
    fn code(&self) -> Code {
        self.code
    }
}

impl Provider for PythonProvider {
    const ID: &'static str = "python";
    const COMMAND_WORDS: &'static [&'static str] = &[COMMAND_WORD];
    const DESCRIPTION: &'static str = commands::ABOUT;
    type Args = commands::Python;
    type Capabilities = (Eval,);

    fn propose(args: Self::Args, stdin_piped: bool) -> Result<Proposal<Self>, Usage> {
        commands::propose(args, stdin_piped)
    }
}

impl Capability for Eval {
    type Provider = PythonProvider;
    const NAME: &'static str = "eval";
    const DESCRIPTION: &'static str = "Evaluate a bounded script with json, re, yaml and broker-granted dekopon_requests GET/HEAD; assign output to result";
    const EFFECT: EffectKind = EffectKind::ReadOnly;
    const RISK: RiskLevel = RiskLevel::High;
    type Input = EvalInput;
    type Needs = Http;
    type Error = PythonError;

    fn run(mut input: Self::Input, http: Http, out: &mut Stdout) -> Result<(), Self::Error> {
        if input.stdin_script {
            // Marker is proposal data, never pre-authorization script data.
            if !input.script.is_empty() {
                return Err(PythonError::new(
                    Code::INVALID_INPUT,
                    "stdin script cannot contain inline code",
                ));
            }
            let pipe = provider::stdin()
                .ok_or_else(|| PythonError::new(Code::USAGE, "python -: nothing was piped in"))?;
            let mut raw = Vec::new();
            pipe.take((SCRIPT_BYTES + 1) as u64)
                .read_to_end(&mut raw)
                .map_err(|_| PythonError::new(Code::USAGE, "python -: invalid piped script"))?;
            if raw.len() > SCRIPT_BYTES {
                return Err(PythonError::new(
                    Code::new("input-too-large"),
                    format!("script exceeds {SCRIPT_BYTES} UTF-8 bytes"),
                ));
            }
            input.script = String::from_utf8(raw)
                .map_err(|_| PythonError::new(Code::USAGE, "python -: invalid UTF-8 script"))?;
        }
        if input.script.len() > SCRIPT_BYTES {
            return Err(PythonError::new(
                Code::new("input-too-large"),
                format!("script exceeds {SCRIPT_BYTES} UTF-8 bytes"),
            ));
        }
        #[cfg(feature = "http")]
        let result = requests::with_http(http, || eval::evaluate(&input.script));
        #[cfg(not(feature = "http"))]
        let result = {
            let _ = http;
            eval::evaluate(&input.script)
        };
        let mut json = serde_json::to_vec(&result).map_err(|_| {
            PythonError::new(
                Code::new("serialization-failed"),
                "python response could not be serialized",
            )
        })?;
        json.push(b'\n');
        out.write_all(&json).map_err(|_| {
            PythonError::new(
                Code::new("write-failed"),
                "python output could not be written",
            )
        })
    }
}

#[allow(unsafe_code)]
mod export {
    dekopon_provider_sdk::export!(super::PythonProvider);
}

#[cfg(all(test, feature = "http"))]
mod tests {
    #[test]
    fn outside_invocation_get_has_no_handle() {
        std::thread::Builder::new()
            .stack_size(32 * 1024 * 1024)
            .spawn(|| {
                let result = crate::eval::evaluate(
                    "import dekopon_requests as r\nr.get('https://crates.io/api/v1/crates/serde')",
                );
                assert_eq!(result["ok"], false);
                assert_eq!(result["error"]["message"], "http not granted");
                let native = dekopon_provider_sdk_testkit::Native::<crate::PythonProvider>::new()
                    .http(dekopon_provider_sdk_testkit::HttpScript::new("crates.io", "GET", dekopon_provider_sdk::provider::Response { status: 200, headers: vec![], body: b"ok".to_vec() }));
                let first = native.call("python.eval", &serde_json::json!({"script":"import dekopon_requests as r\nresult = r.get('https://crates.io/').text"}).to_string());
                assert_eq!(first.status, 0, "{}", first.stderr);
                assert_eq!(serde_json::from_slice::<serde_json::Value>(&first.stdout).unwrap()["result"], "ok");
                let pure = dekopon_provider_sdk_testkit::Native::<crate::PythonProvider>::new()
                    .call("python.eval", &serde_json::json!({"script":"result = 1"}).to_string());
                assert_eq!(pure.status, 0, "{}", pure.stderr);
                let after = crate::eval::evaluate("import dekopon_requests as r\nr.get('https://crates.io/api/v1/crates/serde')");
                assert_eq!(after["error"]["message"], "http not granted");
            })
            .unwrap()
            .join()
            .unwrap();
    }
}

//! One constrained RustPython capability; the broker authorizes each invocation before the VM runs.
mod capture;
mod commands;
mod entropy;
mod eval;
mod exception;
mod limits;
mod numeric;
mod policy;
#[cfg(feature = "http")]
mod requests;
mod sqlite;
#[cfg(feature = "http")]
mod subshell;
#[cfg(feature = "tables")]
mod tables;
mod time;
mod value;
mod yaml;

use dekopon_provider_sdk::provider::{
    self, Capability, Clock, Code, DurableFiles, Failure, Http, Monotonic, Proposal, Provider,
    Random, Spawn, Stdout, Storage, Usage,
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
/// Separately authorized persistent SQL evaluation.
pub struct SqliteEval;

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
    type Capabilities = (Eval, SqliteEval);

    fn propose(args: Self::Args, stdin_piped: bool) -> Result<Proposal<Self>, Usage> {
        commands::propose(args, stdin_piped)
    }
}

impl Capability for Eval {
    type Provider = PythonProvider;
    const NAME: &'static str = "eval";
    const DESCRIPTION: &'static str = commands::CAPABILITY_DESCRIPTION;
    const EFFECT: EffectKind = EffectKind::ReadOnly;
    const RISK: RiskLevel = RiskLevel::High;
    type Input = EvalInput;
    type Needs = (Http, Clock, Monotonic, Random, Spawn);
    type Error = PythonError;

    fn run(
        input: Self::Input,
        (http, clock, monotonic, random, spawn): Self::Needs,
        out: &mut Stdout,
    ) -> Result<(), Self::Error> {
        let pipe = if input.stdin_script {
            provider::stdin()
        } else {
            None
        };
        let script = read_script(input, pipe, false)?;
        let _entropy = entropy::EntropyScope::install(random);
        let result = time::with_clocks(clock, monotonic, || {
            #[cfg(feature = "http")]
            {
                requests::with_http(http, || {
                    subshell::with_spawn(spawn, || eval::evaluate(&script))
                })
            }
            #[cfg(not(feature = "http"))]
            {
                let _ = (http, spawn);
                eval::evaluate(&script)
            }
        });
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

impl Capability for SqliteEval {
    type Provider = PythonProvider;
    const NAME: &'static str = "sqlite-eval";
    const DESCRIPTION: &'static str =
        "Separately authorized Python SQL over broker-owned durable files";
    const EFFECT: EffectKind = EffectKind::LocalWrite;
    const RISK: RiskLevel = RiskLevel::High;
    type Input = EvalInput;
    type Needs = (Storage<DurableFiles>, Clock, Monotonic, Random);
    type Error = PythonError;

    fn run(
        input: Self::Input,
        (storage, clock, monotonic, random): Self::Needs,
        out: &mut Stdout,
    ) -> Result<(), Self::Error> {
        let pipe = if input.stdin_script {
            provider::stdin()
        } else {
            None
        };
        let script = read_script(input, pipe, true)?;
        let _entropy = entropy::EntropyScope::install(random);
        let result = time::with_clocks(clock, monotonic, || {
            dekopon_turso_sql_io::with_database(
                storage,
                |stage, detail| {
                    PythonError::new(
                        Code::new("sql-error"),
                        format!("sqlite {stage} failed: {detail}"),
                    )
                },
                |connection, engine| {
                    sqlite::with_connection(connection, engine, || eval::evaluate_sql(&script))
                        .map_err(|detail| {
                            PythonError::new(
                                Code::new("sql-error"),
                                format!("sqlite rollback failed: {detail}"),
                            )
                        })
                },
            )
        })?;
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

// Validation is deliberately independent of the storage handle: even an ungranted
// storage host must see the input error before any durable-files import is called.
fn read_script(
    mut input: EvalInput,
    pipe: Option<impl Read>,
    sql: bool,
) -> Result<String, PythonError> {
    let usage = Code::USAGE;
    let too_large = if sql {
        Code::INVALID_INPUT
    } else {
        Code::new("input-too-large")
    };
    let command = if sql { "python sql -" } else { "python -" };
    if input.stdin_script {
        if !input.script.is_empty() {
            return Err(PythonError::new(
                Code::INVALID_INPUT,
                "stdin script cannot contain inline code",
            ));
        }
        let mut raw = Vec::new();
        pipe.ok_or_else(|| PythonError::new(usage, format!("{command}: nothing was piped in")))?
            .take((SCRIPT_BYTES + 1) as u64)
            .read_to_end(&mut raw)
            .map_err(|_| PythonError::new(usage, format!("{command}: invalid piped script")))?;
        if raw.len() > SCRIPT_BYTES {
            return Err(PythonError::new(
                too_large,
                format!("script exceeds {SCRIPT_BYTES} UTF-8 bytes"),
            ));
        }
        input.script = String::from_utf8(raw)
            .map_err(|_| PythonError::new(usage, format!("{command}: invalid UTF-8 script")))?;
    }
    if input.script.len() > SCRIPT_BYTES {
        return Err(PythonError::new(
            too_large,
            format!("script exceeds {SCRIPT_BYTES} UTF-8 bytes"),
        ));
    }
    Ok(input.script)
}

#[cfg(test)]
mod sql_input_tests {
    use super::{EvalInput, read_script};
    use dekopon_provider_sdk::provider::{Code, Failure};
    use std::io::Cursor;

    #[test]
    fn piped_sql_errors_match_eval_usage_and_bound_input_before_storage() {
        let input = || EvalInput {
            script: String::new(),
            stdin_script: true,
        };
        assert_eq!(
            read_script(input(), None::<Cursor<Vec<u8>>>, false)
                .unwrap_err()
                .code(),
            read_script(input(), None::<Cursor<Vec<u8>>>, true)
                .unwrap_err()
                .code(),
        );
        for (pipe, code) in [
            (None, Code::USAGE),
            (Some(vec![b'x'; 65_537]), Code::INVALID_INPUT),
            (Some(vec![0xff]), Code::USAGE),
        ] {
            let result = read_script(input(), pipe.map(Cursor::new), true);
            assert_eq!(result.unwrap_err().code(), code);
        }
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
                let first = native.call("python.eval", &serde_json::json!({"script":"import dekopon_requests as r\nresult = r.get('https://crates.io/').status_code"}).to_string());
                assert_eq!(first.status, 0, "{}", first.stderr);
                assert_eq!(serde_json::from_slice::<serde_json::Value>(&first.stdout).unwrap()["result"], 200, "{:?}", first.stdout);
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

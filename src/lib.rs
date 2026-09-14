//! A single constrained RustPython capability for Dekopon, and the `python` command word for it.
//!
//! The default component has no imports; feature `http` adds only broker-mediated HTTP. Every invocation creates a fresh VM, captures bounded stdout in
//! Rust, and projects only an explicitly bounded JSON value model. Resource termination remains a
//! host responsibility: provider code cannot catch Wasmtime fuel, deadline, or memory traps.
//!
//! `run-command` is pure argv parsing in `commands`: it renders help and usage errors or proposes
//! `python.eval`, and never constructs a VM.

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

use dekopon_provider_sdk::{
    CapabilityId, CommandRun, EffectKind, Provider, ProviderApiVersion, ProviderCapability,
    ProviderError, ProviderManifest, RiskLevel,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::limits::SCRIPT_BYTES;

/// The one capability, named once for the manifest, `invoke`, and the command word.
#[cfg(not(feature = "http"))]
pub(crate) const EVAL: &str = "python.eval";
#[cfg(feature = "http")]
pub(crate) const EVAL: &str = "python.eval-http";
/// The command word this provider contributes to the sandboxed shell.
pub(crate) const COMMAND_WORD: &str = "python";

mod bindings {
    wit_bindgen::generate!({
        path: "wit",
        world: "provider-cli",
        pub_export_macro: true,
    });
}

struct PythonProvider;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EvalInput {
    script: String,
}

impl Provider for PythonProvider {
    fn manifest() -> ProviderManifest {
        ProviderManifest {
            api_version: ProviderApiVersion::V1Alpha1,
            id: "python".parse().expect("static provider identifier"),
            description: crate::commands::ABOUT.to_owned(),
            command_words: vec![COMMAND_WORD.to_owned()],
            capabilities: vec![ProviderCapability {
                id: EVAL.parse().expect("static capability identifier"),
                description: if cfg!(feature = "http") {
                    "Evaluate a bounded script with json, re, yaml and dekopon_requests GET/HEAD under the host HTTP invocation grant; assign output to result"
                } else {
                    "Evaluate a bounded Python 3 script with json, re, and constrained yaml; assign the safe JSON-shaped return value to result"
                }.to_owned(),
                effect: EffectKind::ReadOnly,
                risk: RiskLevel::High,
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "script": {
                            "type": "string",
                            "maxLength": SCRIPT_BYTES,
                            "description": "Python 3 source executed in Mode::Exec. The provider enforces 65,536 UTF-8 bytes; assign output to result."
                        }
                    },
                    "required": ["script"],
                    "additionalProperties": false
                }),
            }],
        }
    }

    fn invoke(capability: &CapabilityId, input: Value) -> Result<Value, ProviderError> {
        if capability.as_str() != EVAL {
            return Err(ProviderError::new(
                "unsupported-capability",
                format!("the python provider exposes only {EVAL}"),
            ));
        }
        let EvalInput { script } = serde_json::from_value(input).map_err(|_error| {
            ProviderError::new(
                "invalid-input",
                "input must be exactly an object with one string field named script",
            )
        })?;
        if script.len() > SCRIPT_BYTES {
            return Err(ProviderError::new(
                "input-too-large",
                format!("script exceeds {SCRIPT_BYTES} UTF-8 bytes"),
            ));
        }
        Ok(eval::evaluate(&script))
    }

    fn run_command(argv: &[String], stdin: Option<&str>) -> Result<CommandRun, ProviderError> {
        commands::run(argv, stdin)
    }
}

dekopon_provider_sdk::export_provider_with_cli!(PythonProvider, bindings);

#[cfg(test)]
mod tests {
    use dekopon_provider_sdk::{EffectKind, Provider, RiskLevel};
    use serde_json::json;

    use super::{COMMAND_WORD, EVAL, PythonProvider, SCRIPT_BYTES};

    fn capability(value: &str) -> dekopon_provider_sdk::CapabilityId {
        value.parse().expect("valid capability fixture")
    }

    #[test]
    fn mirrored_wit_and_manifest_are_exact() {
        assert_eq!(
            include_str!("../wit/provider.wit"),
            dekopon_provider_sdk::PROVIDER_WIT
        );
        let manifest = PythonProvider::manifest();
        assert_eq!(manifest.id.as_str(), "python");
        assert_eq!(manifest.command_words, [COMMAND_WORD]);
        assert_eq!(manifest.capabilities.len(), 1);
        let capability = &manifest.capabilities[0];
        assert_eq!(capability.id.as_str(), EVAL);
        assert_eq!(capability.effect, EffectKind::ReadOnly);
        assert_eq!(capability.risk, RiskLevel::High);
        assert_eq!(capability.input_schema["additionalProperties"], false);
        assert_eq!(
            capability.input_schema["properties"]["script"]["maxLength"],
            SCRIPT_BYTES
        );
    }

    #[test]
    fn invocation_input_is_exact_and_bounded_before_vm_construction() {
        for input in [
            json!(null),
            json!({}),
            json!({"script": 1}),
            json!({"script": "result = 1", "extra": true}),
        ] {
            let error =
                PythonProvider::invoke(&capability(EVAL), input).expect_err("invalid input");
            assert_eq!(error.code(), "invalid-input");
        }
        let error = PythonProvider::invoke(
            &capability(EVAL),
            json!({"script": "x".repeat(SCRIPT_BYTES + 1)}),
        )
        .expect_err("oversized script");
        assert_eq!(error.code(), "input-too-large");

        let error = PythonProvider::invoke(&capability("python.other"), json!({"script": ""}))
            .expect_err("unknown capability");
        assert_eq!(error.code(), "unsupported-capability");
    }
}

//! A single constrained RustPython capability for Dekopon.
//!
//! The component has no imports. Every invocation creates a fresh VM, captures bounded stdout in
//! Rust, and projects only an explicitly bounded JSON value model. Resource termination remains a
//! host responsibility: provider code cannot catch Wasmtime fuel, deadline, or memory traps.

mod capture;
mod entropy;
mod eval;
mod limits;
mod policy;
mod value;
mod yaml;

use dekopon_provider_sdk::{
    CapabilityId, EffectKind, Idempotency, Provider, ProviderApiVersion, ProviderCapability,
    ProviderError, ProviderManifest, RiskLevel,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::limits::SCRIPT_BYTES;

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
            description: "Runs one bounded Python 3 script in a fresh import-free RustPython 0.5.0 VM"
                .to_owned(),
            command_words: Vec::new(),
            capabilities: vec![ProviderCapability {
                id: "python.eval"
                    .parse()
                    .expect("static capability identifier"),
                description: "Evaluate a bounded Python 3 script with json, re, and constrained yaml; assign the safe JSON-shaped return value to result"
                    .to_owned(),
                effect: EffectKind::ReadOnly,
                risk: RiskLevel::High,
                idempotency: Idempotency::Idempotent,
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
        if capability.as_str() != "python.eval" {
            return Err(ProviderError::new(
                "unsupported-capability",
                "the python provider exposes only python.eval",
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
}

dekopon_provider_sdk::export_provider!(PythonProvider);

#[cfg(test)]
mod tests {
    use dekopon_provider_sdk::{EffectKind, Idempotency, Provider, RiskLevel};
    use serde_json::json;

    use super::{PythonProvider, SCRIPT_BYTES};

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
        assert!(manifest.command_words.is_empty());
        assert_eq!(manifest.capabilities.len(), 1);
        let capability = &manifest.capabilities[0];
        assert_eq!(capability.id.as_str(), "python.eval");
        assert_eq!(capability.effect, EffectKind::ReadOnly);
        assert_eq!(capability.risk, RiskLevel::High);
        assert_eq!(capability.idempotency, Idempotency::Idempotent);
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
            let error = PythonProvider::invoke(&capability("python.eval"), input)
                .expect_err("invalid input");
            assert_eq!(error.code(), "invalid-input");
        }
        let error = PythonProvider::invoke(
            &capability("python.eval"),
            json!({"script": "x".repeat(SCRIPT_BYTES + 1)}),
        )
        .expect_err("oversized script");
        assert_eq!(error.code(), "input-too-large");

        let error = PythonProvider::invoke(&capability("python.other"), json!({"script": ""}))
            .expect_err("unknown capability");
        assert_eq!(error.code(), "unsupported-capability");
    }
}

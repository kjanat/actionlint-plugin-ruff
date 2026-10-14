//! Ruff owns Python analysis and option validation; actionlint owns script selection and YAML mapping.

mod engine;
#[cfg(target_arch = "wasm32")]
mod wasm;

use serde::Deserialize;
use serde_json::Value;
use serde_json::json;

pub const ABI_VERSION: u32 = 1;
pub const METADATA_VERSION: u32 = 1;
pub const MAX_INPUT_BYTES: usize = 8 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckRequest {
    pub source: String,
    pub filename: String,
    #[serde(default = "empty_config")]
    pub config: Value,
}

fn empty_config() -> Value {
    json!({})
}

pub fn failure(message: impl ToString) -> Value {
    json!({"ok": false, "errors": [message.to_string()]})
}

fn decode<T: serde::de::DeserializeOwned>(input: &[u8]) -> Result<T, String> {
    if input.len() > MAX_INPUT_BYTES {
        return Err(format!("request exceeds {MAX_INPUT_BYTES} bytes"));
    }
    serde_json::from_slice(input).map_err(|e| format!("invalid request JSON: {e}"))
}

/// Check one already-sanitized Python snippet. Coordinates are one-based Unicode characters.
pub fn check(input: &[u8]) -> Value {
    let result = decode::<CheckRequest>(input).and_then(engine::check);
    match result {
        Ok(diagnostics) => json!({"ok": true, "diagnostics": diagnostics}),
        Err(error) => failure(error),
    }
}

/// Validate a Ruff options object with exactly the same path used by `check`.
pub fn validate_config(input: &[u8]) -> Value {
    match decode::<Value>(input).and_then(engine::settings) {
        Ok(_) => json!({"ok": true, "errors": []}),
        Err(error) => failure(error),
    }
}

/// Generate metadata and schema from the linked Ruff crates, without checked-in catalogs.
pub fn metadata() -> Value {
    let mut schema = serde_json::to_value(schemars::schema_for!(ruff_workspace::options::Options))
        .expect("Ruff schema is JSON serializable");
    // Runtime and editor completion expose the same offline subset of Ruff options.
    for key in engine::UNSUPPORTED_OPTIONS {
        schema["properties"].as_object_mut().unwrap().remove(*key);
    }
    schema["properties"]["target-version"]["default"] = json!("py314");
    json!({
        "metadata_version": METADATA_VERSION,
        "abi_version": ABI_VERSION,
        "transport": "wasm-core-json",
        "id": env!("CARGO_PKG_NAME"),
        "name": "Ruff",
        "description": env!("CARGO_PKG_DESCRIPTION"),
        "license": env!("CARGO_PKG_LICENSE"),
        "homepage": env!("CARGO_PKG_HOMEPAGE"),
        "repository": env!("CARGO_PKG_REPOSITORY"),
        "documentation_url": "https://github.com/kjanat/actionlint-plugin-ruff/blob/master/docs/plugin-contract.md",
        "update_url": "https://api.github.com/repos/kjanat/actionlint-plugin-ruff/releases",
        "update_format": "github-releases-v1",
        "languages": ["python"],
        "shells": ["python"],
        "file_extensions": ["py", "pyi"],
        "conflicts": [],
        "plugin_version": env!("CARGO_PKG_VERSION"),
        "engine": {"name": "ruff", "version": ruff_linter::VERSION},
        "interfaces": {
            "check": [{"version": 1, "export": "check", "exclusive_group": "python/ruff/check"}],
            "validate_config": [{"version": 1, "export": "validate_config"}],
            "metadata": [{"version": 1, "export": "metadata"}]
        },
        "capabilities": {
            "check": true, "validate_config": true, "config_schema": true,
            "noqa": false, "filesystem": false, "fixes": false, "format": false,
            "position_encoding": "unicode", "max_input_bytes": MAX_INPUT_BYTES
        },
        "config_schema": schema
    })
}

//! Analyze Python snippets and validate Ruff options for actionlint.
//! The actionlint host selects scripts and maps locations to YAML.

mod engine;
pub mod template_effects;
#[cfg(all(target_arch = "wasm32", target_os = "unknown", feature = "core-abi"))]
mod wasm;

use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use serde_json::json;

pub const ABI_VERSION: u32 = 1;
pub const METADATA_VERSION: u32 = 1;
pub const MAX_INPUT_BYTES: usize = 8 * 1024 * 1024;
pub const PLUGIN_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const ENGINE_VERSION: &str = ruff_linter::VERSION;
/// Python version used when neither configuration nor the host supplies one.
pub const DEFAULT_TARGET_VERSION: &str = "py314";

pub use engine::Diagnostic;
pub use engine::Position;
pub use engine::Severity;

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

/// Check a sanitized Python snippet and return one-based Unicode character coordinates.
pub fn check(input: &[u8]) -> Value {
    let result = decode::<CheckRequest>(input).and_then(check_request);
    match result {
        Ok(diagnostics) => json!({"ok": true, "diagnostics": diagnostics}),
        Err(error) => failure(error),
    }
}

/// Check Ruff options with the same validation used for snippet analysis.
pub fn validate_config(input: &[u8]) -> Value {
    match decode::<Value>(input).and_then(validate_options) {
        Ok(_) => json!({"ok": true, "errors": []}),
        Err(error) => failure(error),
    }
}

/// Analyze a request shared by the core module and component.
pub fn check_request(request: CheckRequest) -> Result<Vec<Diagnostic>, String> {
    engine::check(request)
}

/// Resolve decoded Ruff options as the analysis function does.
pub fn validate_options(config: Value) -> Result<(), String> {
    engine::settings(config).map(|_| ())
}

/// Describe the rule exposed by both transports.
#[derive(Serialize)]
pub struct RuleDescriptor {
    pub name: String,
    pub description: String,
    pub category: String,
    pub subcategory: String,
    pub maturity: String,
    pub provider: String,
    pub recommended: bool,
    pub explanation: String,
    pub help: String,
    pub documentation_url: String,
    pub options_schema: Value,
}

pub fn rule_descriptor() -> RuleDescriptor {
    RuleDescriptor {
        name: "ruff".into(),
        description: "Check workflow Python scripts with Ruff".into(),
        category: "correctness".into(),
        subcategory: "python".into(),
        maturity: "stable".into(),
        provider: env!("CARGO_PKG_NAME").into(),
        recommended: true,
        explanation: "The host masks workflow expressions before Ruff checks the Python script."
            .into(),
        help: "Fix the reported Python code or change tools.ruff.select and tools.ruff.ignore."
            .into(),
        documentation_url: "https://docs.astral.sh/ruff/rules/".into(),
        options_schema: config_schema(),
    }
}

/// Build the supported options schema from the linked Ruff types.
pub fn config_schema() -> Value {
    let mut schema = serde_json::to_value(schemars::schema_for!(ruff_workspace::options::Options))
        .expect("Ruff schema is JSON serializable");
    // The runtime and editor schema use the same options without filesystem access.
    for key in engine::UNSUPPORTED_OPTIONS {
        schema["properties"].as_object_mut().unwrap().remove(*key);
    }
    schema["properties"]["target-version"]["default"] = json!(DEFAULT_TARGET_VERSION);
    schema
}

/// Build metadata and schema from Ruff rather than a checked-in catalog.
pub fn metadata() -> Value {
    let rule = rule_descriptor();
    let schema = rule.options_schema.clone();
    json!({
        "metadata_version": METADATA_VERSION,
        "template_contract": template_effects::CONTRACT,
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
            "check": [
                {"version": 2, "export": "check", "exclusive_group": "python/ruff/check"},
                {"version": 1, "export": "check", "exclusive_group": "python/ruff/check"}
            ],
            "validate_config": [{"version": 1, "export": "validate_config"}],
            "metadata": [{"version": 1, "export": "metadata"}]
        },
        "capabilities": {
            "check": true, "validate_config": true, "config_schema": true,
            "noqa": false, "filesystem": false, "fixes": false, "format": false,
            "position_encoding": "unicode", "max_input_bytes": MAX_INPUT_BYTES,
            "script_shells": ["python"]
        },
        "rules": [rule],
        "config_schema": schema
    })
}

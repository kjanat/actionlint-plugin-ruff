//! Typed Component Model transport over the same hermetic Ruff analysis engine.

use actionlint_plugin_ruff as engine;
use serde_json::Value;
use serde_json::json;
use sha2::Digest;
use sha2::Sha256;

wit_bindgen::generate!({
    path: "wit",
    world: "script-engine-plugin",
});

use exports::kjanat::actionlint::script_engine as api;

pub struct Plugin;

impl api::Guest for Plugin {
    fn describe() -> api::Descriptor {
        descriptor()
    }

    fn validate_config(config: String) -> Result<(), String> {
        decode_config(&config).and_then(engine::validate_options)
    }

    fn check(request: api::Request) -> api::Outcome {
        let result = check_request(request);
        match result {
            Ok(diagnostics) => api::Outcome {
                diagnostics,
                error: None,
            },
            Err(error) => api::Outcome {
                diagnostics: Vec::new(),
                error: Some(error),
            },
        }
    }
}

fn decode_config(config: &str) -> Result<Value, String> {
    if config.len() > engine::MAX_INPUT_BYTES {
        return Err(format!("request exceeds {} bytes", engine::MAX_INPUT_BYTES));
    }
    if config.is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_str(config).map_err(|error| format!("invalid Ruff config JSON: {error}"))
}

fn check_request(request: api::Request) -> Result<Vec<api::Diagnostic>, String> {
    let length = request
        .source
        .len()
        .saturating_add(request.filename.len())
        .saturating_add(request.config.len());
    if length > engine::MAX_INPUT_BYTES {
        return Err(format!("request exceeds {} bytes", engine::MAX_INPUT_BYTES));
    }
    let request = engine::CheckRequest {
        source: request.source,
        filename: request.filename,
        config: decode_config(&request.config)?,
    };
    engine::check_request(request)?
        .into_iter()
        .map(diagnostic)
        .collect()
}

fn position(value: engine::Position) -> Result<api::Position, String> {
    Ok(api::Position {
        row: u32::try_from(value.row).map_err(|_| "diagnostic row exceeds u32")?,
        column: u32::try_from(value.column).map_err(|_| "diagnostic column exceeds u32")?,
    })
}

fn diagnostic(value: engine::Diagnostic) -> Result<api::Diagnostic, String> {
    Ok(api::Diagnostic {
        filename: value.filename,
        code: value.code,
        name: value.name,
        message: value.message,
        severity: match value.severity {
            engine::Severity::Info => api::Severity::Info,
            engine::Severity::Warning => api::Severity::Warning,
            engine::Severity::Error => api::Severity::Error,
            engine::Severity::Fatal => api::Severity::Fatal,
        },
        location: position(value.location)?,
        end_location: position(value.end_location)?,
        url: value.url,
    })
}

pub fn descriptor() -> api::Descriptor {
    let rule = engine::rule_descriptor();
    api::Descriptor {
        id: rule.provider.clone(),
        version: engine::PLUGIN_VERSION.into(),
        name: "Ruff".into(),
        engine: "ruff".into(),
        engine_version: engine::ENGINE_VERSION.into(),
        shells: vec!["python".into()],
        rule: api::RuleDescriptor {
            name: rule.name,
            description: rule.description,
            category: rule.category,
            subcategory: rule.subcategory,
            maturity: rule.maturity,
            provider: rule.provider,
            recommended: rule.recommended,
            explanation: rule.explanation,
            help: rule.help,
            documentation_url: rule.documentation_url,
            options_schema: rule.options_schema.to_string(),
            examples: Vec::new(),
        },
    }
}

/// Discovery metadata binds the packaged component to this exact WIT revision.
pub fn metadata() -> Value {
    let mut value = engine::metadata();
    let object = value.as_object_mut().expect("engine metadata is an object");
    object.remove("abi_version");
    object.remove("interfaces");
    value["transport"] = json!("wasm-component");
    value["interface"] = json!("kjanat:actionlint/script-engine@0.1.0");
    value["contract_sha256"] = json!(format!(
        "{:x}",
        Sha256::digest(
            include_str!("../wit/script-engine.wit")
                .replace("\r\n", "\n")
                .as_bytes()
        )
    ));
    value["capabilities"]["script_shells"] = json!(["python"]);
    value
}

export!(Plugin);

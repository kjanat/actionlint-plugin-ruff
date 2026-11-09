//! Typed Component Model transport over the same hermetic Ruff analysis engine.

use actionlint_extension_api::script_engine::v0_2::exports::kjanat::actionlint0_2_0::script_engine as api;
use actionlint_extension_api::script_engine::v0_2::exports::kjanat::actionlint0_2_0::template_effects as effects;
use actionlint_plugin_ruff as engine;
use serde_json::Value;
use serde_json::json;

pub struct Plugin;

impl effects::Guest for Plugin {
    fn contract() -> String {
        engine::template_effects::CONTRACT.into()
    }

    fn check(request: effects::Request) -> effects::Outcome {
        match engine_request(request.source, request.filename, request.config)
            .and_then(engine::check_request)
            .and_then(|findings| {
                findings
                    .into_iter()
                    .map(|finding| {
                        let effect = match finding.template_effect {
                            engine::template_effects::TemplateEffect::None => effects::Effect::None,
                            engine::template_effects::TemplateEffect::LineLength => {
                                effects::Effect::LineLength
                            }
                            engine::template_effects::TemplateEffect::CompleteReferences => {
                                effects::Effect::CompleteReferences
                            }
                            engine::template_effects::TemplateEffect::OpaqueName => {
                                effects::Effect::OpaqueName
                            }
                            engine::template_effects::TemplateEffect::OpaqueParentheses => {
                                effects::Effect::OpaqueParentheses
                            }
                            engine::template_effects::TemplateEffect::StringContents => {
                                effects::Effect::StringContents
                            }
                            engine::template_effects::TemplateEffect::DocstringSummary => {
                                effects::Effect::DocstringSummary
                            }
                            engine::template_effects::TemplateEffect::ExportedName => {
                                effects::Effect::ExportedName
                            }
                            engine::template_effects::TemplateEffect::PercentConversion => {
                                effects::Effect::PercentConversion
                            }
                        };
                        diagnostic(finding).map(|diagnostic| effects::Finding {
                            diagnostic: effects_diagnostic(diagnostic),
                            effect,
                        })
                    })
                    .collect()
            }) {
            Ok(diagnostics) => effects::Outcome {
                diagnostics,
                error: None,
            },
            Err(error) => effects::Outcome {
                diagnostics: Vec::new(),
                error: Some(error),
            },
        }
    }
}

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
    engine::check_request(engine_request(
        request.source,
        request.filename,
        request.config,
    )?)?
    .into_iter()
    .map(diagnostic)
    .collect()
}

fn engine_request(
    source: String,
    filename: String,
    config: String,
) -> Result<engine::CheckRequest, String> {
    let length = source
        .len()
        .saturating_add(filename.len())
        .saturating_add(config.len());
    if length > engine::MAX_INPUT_BYTES {
        return Err(format!("request exceeds {} bytes", engine::MAX_INPUT_BYTES));
    }
    Ok(engine::CheckRequest {
        source,
        filename,
        config: decode_config(&config)?,
    })
}

fn effects_diagnostic(value: api::Diagnostic) -> effects::Diagnostic {
    effects::Diagnostic {
        filename: value.filename,
        code: value.code,
        name: value.name,
        message: value.message,
        severity: match value.severity {
            api::Severity::Info => effects::Severity::Info,
            api::Severity::Warning => effects::Severity::Warning,
            api::Severity::Error => effects::Severity::Error,
            api::Severity::Fatal => effects::Severity::Fatal,
        },
        location: effects::Position {
            row: value.location.row,
            column: value.location.column,
        },
        end_location: effects::Position {
            row: value.end_location.row,
            column: value.end_location.column,
        },
        url: value.url,
    }
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

/// Generates component discovery metadata from the descriptor and SDK.
#[cfg(not(target_arch = "wasm32"))]
pub fn metadata() -> Result<Value, Box<dyn std::error::Error>> {
    use actionlint_extension_api::package::Identity;
    use actionlint_extension_api::package::ScriptPackage;
    use actionlint_extension_api::package::ScriptWorld;
    let descriptor = descriptor();
    let package = ScriptPackage {
        identity: Identity {
            id: &descriptor.id,
            name: &descriptor.name,
            description: "Hermetic Ruff engine for actionlint",
            license: "MIT",
            repository: "https://github.com/kjanat/actionlint-plugin-ruff",
            version: &descriptor.version,
        },
        descriptor: &descriptor,
        world: ScriptWorld::TemplateScriptEngine,
        script_inputs: &[],
        license_text: "",
    };
    Ok(serde_json::from_slice(&package.metadata()?)?)
}

#[cfg(target_arch = "wasm32")]
actionlint_extension_api::script_engine::v0_2::export_template_script_engine_v0_2!(Plugin);

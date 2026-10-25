use std::path::Path;

use ruff_linter::Locator;
use ruff_linter::directives;
use ruff_linter::linter::check_path;
use ruff_linter::settings::flags::Noqa;
use ruff_linter::source_kind::SourceKind;
use ruff_linter::suppression::Suppressions;
use ruff_python_ast::PySourceType;
use ruff_python_codegen::Stylist;
use ruff_python_index::Indexer;
use ruff_python_parser::ParseOptions;
use ruff_python_parser::parse_unchecked;
use ruff_ranged_value::ValueSource;
use ruff_ranged_value::ValueSourceGuard;
use ruff_source_file::PositionEncoding;
use ruff_source_file::SourceLocation;
use ruff_workspace::Settings;
use ruff_workspace::configuration::Configuration;
use ruff_workspace::options::Options;
use serde::Serialize;
use serde_json::Value;
use serde_json::json;

use crate::CheckRequest;

// These options discover files, expand environment variables, or imply CLI side effects.
// This is an adapter boundary, not a copy of Ruff's rule or Python-version catalog.
pub(crate) const UNSUPPORTED_OPTIONS: &[&str] = &[
    "extend",
    "cache-dir",
    "src",
    "namespace-packages",
    "fix",
    "fix-only",
    "unsafe-fixes",
    "show-fixes",
    "output-file",
];

pub(crate) fn settings(mut config: Value) -> Result<Settings, String> {
    if crate::ENGINE_VERSION != crate::template_effects::REVIEWED_ENGINE_VERSION {
        return Err("Ruff version has no reviewed template-effects adapter".into());
    }
    let object = config
        .as_object_mut()
        .ok_or("config must be a Ruff options object")?;
    for key in UNSUPPORTED_OPTIONS {
        if object.contains_key(*key) {
            return Err(format!(
                "Ruff option `{key}` is unavailable in the embedded, read-only engine"
            ));
        }
    }
    if object.get("target-version").is_none_or(Value::is_null) {
        object.insert(
            "target-version".into(),
            json!(crate::DEFAULT_TARGET_VERSION),
        );
    }
    let _guard = ValueSourceGuard::new(ValueSource::Cli, false);
    let options: Options =
        serde_json::from_value(config).map_err(|e| format!("invalid Ruff config: {e}"))?;
    // An absolute virtual root prevents cwd or filesystem discovery during settings resolution.
    let root = Path::new("/");
    let mut configuration = Configuration::from_options(options, None, root)
        .map_err(|e| format!("invalid Ruff config: {e}"))?;
    configuration.cache_dir = Some(root.join(".ruff_cache"));
    configuration.src = Some(Vec::new());
    configuration
        .into_settings(root)
        .map_err(|e| format!("invalid Ruff config: {e}"))
}

#[derive(Serialize)]
pub struct Position {
    pub row: usize,
    pub column: usize,
}

impl From<SourceLocation> for Position {
    fn from(value: SourceLocation) -> Self {
        Self {
            row: value.line.get(),
            column: value.character_offset.get(),
        }
    }
}

#[derive(Serialize)]
pub struct Diagnostic {
    pub template_effect: crate::template_effects::TemplateEffect,
    pub filename: String,
    pub code: String,
    pub name: String,
    pub severity: ruff_db::diagnostic::Severity,
    pub message: String,
    pub location: Position,
    pub end_location: Position,
    pub url: Option<String>,
}

pub(crate) fn check(request: CheckRequest) -> Result<Vec<Diagnostic>, String> {
    if request.filename.is_empty() || request.filename.contains('\0') {
        return Err("filename must be nonempty and contain no NUL bytes".into());
    }
    let settings = settings(request.config)?;
    let virtual_path = Path::new("/").join(&request.filename);
    let path = virtual_path.as_path();
    let source_type = if path.extension().is_some_and(|ext| ext == "pyi") {
        PySourceType::Stub
    } else {
        PySourceType::Python
    };
    let source_kind = SourceKind::Python {
        code: request.source.clone(),
        is_stub: source_type.is_stub(),
    };
    let target = settings.linter.resolve_target_version(path);
    let parsed = parse_unchecked(
        &request.source,
        ParseOptions::from(source_type).with_target_version(target.parser_version()),
    )
    .try_into_module()
    .expect("Python source types parse as modules");
    let locator = Locator::new(&request.source);
    let stylist = Stylist::from_tokens(parsed.tokens(), locator.contents());
    let indexer = Indexer::from_tokens(parsed.tokens(), locator.contents());
    let directives = directives::extract_directives(
        parsed.tokens(),
        directives::Flags::from_settings(&settings.linter),
        &locator,
        &indexer,
    );
    let suppressions = Suppressions::from_tokens(
        locator.contents(),
        parsed.tokens(),
        &indexer,
        &settings.linter,
    );
    let diagnostics = check_path(
        path,
        None,
        &locator,
        &stylist,
        &indexer,
        &directives,
        &settings.linter,
        Noqa::Disabled,
        &source_kind,
        source_type,
        &parsed,
        target,
        &suppressions,
    );
    let source = locator.to_source_code();
    let mut diagnostics: Vec<_> = diagnostics
        .into_iter()
        .map(|diagnostic| {
            let range = diagnostic.range().unwrap_or_default();
            Diagnostic {
                template_effect: crate::template_effects::classify(
                    diagnostic.secondary_code_or_id(),
                ),
                filename: request.filename.clone(),
                code: diagnostic.secondary_code_or_id().to_string(),
                name: diagnostic.id().as_str().to_string(),
                severity: diagnostic.severity(),
                message: diagnostic.concise_message().to_string(),
                location: source
                    .source_location(range.start(), PositionEncoding::Utf32)
                    .into(),
                end_location: source
                    .source_location(range.end(), PositionEncoding::Utf32)
                    .into(),
                url: diagnostic.documentation_url().map(ToOwned::to_owned),
            }
        })
        .collect();
    diagnostics.sort_by(|a, b| {
        (a.location.row, a.location.column, &a.code).cmp(&(
            b.location.row,
            b.location.column,
            &b.code,
        ))
    });
    Ok(diagnostics)
}

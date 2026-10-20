use actionlint_plugin_ruff_component::Plugin;
use actionlint_plugin_ruff_component::exports::kjanat::actionlint::script_engine::Guest;
use actionlint_plugin_ruff_component::exports::kjanat::actionlint::script_engine::Request;
use serde_json::json;

#[test]
fn typed_transport_preserves_unicode_diagnostics() {
    let outcome = Plugin::check(Request {
        source: "label = 'é🦀'; print(missing) # noqa\r\n".into(),
        filename: "step.py".into(),
        config: json!({"lint": {"select": ["F821"]}}).to_string(),
    });
    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    assert_eq!(outcome.diagnostics.len(), 1);
    let diagnostic = &outcome.diagnostics[0];
    assert_eq!(diagnostic.code, "F821");
    assert_eq!(diagnostic.filename, "step.py");
    assert_eq!(
        (diagnostic.location.row, diagnostic.location.column),
        (1, 21)
    );
    assert_eq!(
        (diagnostic.end_location.row, diagnostic.end_location.column),
        (1, 28)
    );
}

#[test]
fn typed_validation_and_checks_share_errors() {
    let config = json!({"lint": {"select": ["FAKE999"]}}).to_string();
    let validation = Plugin::validate_config(config.clone()).unwrap_err();
    let outcome = Plugin::check(Request {
        source: "print(missing)\n".into(),
        filename: "step.py".into(),
        config,
    });
    assert!(outcome.diagnostics.is_empty());
    assert_eq!(outcome.error, Some(validation));
    assert!(Plugin::validate_config("{".into()).is_err());
    assert!(Plugin::validate_config(String::new()).is_ok());
}

#[test]
fn metadata_and_descriptor_identify_the_same_engine() {
    let descriptor = Plugin::describe();
    let metadata = actionlint_plugin_ruff_component::metadata();
    assert_eq!(metadata["id"], descriptor.id);
    assert_eq!(metadata["plugin_version"], descriptor.version);
    assert_eq!(metadata["engine"]["name"], descriptor.engine);
    assert_eq!(metadata["engine"]["version"], descriptor.engine_version);
    assert_eq!(
        metadata["capabilities"]["script_shells"],
        json!(descriptor.shells)
    );
    assert_eq!(metadata["rules"][0]["name"], descriptor.rule.name);
    let schema: serde_json::Value = serde_json::from_str(&descriptor.rule.options_schema).unwrap();
    assert_eq!(metadata["config_schema"], schema);
    assert_eq!(metadata["rules"][0]["options_schema"], schema);
    assert!(metadata.get("abi_version").is_none());
}

use actionlint_plugin_ruff::check;
use actionlint_plugin_ruff::metadata;
use actionlint_plugin_ruff::validate_config;
use serde_json::Value;
use serde_json::json;

fn lint(source: &str, config: Value) -> Value {
    check(
        &serde_json::to_vec(
            &json!({"source": source, "filename": "workflow.step.py", "config": config}),
        )
        .unwrap(),
    )
}

fn codes(response: &Value) -> Vec<&str> {
    assert_eq!(response["ok"], true, "{response}");
    response["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["code"].as_str().unwrap())
        .collect()
}

#[test]
fn checks_ignore_noqa() {
    let result = lint(
        "# ruff: noqa\nimport os  # noqa: F401\nprint(missing)  # noqa\n",
        json!({"lint": {"select": ["F401", "F821"]}}),
    );
    assert_eq!(codes(&result), ["F401", "F821"]);
    assert_eq!(
        result["diagnostics"][1]["location"],
        json!({"row": 3, "column": 7})
    );
    assert_eq!(
        result["diagnostics"][1]["end_location"],
        json!({"row": 3, "column": 14})
    );
    assert_eq!(result["diagnostics"][1]["filename"], "workflow.step.py");
}

#[test]
fn empty_selection_still_reports_syntax_errors() {
    assert!(codes(&lint("import os\n", json!({"lint": {"select": []}}))).is_empty());
    let response = lint("if :\n", json!({"lint": {"select": []}}));
    let syntax_codes = codes(&response);
    assert!(!syntax_codes.is_empty());
    assert!(syntax_codes.iter().all(|code| *code == "invalid-syntax"));
}

#[test]
fn selection_and_ignore_use_ruff_resolution() {
    let result = lint(
        "import os\nprint(missing)\n",
        json!({"lint": {"select": ["F"], "ignore": ["F401"]}}),
    );
    assert_eq!(codes(&result), ["F821"]);
    assert_eq!(
        codes(&lint(
            "x = None\nif x == None: pass\n",
            json!({"lint": {"select": ["E711"]}})
        )),
        ["E711"]
    );
}

#[test]
fn unicode_columns_and_crlf_are_preserved() {
    let result = lint(
        "label = 'é🦀'; print(missing)\r\n",
        json!({"lint": {"select": ["F821"]}}),
    );
    assert_eq!(codes(&result), ["F821"]);
    assert_eq!(
        result["diagnostics"][0]["location"],
        json!({"row": 1, "column": 21})
    );
    assert_eq!(
        result["diagnostics"][0]["end_location"],
        json!({"row": 1, "column": 28})
    );
}

#[test]
fn lone_cr_uses_universal_newline_coordinates() {
    let result = lint(
        "print(1)\rprint(missing)\r",
        json!({"lint": {"select": ["F821"]}}),
    );
    assert_eq!(codes(&result), ["F821"]);
    assert_eq!(
        result["diagnostics"][0]["location"],
        json!({"row": 2, "column": 7})
    );
    assert_eq!(
        result["diagnostics"][0]["end_location"],
        json!({"row": 2, "column": 14})
    );
}

#[test]
fn modern_and_legacy_options_share_validation() {
    for config in [
        json!({"select": ["F821"]}),
        json!({"lint": {"select": ["F821"]}}),
    ] {
        assert_eq!(
            validate_config(&serde_json::to_vec(&config).unwrap())["ok"],
            true
        );
        assert_eq!(codes(&lint("print(missing)\n", config)), ["F821"]);
    }
    for config in [
        json!({"lint": {"select": ["FAKE999"]}}),
        json!({"target-version": "py999"}),
        json!({"typo": true}),
        json!({"lint": {"dummy-variable-rgx": "["}}),
        json!({"required-version": ">=99.0"}),
        json!({"select": "F"}),
        json!(null),
        json!([]),
    ] {
        let validation = validate_config(&serde_json::to_vec(&config).unwrap());
        assert_eq!(validation["ok"], false, "{config}: {validation}");
        assert_eq!(lint("pass\n", config)["ok"], false);
    }
}

#[test]
fn plugin_specific_ruff_options_are_available() {
    let config = json!({"lint": {"select": ["B"], "flake8-bugbear": {"extend-immutable-calls": ["custom"]}}});
    assert_eq!(
        validate_config(&serde_json::to_vec(&config).unwrap())["ok"],
        true
    );
}

#[test]
fn target_version_changes_python_analysis() {
    let source = "match value:\n    case _: pass\n";
    let old = lint(source, json!({"target-version": "py39", "select": []}));
    assert!(!codes(&old).is_empty());
    assert!(
        codes(&lint(
            source,
            json!({"target-version": "py310", "select": []})
        ))
        .is_empty()
    );
}

#[test]
fn per_file_ignores_and_versions_use_virtual_filename() {
    assert!(
        codes(&lint(
            "print(missing)\n",
            json!({"lint": {"per-file-ignores": {"*.step.py": ["F821"]}}})
        ))
        .is_empty()
    );
    assert!(codes(&lint("match x:\n    case _: pass\n", json!({"select": [], "target-version": "py39", "per-file-target-version": {"*.step.py": "py310"}}))).is_empty());
}

#[test]
fn filesystem_and_side_effect_options_are_rejected() {
    for config in [
        json!({"extend": "ruff.toml"}),
        json!({"src": []}),
        json!({"cache-dir": "$HOME/cache"}),
        json!({"fix": false}),
        json!({"namespace-packages": []}),
    ] {
        assert_eq!(
            validate_config(&serde_json::to_vec(&config).unwrap())["ok"],
            false
        );
    }
}

#[test]
fn requests_are_strict_and_bounded() {
    for input in [
        b"garbage".as_slice(),
        b"{}",
        br#"{"source":"pass","filename":"x.py","extra":true}"#,
        br#"{"source":"pass","filename":""}"#,
        br#"{"source":"pass","filename":"x\u0000.py"}"#,
    ] {
        assert_eq!(check(input)["ok"], false);
    }
    assert_eq!(
        check(&vec![b' '; actionlint_plugin_ruff::MAX_INPUT_BYTES + 1])["ok"],
        false
    );
}

#[test]
fn metadata_schema_comes_from_the_linked_engine() {
    let data = metadata();
    assert_eq!(data["engine"]["version"], ruff_linter::VERSION);
    assert_eq!(data["abi_version"], 1);
    assert_eq!(data["metadata_version"], 1);
    assert_eq!(data["transport"], "wasm-core-json");
    assert_eq!(data["id"], "actionlint-plugin-ruff");
    assert_eq!(data["homepage"], env!("CARGO_PKG_HOMEPAGE"));
    assert_eq!(data["languages"], json!(["python"]));
    assert_eq!(data["shells"], json!(["python"]));
    assert_eq!(data["file_extensions"], json!(["py", "pyi"]));
    assert_eq!(data["conflicts"], json!([]));
    assert_eq!(data["interfaces"]["check"][0]["version"], 2);
    assert_eq!(data["interfaces"]["check"][1]["version"], 1);
    assert_eq!(
        data["template_contract"],
        actionlint_plugin_ruff::template_effects::CONTRACT
    );
    assert_eq!(data["interfaces"]["check"][0]["export"], "check");
    assert_eq!(data["capabilities"]["format"], false);
    assert_eq!(data["capabilities"]["fixes"], false);
    assert!(data["interfaces"].get("format").is_none());
    assert!(data["interfaces"].get("fix").is_none());
    assert_eq!(data["config_schema"]["additionalProperties"], false);
    assert!(data["config_schema"]["properties"].get("lint").is_some());
    assert!(data["config_schema"]["properties"].get("extend").is_none());
    assert_eq!(
        data["config_schema"]["properties"]["target-version"]["default"],
        "py314"
    );
    let upstream =
        serde_json::to_value(schemars::schema_for!(ruff_workspace::options::Options)).unwrap();
    assert_eq!(
        data["config_schema"]["properties"]["select"],
        upstream["properties"]["select"]
    );
    assert_eq!(
        data["config_schema"]["$defs"]["LintOptions"]["properties"]["select"],
        upstream["$defs"]["LintOptions"]["properties"]["select"]
    );
}

#[test]
fn default_selection_comes_from_ruff() {
    let selectors: Vec<_> = ruff_linter::settings::DEFAULT_SELECTORS
        .iter()
        .map(|selector| {
            let (prefix, code) = selector.prefix_and_code();
            format!("{prefix}{code}")
        })
        .collect();
    let source = "import os\nvalue = None\nif value == None: pass\nprint(missing)\n";
    let explicit = lint(source, json!({"lint": {"select": selectors}}));
    for config in [
        json!({}),
        json!({"select": null}),
        json!({"lint": {"select": null}}),
    ] {
        assert_eq!(lint(source, config), explicit);
    }
    assert_ne!(lint(source, json!({"select": ["F"]})), explicit);
}

#[test]
fn calls_do_not_share_configuration_or_suppressions() {
    for _ in 0..3 {
        assert!(codes(&lint("print(missing)\n", json!({"select": []}))).is_empty());
        assert_eq!(
            codes(&lint("print(missing)  # noqa\n", json!({}))),
            ["F821"]
        );
    }
}

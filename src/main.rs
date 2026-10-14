use std::io::{self, Read};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let operation = std::env::args().nth(1).unwrap_or_else(|| "check".into());
    let response = match operation.as_str() {
        "metadata" => actionlint_plugin_ruff::metadata(),
        "schema" => actionlint_plugin_ruff::metadata()["config_schema"].clone(),
        "check" | "validate-config" => {
            let mut input = Vec::new();
            io::stdin()
                .take(actionlint_plugin_ruff::MAX_INPUT_BYTES as u64 + 1)
                .read_to_end(&mut input)?;
            if operation == "check" {
                actionlint_plugin_ruff::check(&input)
            } else {
                actionlint_plugin_ruff::validate_config(&input)
            }
        }
        _ => {
            return Err(
                "usage: actionlint-plugin-ruff [check|validate-config|metadata|schema]".into(),
            );
        }
    };
    serde_json::to_writer(io::stdout().lock(), &response)?;
    println!();
    Ok(())
}

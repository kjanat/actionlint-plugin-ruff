fn main() -> Result<(), Box<dyn std::error::Error>> {
    serde_json::to_writer_pretty(
        std::io::stdout().lock(),
        &actionlint_plugin_ruff_component::metadata(),
    )?;
    println!();
    Ok(())
}

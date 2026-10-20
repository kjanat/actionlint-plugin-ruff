use std::io::Write;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let input = args.next().ok_or("usage: componentize INPUT.wasm")?;
    if args.next().is_some() {
        return Err("usage: componentize INPUT.wasm".into());
    }
    let module = std::fs::read(input)?;
    let component = wit_component::ComponentEncoder::default()
        .module(&module)?
        .validate(true)
        .encode()?;
    std::io::stdout().lock().write_all(&component)?;
    Ok(())
}

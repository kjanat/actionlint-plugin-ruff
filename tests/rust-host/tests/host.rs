use serde_json::Value;
use serde_json::json;
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use wasmi::Config;
use wasmi::Engine;
use wasmi::ExternType;
use wasmi::Instance;
use wasmi::Linker;
use wasmi::Memory;
use wasmi::Module;
use wasmi::Store;
use wasmi::StoreLimits;
use wasmi::StoreLimitsBuilder;

type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;

fn asset(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../artifacts")
        .join(name)
}

// Deliberately reads only the first custom section, without loading a runtime or code.
fn static_metadata(wasm: &[u8]) -> Result<&[u8]> {
    fn u32_leb(bytes: &[u8], offset: &mut usize) -> Result<u32> {
        let mut value = 0;
        for i in 0..5 {
            let byte = *bytes.get(*offset).ok_or("truncated section length")?;
            *offset += 1;
            if i == 4 && byte & 0xf0 != 0 {
                return Err("section length overflows u32".into());
            }
            value |= u32::from(byte & 0x7f) << (7 * i);
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err("invalid section length".into())
    }
    if wasm.get(..9) != Some(&[0, 97, 115, 109, 1, 0, 0, 0, 0]) {
        return Err("expected core WASM header followed by custom section".into());
    }
    let mut offset = 9;
    let size = u32_leb(wasm, &mut offset)? as usize;
    if size > 1024 * 1024 + 64 {
        return Err("metadata section exceeds limit".into());
    }
    let end = offset.checked_add(size).ok_or("section offset overflow")?;
    let section = wasm.get(..end).ok_or("truncated custom section")?;
    let name_size = u32_leb(section, &mut offset)? as usize;
    let name_end = offset
        .checked_add(name_size)
        .ok_or("section name offset overflow")?;
    if section.get(offset..name_end) != Some(b"actionlint.plugin".as_slice()) {
        return Err("unexpected custom section".into());
    }
    Ok(&section[name_end..])
}

struct Host {
    store: Store<StoreLimits>,
    instance: Instance,
    memory: Memory,
}

fn v1_export<'a>(metadata: &'a Value, operation: &str) -> Result<&'a str> {
    if metadata["abi_version"] != 1
        || metadata
            .get("metadata_version")
            .is_some_and(|version| version != 1)
        || metadata
            .get("transport")
            .is_some_and(|transport| transport != "wasm-core-json")
    {
        return Err("unsupported discovery format or transport".into());
    }
    if metadata.get("interfaces").is_none() && metadata.get("metadata_version").is_none() {
        return match operation {
            "check" => Ok("check"),
            "validate_config" => Ok("validate_config"),
            "metadata" => Ok("metadata"),
            _ => Err("unsupported v1 operation".into()),
        };
    }
    metadata["interfaces"][operation]
        .as_array()
        .and_then(|versions| versions.iter().find(|entry| entry["version"] == 1))
        .and_then(|entry| entry["export"].as_str())
        .ok_or_else(|| "unsupported v1 operation".into())
}

impl Host {
    fn new(wasm: &[u8]) -> Result<Self> {
        let discovery: Value = serde_json::from_slice(static_metadata(wasm)?)?;
        for operation in ["check", "validate_config", "metadata"] {
            if v1_export(&discovery, operation)? != operation {
                return Err("unexpected frozen v1 export name".into());
            }
        }
        let mut config = Config::default();
        config.consume_fuel(true);
        let engine = Engine::new(&config);
        let module = Module::new(&engine, wasm)?;
        assert_eq!(
            module.imports().count(),
            0,
            "no Go, JS, WASI, or native host imports"
        );
        let fixture: Value = serde_json::from_str(include_str!("../../fixtures/abi-v1.json"))?;
        for (name, signature) in fixture["exports"]
            .as_object()
            .ok_or("missing ABI fixture")?
        {
            let export = module
                .exports()
                .find(|export| export.name() == name)
                .ok_or("missing frozen v1 export")?;
            let ExternType::Func(function) = export.ty() else {
                return Err("expected function export".into());
            };
            let names = |types: &[wasmi::ValType]| {
                types
                    .iter()
                    .map(|t| format!("{t:?}").to_lowercase())
                    .collect::<Vec<_>>()
            };
            assert_eq!(
                json!(names(function.params())),
                signature["parameters"],
                "{name} parameters"
            );
            assert_eq!(
                json!(names(function.results())),
                signature["results"],
                "{name} results"
            );
        }
        let limits = StoreLimitsBuilder::new()
            .memory_size(256 * 1024 * 1024)
            .build();
        let mut store = Store::new(&engine, limits);
        store.limiter(|limits| limits);
        store.set_fuel(1_000_000_000)?;
        let instance = Linker::new(&engine).instantiate_and_start(&mut store, &module)?;
        let version = instance
            .get_typed_func::<(), i32>(&store, "abi_version")?
            .call(&mut store, ())?;
        assert_eq!(
            version,
            fixture["abi_version"]
                .as_i64()
                .ok_or("missing ABI version")? as i32
        );
        let memory = instance
            .get_memory(&store, "memory")
            .ok_or("missing memory")?;
        Ok(Self {
            store,
            instance,
            memory,
        })
    }

    fn free(&mut self, pointer: u32) -> Result {
        self.instance
            .get_typed_func::<i32, ()>(&self.store, "dealloc")?
            .call(&mut self.store, pointer as i32)?;
        Ok(())
    }

    fn response(&mut self, packed: i64) -> Result<Value> {
        // Preserve all bits, even when the signed WASM result is negative.
        let packed = packed as u64;
        let pointer = (packed >> 32) as u32;
        let length = packed as u32;
        let start = pointer as usize;
        let bytes = start
            .checked_add(length as usize)
            .and_then(|end| self.memory.data(&self.store).get(start..end))
            .map(<[u8]>::to_vec)
            .ok_or("response pointer/length outside guest memory");
        self.free(pointer)?;
        Ok(serde_json::from_slice(&bytes?)?)
    }

    fn metadata(&mut self) -> Result<Value> {
        let packed = self
            .instance
            .get_typed_func::<(), i64>(&self.store, "metadata")?
            .call(&mut self.store, ())?;
        self.response(packed)
    }

    fn invoke(&mut self, operation: &str, input: &[u8]) -> Result<Value> {
        let length = i32::try_from(input.len())?;
        let pointer = self
            .instance
            .get_typed_func::<i32, i32>(&self.store, "alloc")?
            .call(&mut self.store, length)?;
        if pointer == 0 {
            return Err("input allocation rejected".into());
        }
        let result = (|| -> Result<Value> {
            self.memory
                .write(&mut self.store, pointer as u32 as usize, input)?;
            let packed = self
                .instance
                .get_typed_func::<(i32, i32), i64>(&self.store, operation)?
                .call(&mut self.store, (pointer, length))?;
            self.response(packed)
        })();
        self.free(pointer as u32)?;
        result
    }
}

#[test]
fn rust_reads_discovery_without_compiling_or_instantiating() -> Result {
    let wasm = fs::read(asset("plugin.wasm"))?;
    let bytes = static_metadata(&wasm)?;
    assert_eq!(bytes, fs::read(asset("metadata.json"))?);
    let end = bytes.as_ptr() as usize - wasm.as_ptr() as usize + bytes.len();
    assert_eq!(static_metadata(&wasm[..end])?, bytes);
    let metadata: Value = serde_json::from_slice(bytes)?;
    assert_eq!(metadata["id"], "actionlint-plugin-ruff");
    assert_eq!(metadata["transport"], "wasm-core-json");
    assert_eq!(v1_export(&metadata, "check")?, "check");
    Ok(())
}

#[test]
fn rust_negotiates_v1_with_optional_extensions_and_other_versions() -> Result {
    let mut metadata = json!({
        "abi_version": 1, "metadata_version": 1, "transport": "wasm-core-json",
        "future_metadata": {"unknown": true},
        "interfaces": {
            "check": [{"version": 2, "export": "check_v2"}, {"version": 1, "export": "check"}],
            "format": [{"version": 2, "export": "format_v2"}],
            "future_operation": [{"version": 9, "export": "future_v9"}]
        }
    });
    assert_eq!(v1_export(&metadata, "check")?, "check");
    assert!(v1_export(&metadata, "format").is_err());
    metadata["interfaces"]["check"] = json!([{"version": 2, "export": "check_v2"}]);
    assert!(v1_export(&metadata, "check").is_err());
    let legacy = json!({"abi_version": 1});
    assert_eq!(v1_export(&legacy, "check")?, "check");
    assert!(v1_export(&legacy, "fix").is_err());
    metadata["abi_version"] = json!(2);
    assert!(v1_export(&metadata, "check").is_err());
    Ok(())
}

#[test]
fn rust_executes_the_same_v1_artifact_as_go_and_typescript() -> Result {
    let wasm = fs::read(asset("plugin.wasm"))?;
    let metadata: Value = serde_json::from_slice(static_metadata(&wasm)?)?;
    let mut host = Host::new(&wasm)?;
    assert_eq!(host.metadata()?, metadata);
    assert_eq!(
        host.invoke("validate_config", b"{}")?,
        json!({"ok": true, "errors": []})
    );
    let bad_config = host.invoke("validate_config", br#"{"select":["FAKE999"]}"#)?;
    assert_eq!(bad_config["ok"], false);
    assert!(bad_config["errors"].is_array());
    assert!(bad_config.get("diagnostics").is_none());
    let request = serde_json::to_vec(&json!({
        "source": "label = 'é🦀'; print(missing) # noqa\r\n", "filename": "workflow.step.py",
        "config": {"lint": {"select": ["F821"]}}
    }))?;
    for _ in 0..3 {
        let response = host.invoke("check", &request)?;
        assert_eq!(response["ok"], true);
        assert_eq!(
            response["diagnostics"]
                .as_array()
                .ok_or("missing diagnostics")?
                .len(),
            1
        );
        assert_eq!(response["diagnostics"][0]["code"], "F821");
        assert_eq!(
            response["diagnostics"][0]["location"],
            json!({"row":1,"column":21})
        );
        assert_eq!(
            response["diagnostics"][0]["end_location"],
            json!({"row":1,"column":28})
        );
    }
    assert_eq!(host.invoke("check", b"not json")?["ok"], false);
    // Runtime limits are a host policy, independent of the wire contract.
    host.store.set_fuel(0)?;
    let error = host
        .instance
        .get_typed_func::<(), i64>(&host.store, "metadata")?
        .call(&mut host.store, ())
        .expect_err("zero fuel must interrupt execution");
    assert_eq!(error.as_trap_code(), Some(wasmi::TrapCode::OutOfFuel));
    Ok(())
}

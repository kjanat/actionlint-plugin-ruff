//! Owned allocation handles avoid trusting raw pointers supplied by the host.
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::Once;

thread_local! {
    static BUFFERS: RefCell<BTreeMap<u32, Vec<u8>>> = const { RefCell::new(BTreeMap::new()) };
}

fn initialize() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        unsafe extern "C" {
            fn __wasm_call_ctors();
        }
        // SAFETY: call linked inventory constructors exactly once before Ruff is used.
        unsafe {
            __wasm_call_ctors();
        }
    });
}

fn store(bytes: Vec<u8>) -> u64 {
    let pointer = bytes.as_ptr() as u32;
    let length = bytes.len() as u32;
    BUFFERS.with(|buffers| buffers.borrow_mut().insert(pointer, bytes));
    (u64::from(pointer) << 32) | u64::from(length)
}

#[unsafe(no_mangle)]
pub extern "C" fn alloc(length: u32) -> u32 {
    if length == 0 || length as usize > crate::MAX_INPUT_BYTES {
        return 0;
    }
    (store(vec![0; length as usize]) >> 32) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn dealloc(pointer: u32) {
    BUFFERS.with(|buffers| buffers.borrow_mut().remove(&pointer));
}

fn invoke(pointer: u32, length: u32, function: fn(&[u8]) -> serde_json::Value) -> u64 {
    initialize();
    let response = BUFFERS.with(|buffers| {
        let buffers = buffers.borrow();
        match buffers
            .get(&pointer)
            .and_then(|bytes| bytes.get(..length as usize))
        {
            Some(bytes) => function(bytes),
            None => crate::failure("invalid input allocation or length"),
        }
    });
    store(serde_json::to_vec(&response).expect("response is JSON serializable"))
}

#[unsafe(no_mangle)]
pub extern "C" fn abi_version() -> u32 {
    crate::ABI_VERSION
}

#[unsafe(no_mangle)]
pub extern "C" fn check(pointer: u32, length: u32) -> u64 {
    invoke(pointer, length, crate::check)
}

#[unsafe(no_mangle)]
pub extern "C" fn validate_config(pointer: u32, length: u32) -> u64 {
    invoke(pointer, length, crate::validate_config)
}

#[unsafe(no_mangle)]
pub extern "C" fn metadata() -> u64 {
    initialize();
    store(serde_json::to_vec(&crate::metadata()).expect("metadata is JSON serializable"))
}

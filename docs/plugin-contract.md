# Embedded Ruff WASM contract

This is the implemented contract for `actionlint-plugin-ruff`. It replaces the earlier stdin/stdout checker design.
Actionlint owns shell selection, expression sanitization, source mapping, suppression, scheduling, and cancellation.
Ruff owns Python analysis and configuration validation. The plugin receives one already-sanitized snippet per call.

## Core module ABI, version 1

The packaged module is `artifacts/plugin.wasm`, targeting `wasm32-unknown-unknown`. It requires no host imports or WASI.
Requests and responses are UTF-8 JSON bytes in linear memory.

For embedding in a Go executable, use the reproducible `artifacts/plugin.wasm.gz` asset and decompress it once with
`compress/gzip` before passing the bytes to the runtime. It restores the exact same core module and preserves this ABI.

| Export            | Signature                            | Meaning                                                                        |
| ----------------- | ------------------------------------ | ------------------------------------------------------------------------------ |
| `memory`          | linear memory                        | Input and output UTF-8 JSON bytes                                              |
| `abi_version`     | `() -> i32`                          | Returns `1`                                                                    |
| `alloc`           | `(length: i32) -> i32`               | Returns a zeroed input buffer pointer, or `0` if length is zero or above 8 MiB |
| `dealloc`         | `(pointer: i32) -> ()`               | Releases an owned buffer; unknown pointers are ignored                         |
| `check`           | `(pointer: i32, length: i32) -> i64` | Checks a JSON snippet request                                                  |
| `validate_config` | `(pointer: i32, length: i32) -> i64` | Validates a JSON Ruff options object                                           |
| `metadata`        | `() -> i64`                          | Returns engine version, capabilities, and generated config schema              |

An `i64` result packs the output pointer in the high 32 bits and byte length in the low 32 bits:

```go
pointer := uint32(result >> 32)
length := uint32(result)
```

The host treats pointers, lengths, and packed results as unsigned bit patterns. It must:

1. Verify `abi_version() == 1`.
2. Encode the request, call `alloc(len)`, and write exactly that many bytes at the returned pointer.
3. Call the operation, unpack the result, and read its JSON bytes.
4. Call `dealloc` for both the input and output pointers, even when the operation reports an error.

Calls can grow memory. Obtain fresh views after every guest call and copy response bytes before releasing them.
Inputs are borrowed during a call, not consumed. Passing an unknown pointer or a length beyond its owned allocation
returns a structured error without dereferencing arbitrary host-provided addresses.

One instance supports many calls. Serialize calls within an instance; use separate instances for parallel checks.
Configuration is resolved anew per check and never leaks between calls. Initialization of Ruff's inventory constructors
happens once inside the plugin; the host needs no initialization export.

The host supplies memory and execution limits and terminates/discards an instance on cancellation or a WASM trap.
Panics abort/trap instead of becoming JSON errors. The 8 MiB limit applies to request bytes, not total guest memory or
response size; metadata contains the full schema. Buffer ownership does not replace host memory limits.

## Requests and responses

`check` accepts exactly these fields; `config` may be omitted:

```json
{
  "source": "print(missing)\n",
  "filename": "workflow.step.py",
  "config": { "lint": { "extend-select": ["B", "UP"] }, "target-version": "py314" }
}
```

`filename` must be nonempty and contain no NUL bytes. Relative names resolve against a virtual `/` root for Ruff's
path matching. The diagnostic retains the original filename. `.pyi` selects stub mode; all other extensions use Python
module parsing. No file is read.

Successful checks return:

```json
{
  "ok": true,
  "diagnostics": [
    {
      "filename": "workflow.step.py",
      "code": "F821",
      "name": "undefined-name",
      "severity": "error",
      "message": "Undefined name `missing`",
      "location": { "row": 1, "column": 7 },
      "end_location": { "row": 1, "column": 14 },
      "url": "https://docs.astral.sh/ruff/rules/undefined-name/"
    }
  ]
}
```

Rows and columns are one-based Unicode character offsets. End positions are exclusive. CRLF is preserved. `code`
prefers Ruff's short rule code and falls back to the diagnostic name, including `invalid-syntax`. `severity` preserves
Ruff's `info`, `warning`, `error`, or `fatal` value. `url` can be null. Diagnostics sort by row, column, and code.
There are no fix edits. Field names and coordinates match actionlint's existing Ruff diagnostic decoder.

`validate_config` accepts the Ruff options object itself, not a snippet or a `config` envelope:

```json
{ "lint": { "extend-select": ["B", "UP"], "ignore": ["F401"] }, "target-version": "py312" }
```

Success returns `{ "ok": true, "errors": [] }`. Both operations use the same Ruff settings conversion. Malformed JSON,
unknown fields, invalid options or selectors, unsupported embedded options, and invalid allocations return:

```json
{ "ok": false, "errors": ["invalid Ruff config: ..."] }
```

Errors have no `diagnostics` field. Findings and Python syntax errors are successful analysis, so `ok` remains true.
The host handles `enabled` and passes only Ruff options. It must not validate selectors or targets against a second
catalog.

## Metadata and schema

`metadata()` returns:

```json
{
  "abi_version": 1,
  "plugin_version": "0.1.0",
  "engine": { "name": "ruff", "version": "0.17.0" },
  "capabilities": {
    "check": true,
    "validate_config": true,
    "config_schema": true,
    "noqa": false,
    "filesystem": false,
    "fixes": false,
    "position_encoding": "unicode",
    "max_input_bytes": 8388608
  },
  "config_schema": {}
}
```

The actual `config_schema` is generated by `schemars` from the linked Ruff `Options` type, with unsupported embedded
options removed and plugin defaults annotated. Embed it as a complete schema document, preserving its `$defs` and
references. JSON Schema cannot express every Ruff semantic error; `validate_config` is authoritative.

The build obtains `schema.json` by calling `metadata` on the final module, keeping editor completion and the engine on
the same version without a schema snapshot in the source tree. Upstream references are
[Ruff's options](https://github.com/astral-sh/ruff/blob/5b27c1ccfaa230fb195855f5cf3404a21e3379cd/crates/ruff_workspace/src/options.rs)
and its
[WASM checker](https://github.com/astral-sh/ruff/blob/5b27c1ccfaa230fb195855f5cf3404a21e3379cd/crates/ruff_wasm/src/lib.rs).

## Host responsibilities

The Go host can compile the embedded module once and pool instances. It forwards sanitized Python and opaque Ruff
options,
unwraps the response, then applies expression-span filtering and actionlint's YAML source mapping. The plugin does not
interpret `${{ }}`, workflow YAML, shell strings, or actionlint suppressions. The host supplies virtual filenames and
interpreter-derived targets. Package discovery is disabled, so checks requiring package roots lack repository context.

The native utility is for development and consumes the same JSON requests. It does not implement the previous generic
workflow/snippet-batch process protocol. The current actionlint checkout still uses its executable Ruff adapter until
the
Go integration is implemented.

# Ruff plugin contract

This document describes the WebAssembly interfaces exposed by `actionlint-plugin-ruff`. The host selects scripts,
replaces workflow expressions, maps locations back to YAML, applies suppressions, and controls execution. The plugin
checks one sanitized Python snippet at a time and validates Ruff options.

## Component interface

New hosts should load `artifacts/plugin.component.wasm` or its gzip copy. The component implements
`kjanat:actionlint/script-engine@0.2.0`. Its
[SDK WIT definition](https://github.com/kjanat/actionlint/blob/ab420b66fb98af9d993c176e97506ecec9d3a396/plugins/sdk/rust/wit/script-engine.wit)
defines these operations:

- `describe()` returns the plugin and engine identity, supported shells, and one rule descriptor.
- `validate-config()` accepts a JSON Ruff options object and returns a typed result.
- `check()` accepts source, a virtual filename, and JSON configuration. It returns typed diagnostics and may also
  return an incomplete-analysis error.

Coordinates count Unicode scalar values from one. End positions are exclusive. The component passes diagnostic
fields as typed values rather than JSON blobs. It exports a script engine, not a workflow analyzer.

The outer `actionlint.plugin` section contains `component.metadata.json`. It specifies `transport: "wasm-component"`,
the interface, and the SHA-256 digest of the LF-normalized WIT. It also contains the identity, engine version, rules,
and options schema returned by `describe()`. `SHA256SUMS` includes digests for the raw and gzip artifacts. The host
checks metadata against the component's exports and descriptor. The WIT digest identifies the source used to build the
component. Hosts check interface compatibility structurally; independent implementations do not need identical WIT
formatting or comments.

The guest targets `wasm32-unknown-unknown`. The Cargo-locked `wit-component` encoder produces the component. The guest
imports no filesystem, credentials, clock, or network services. A clock request traps. Public-loader tests exercise
both packaged forms, configuration errors, Unicode and CRLF coordinates, expression masking, and YAML source mapping.
They use the public host API and vary the provider identity and diagnostic text. Guest builds do not need a sibling
checkout. The Cargo SDK dependency and both Go integration modules pin actionlint revision
`ab420b66fb98af9d993c176e97506ecec9d3a396`. The vendored WIT supplies only a digest test fixture.

## Template effects

The optional `kjanat:actionlint/template-effects@0.2.0` component interface reports how expression substitution affects
each diagnostic. Its `contract()` returns `kjanat:actionlint/template-effects@0.1.0`. Its request and diagnostic types
use the same sanitized-source and coordinate rules as script-engine. The interfaces have independent versions.

Core JSON `check` version 2 adds `template_effect` and requires the corresponding `template_contract` discovery field.
The original script-engine interface, core `check` version 1, and memory ABI version 1 remain available.

Effects identify affected line length, references, generated names or parentheses, string contents, docstring
summaries, exported names, and percent-conversion slots. `none` tells the host to keep the finding. For an export
finding, the effect identifies the exact exported string range. Hosts combine effects with recorded substitution ranges
to filter findings. They must not classify findings by diagnostic code or message text. Percent-conversion masks use a
valid `s` conversion, so hosts do not need to inspect messages for synthetic unsupported-conversion findings.

The mapping has been reviewed for Ruff 0.17.4. Review the mapping and range fixtures when changing Ruff versions. The
adapter rejects an engine version that has not been reviewed. The engine and schema define the Python default; hosts
only pass a target when configuration or the interpreter supplies one.

## Core module ABI, version 1

`artifacts/plugin.wasm` is a core WebAssembly module for `wasm32-unknown-unknown`. It needs no host imports or WASI. Its
requests and responses are UTF-8 JSON in guest linear memory. A host can embed `plugin.wasm.gz`, decompress it once,
and compile the resulting bytes. The decompressed bytes match `plugin.wasm`. Go hosts can use `compress/gzip`.

| Export            | Signature                            | Meaning                                                                        |
| ----------------- | ------------------------------------ | ------------------------------------------------------------------------------ |
| `memory`          | linear memory                        | Input and output UTF-8 JSON bytes                                              |
| `abi_version`     | `() -> i32`                          | Returns `1`                                                                    |
| `alloc`           | `(length: i32) -> i32`               | Returns a zeroed input buffer pointer, or `0` if length is zero or above 8 MiB |
| `dealloc`         | `(pointer: i32) -> ()`               | Releases an owned buffer and ignores unknown pointers                          |
| `check`           | `(pointer: i32, length: i32) -> i64` | Checks a JSON snippet request                                                  |
| `validate_config` | `(pointer: i32, length: i32) -> i64` | Validates a JSON Ruff options object                                           |
| `metadata`        | `() -> i64`                          | Returns engine version, capabilities, and generated config schema              |

The high 32 bits of an `i64` result hold the output pointer. The low 32 bits hold the byte length. For example:

```go
pointer := uint32(result >> 32)
length := uint32(result)
```

In Rust, preserve the signed result's bits before unpacking:

```rust
let bits = result as u64;
let pointer = (bits >> 32) as u32;
let length = bits as u32;
```

Treat pointers, lengths, and results as unsigned bit patterns. For each request:

1. Check that `abi_version()` returns `1`.
2. Encode the request, allocate its byte length with `alloc`, and write the bytes at the returned pointer.
3. Call the operation, unpack its result, and copy the response bytes.
4. Call `dealloc` on both buffers, including when the response reports an error.

Guest calls can grow memory, so obtain a fresh memory view after each call. Inputs remain owned by the caller and are
borrowed for the call. Unknown pointers and lengths beyond an allocation produce structured errors without reading
arbitrary addresses. Calls within an instance must be sequential; use multiple instances for parallel checks. The
plugin resolves options for every check and initializes Ruff's inventory constructors once. There is no host
initialization call.

The host sets memory and execution limits and discards an instance after cancellation or a trap. Panics abort or trap;
they do not become JSON errors. The 8 MiB input limit does not limit total guest memory or response size. Metadata
includes the full schema, so the host must set a separate memory limit.

## Core requests and responses

`check` accepts `source`, `filename`, and optional `config`:

```json
{
  "source": "print(missing)\n",
  "filename": "workflow.step.py",
  "config": { "lint": { "extend-select": ["B", "UP"] }, "target-version": "py314" }
}
```

The filename must be nonempty and contain no NUL byte. Ruff resolves relative names against a virtual `/` root when
matching paths. The diagnostic retains the supplied filename. A `.pyi` extension selects stub parsing; other extensions
select Python module parsing. The engine does not open the file.

A successful check can return diagnostics:

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

Rows and columns count Unicode scalar values from one, and end positions are exclusive. The engine preserves CRLF.
`code` contains Ruff's short rule code when available, otherwise the diagnostic name such as `invalid-syntax`.
`severity` preserves Ruff's `info`, `warning`, `error`, and `fatal` values and maps `hint` to `info`. `url` may be null.
Diagnostics sort by row, column, then code. The response contains no fix edits. The fields and coordinates match
actionlint's Ruff diagnostic decoder.

`validate_config` takes the options object itself:

```json
{ "lint": { "extend-select": ["B", "UP"], "ignore": ["F401"] }, "target-version": "py312" }
```

It returns `{ "ok": true, "errors": [] }` on success. Both operations resolve settings through the same Ruff code.
Malformed JSON, unknown fields, invalid options or selectors, unsupported embedded options, and invalid allocations
return an error object:

```json
{ "ok": false, "errors": ["invalid Ruff config: ..."] }
```

Error objects omit `diagnostics`. Findings and syntax errors are valid analysis results and retain `ok: true`. The host
handles actionlint's `enabled` setting and sends only Ruff options. Do not maintain a second selector or Python target
catalog in the host.

## Metadata and schema

The core `metadata()` export returns an object with this structure. Version numbers shown here are examples:

```json
{
  "metadata_version": 1,
  "abi_version": 1,
  "transport": "wasm-core-json",
  "id": "actionlint-plugin-ruff",
  "name": "Ruff",
  "description": "Run Ruff on actionlint Python snippets through WebAssembly",
  "license": "MIT",
  "homepage": "https://github.com/kjanat/actionlint-plugin-ruff",
  "repository": "https://github.com/kjanat/actionlint-plugin-ruff",
  "documentation_url": "https://github.com/kjanat/actionlint-plugin-ruff/blob/master/docs/plugin-contract.md",
  "update_url": "https://api.github.com/repos/kjanat/actionlint-plugin-ruff/releases",
  "update_format": "github-releases-v1",
  "languages": ["python"],
  "shells": ["python"],
  "file_extensions": ["py", "pyi"],
  "conflicts": [],
  "plugin_version": "0.1.0",
  "engine": { "name": "ruff", "version": "0.17.0" },
  "interfaces": {
    "check": [{ "version": 1, "export": "check", "exclusive_group": "python/ruff/check" }],
    "validate_config": [{ "version": 1, "export": "validate_config" }],
    "metadata": [{ "version": 1, "export": "metadata" }]
  },
  "capabilities": {
    "check": true,
    "validate_config": true,
    "config_schema": true,
    "noqa": false,
    "filesystem": false,
    "fixes": false,
    "format": false,
    "position_encoding": "unicode",
    "max_input_bytes": 8388608
  },
  "config_schema": {}
}
```

`schemars` generates the schema from the linked Ruff `Options` type. The plugin removes options that cannot work in the
embedded engine and annotates its defaults. Retain the complete schema, including `$defs` and references. The schema
cannot represent every Ruff validation rule; call `validate_config` for the final result.

### Discovery and plugin selection

`id` identifies the plugin across releases; `plugin_version` identifies a release. The project URLs refer to this
wrapper and `engine` identifies Ruff. An `update_format` of `github-releases-v1` means `update_url` points to GitHub's
releases API. It is neither an artifact URL nor an instruction to update automatically. If the feed is empty, no update
is available. Before replacing a plugin, an updater must select an eligible release, obtain its artifacts and checksums,
and check compatibility. Plugin and Ruff version numbers do not specify supported interfaces.

`languages` contains editor language identifiers and `shells` contains canonical actionlint shell identifiers. The host
parses commands such as `python3.12 -u {0}`, selects Python snippets, and sanitizes expressions. The plugin does not
parse shell commands. `file_extensions` lists lowercase extensions without dots; `.pyi` selects stub parsing. These
fields help select a plugin and do not prohibit other virtual filenames for sanitized Python.

`conflicts` lists plugin IDs that cannot be enabled together. An empty list declares no cross-plugin conflict. Two
artifacts with the same plugin ID are versions of one plugin. An operation's `exclusive_group` identifies
interchangeable
providers. For each matching snippet, the host selects one provider for that operation, avoiding duplicate diagnostics.
Language overlap alone does not declare a conflict. Operation groups are independent, so a formatter and checker can
coexist. The host makes selections and applies overrides; metadata does not disable another plugin by itself.

### Interface compatibility and evolution

Three independently versioned fields define compatibility:

| Field                              | Contract                                                                               |
| ---------------------------------- | -------------------------------------------------------------------------------------- |
| `metadata_version`                 | Discovery format. An absent field identifies the original v1 format.                   |
| `abi_version`                      | Core memory layout, allocation ownership, calling convention, and import requirements. |
| `interfaces.<operation>[].version` | An operation's JSON requests, responses, coordinates, and semantics.                   |

Each version is an exact positive integer, not a minimum host version. A host selects an operation version it
understands
and calls its advertised `export`. An absent or unsupported optional operation disables that feature. An absent required
operation prevents activation. Ignore unknown optional fields and operation names. The `resolveInterface` and
`requireInterface` helpers implement discovery without loading a runtime. The development host checks versions before
checking a snippet. Artifacts without `metadata_version` or `interfaces` expose only the original v1 `check`,
`validate_config`, and `metadata` exports.

Keep advertised v1 contracts usable. New optional response and discovery fields must preserve existing fields and their
meaning. The v1 `check` request rejects unknown fields, so hosts cannot send new request fields merely because a plugin
is newer. Changes to request shape, required fields, coordinate units, errors, or result shape require a new operation
version and export, such as `check_v2`. Advertise both versions while older hosts need v1. Changes to memory ownership
or
calling conventions require a new ABI version; incompatible discovery changes require a new metadata version. A plugin
release version does not replace any of these versions. Ruff rules and defaults follow the engine version and user
configuration, not the memory ABI.

The current build has no `format_v1` or `fix_v1` export, and both corresponding capabilities are false. Adding those
exports requires a specified and tested contract. A fix contract must distinguish safe and unsafe fixes and specify
coordinates, non-overlap, and the exact source revision. Producing edits does not authorize applying them. The CLI or
LSP
host decides when to apply edits, obtains unsafe-fix opt-in, and maps edits to workflow YAML.

An actionlint LSP can use check diagnostics and negotiate future formatting and fix operations. The host converts
one-based Unicode coordinates to the negotiated LSP encoding, maps edits to YAML, and rejects edits that cross sanitized
expressions or refer to stale documents. The engine exposes no LSP transport. The host manages LSP methods and document
lifetime. Formatting, generating fixes, and applying edits are separate capabilities.

## Static core metadata

The first section after the eight-byte core WebAssembly header is the `actionlint.plugin` custom section. Its UTF-8 JSON
payload matches `metadata.json` byte for byte, including the final newline and full schema. It describes the same object
as `metadata()`.

A reader can stop after the first section. Read the core header, section ID `0`, the u32 LEB128 section length, the u32
LEB128 name length, and the UTF-8 name `actionlint.plugin`; parse the remaining section bytes as JSON. No runtime or
code
section is needed. The TypeScript reader rejects invalid headers, truncated or overflowing lengths, malformed metadata,
and metadata above 1 MiB.

An HTTP Range response can provide the required prefix. Alternatively, fetch the identical `metadata.json` sidecar.
Decompress a gzip artifact before reading its section. Metadata inspection does not validate the executable module;
the host must validate the module when loading it.

Packaging inserts or replaces the section after optimization and generates the gzip artifact and checksums. The build
obtains `schema.json` by calling `metadata` on the final module. This keeps editor completion aligned with the engine
without a separate checked-in schema. See
[Ruff's options definition](https://github.com/astral-sh/ruff/blob/5b27c1ccfaa230fb195855f5cf3404a21e3379cd/crates/ruff_workspace/src/options.rs)
and
[WASM checker](https://github.com/astral-sh/ruff/blob/5b27c1ccfaa230fb195855f5cf3404a21e3379cd/crates/ruff_wasm/src/lib.rs).

## Host portability and verification

The serialized contract does not depend on the host language. `transport: "wasm-core-json"` identifies the core binding;
legacy metadata without `transport` identifies the same binding. `abi_version: 1` defines its memory and calling rules,
while operation versions define the data exchanged. Changing a host from Go to Rust does not change plugin IDs,
operation versions, or packaged artifacts.

Guest pointers are offsets into guest memory, not addresses in the host. Go struct layouts, Rust trait objects and enum
layouts, allocators, compiler versions, and runtime-specific compiled modules are outside the contract. Hosts copy
results to their own memory and release guest allocations with `dealloc`. Response and discovery parsers must accept
unknown optional fields; Rust hosts must not use `serde(deny_unknown_fields)` on those objects. Request validation
follows
the versioned rules above. A native Rust interface cannot replace the serialized ABI. Runtime compilation caches stay on
the host that produced them.

Source and filename values are JSON strings. Coordinates are Unicode scalar values, not UTF-8 byte offsets, grapheme
clusters, or LSP UTF-16 offsets. Virtual filenames do not grant file access. YAML syntax trees, editor protocol objects,
and cancellation are host implementation details. Hosts can compile a module once and pool instances. They pass
sanitized
Python and Ruff options, then filter expression effects and map locations to YAML. The plugin does not interpret
workflow expressions, YAML, shell commands, or actionlint suppressions. Package discovery is disabled, so rules that
need
package roots have no repository context.

Go/wazero, Rust/Wasmi, and TypeScript/WebAssembly tests load the packaged core artifact. The independent Rust host
package has no dependency on the plugin crate or Ruff and shares the frozen v1 signature fixture with the Go tests.
These tests cover metadata discovery, configuration errors, Unicode, buffer ownership, repeated calls, and interruption
under host limits. Wasmi is a host-test dependency, not a guest dependency. The component tests use actionlint's public
loader. Both adapters call the same Rust analysis code.

The `Contract checks` workflow builds the artifacts and runs the host, discovery, CLI, formatting, and lint checks on
pull requests and pushes to `master`. Run the checks against rebuilt artifacts before changing an interface. Extending
`tests/fixtures/abi-v1.json` does not make a v1 break compatible.

The native utility accepts the same JSON requests for development. It does not implement the former generic
workflow/snippet-batch process protocol. Actionlint's public loader supports both the core module and typed component;
neither needs a native Ruff executable.

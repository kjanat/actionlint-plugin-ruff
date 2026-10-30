# Embedded Ruff WASM contract

This is the implemented contract for `actionlint-plugin-ruff`.
Actionlint owns shell selection, expression sanitization, source mapping, suppression, scheduling, and cancellation.
Ruff owns Python analysis and configuration validation. The plugin receives one already-sanitized snippet per call.

## Template provenance semantics

The optional `kjanat:actionlint/template-effects@0.2.0` component interface returns each
diagnostic together with a typed effect describing how expression substitution affects
its validity. Its `contract()` returns the semantic identifier `kjanat:actionlint/template-effects@0.1.0`. The original script-engine
interface remains available. Core JSON `check` operation version 2 adds `template_effect`
and requires the matching `template_contract` discovery field; operation version 1 and
the physical memory ABI version 1 remain supported.
Its self-contained request and diagnostic types use the same sanitized-source and Unicode
coordinate semantics as script-engine; the two interfaces can evolve independently.

Effects identify line length, complete references, generated names or parentheses,
string contents, docstring summary, exported names, and percent-conversion slots.
`none` explicitly preserves a finding. Hosts filter using the effect and recorded
substitution ranges, never diagnostic codes or human-readable messages. In particular,
export findings identify the exact exported string range. Hosts use a valid `s`
conversion in percent-conversion masks, so synthetic unsupported-conversion findings
do not require inspecting upstream message text.

The Ruff adapter is reviewed against Ruff 0.17.0. Changing the linked engine version
requires reviewing the effect mapping and range fixtures; an unreviewed engine is
rejected. The Python default belongs to the engine and its generated schema. Hosts
forward an explicit target only when configured or inferred from an interpreter.

## Typed script-engine component

New integrations use `artifacts/plugin.component.wasm` or its gzip form. The component exports
`kjanat:actionlint/script-engine@0.2.0`, defined in
[`vendor/actionlint-extension-api/wit/script-engine.wit`](../vendor/actionlint-extension-api/wit/script-engine.wit).

`describe()` returns typed plugin/engine identity, supported shells and one rule descriptor.
`validate-config()` accepts the engine's JSON options object and returns a typed success/error result.
`check()` accepts typed source, virtual filename and JSON configuration, returning typed diagnostics and an
independent optional incomplete-analysis error. Positions count one-based Unicode scalar values and end
positions are exclusive. Findings, errors, severity, and coordinates are not transported as JSON blobs.

Static `component.metadata.json` is embedded in the outer `actionlint.plugin` section. It records
`transport: "wasm-component"`, the interface name, the SHA-256 of LF-normalized WIT, the same plugin identity,
engine version, rules and options schema returned by `describe()`. Packaging includes both raw and gzip
digests in `SHA256SUMS`; the public host verifies metadata against the executable interface and descriptor.
The WIT source digest records provenance; compatibility is checked structurally, so compatible
independent implementations need not copy this repository's WIT formatting or comments.

The guest targets `wasm32-unknown-unknown` and is componentized with the Cargo-locked `wit-component`
encoder. It has no host imports: filesystem, credentials, clocks and network services are unavailable.
The existing clock facade traps if analysis unexpectedly requests time.
The public-loader tests check both exact packaged variants, typed configuration errors, Unicode/CRLF
coordinates and workflow expression masking/source mapping. Tests use the public host API, including
an arbitrary provider identity and changed diagnostic presentation; they never read host source files.
The integration modules pin actionlint commit `8836017f3c5150fcd6eba7b4ae64658b2d5c53e1`, the same revision
as the vendored SDK; guest builds never depend on a sibling checkout.

The following core ABI remains supported independently for existing consumers.

## Core module ABI, version 1

The packaged module is `artifacts/plugin.wasm`, targeting `wasm32-unknown-unknown`. It requires no host imports or WASI.
Requests and responses are UTF-8 JSON bytes in linear memory.

For embedding in any host executable, use the reproducible `artifacts/plugin.wasm.gz` asset and decompress it once
before passing the bytes to a compatible core WASM runtime. It restores the exact same core module and preserves this
ABI.
Go can use `compress/gzip`; a Rust host can use its preferred gzip implementation.

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

The equivalent Rust unpacking preserves the signed WASM result's bits:

```rust
let bits = result as u64;
let pointer = (bits >> 32) as u32;
let length = bits as u32;
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

## Host portability

This is a serialized plugin contract, independent of the language implementing actionlint. `transport: "wasm-core-json"`
identifies the current binding; absent on legacy artifacts means the same core WASM/JSON binding. `abi_version: 1`
specifies that binding's exact memory and calling rules. Operation versions describe the data contract. A rewrite from
Go to Rust changes the host adapter, not the plugin ID, operation versions, or artifacts.

The boundary contains WASM integers, guest-owned linear-memory buffers, and UTF-8 JSON. Guest pointers are offsets into
guest memory, never pointers into Go or Rust host memory. No Go struct layout, Rust trait object, enum layout,
allocator,
compiler version, or runtime-specific compiled-module format is part of the contract. Hosts copy results into their own
memory and release guest allocations with the guest's `dealloc`. They must parse response objects with support for
unknown optional fields; Rust consumers must not apply `serde(deny_unknown_fields)` to extensible response or discovery
objects. Requests retain the stricter versioned rules described below.

Source and filenames are ordinary JSON strings. The v1 coordinate unit is Unicode scalar values, not UTF-8 byte indices,
grapheme clusters, or LSP UTF-16 units. Preserve it when changing the host language. Virtual filenames describe
snippets;
they do not grant filesystem access. Actionlint's YAML AST, internal diagnostic types, editor protocol objects, and
cancellation primitives remain host implementation details.

The same packaged artifact is exercised by independent Go/wazero, Rust/Wasmi, and TypeScript/WebAssembly hosts. The Rust
host test is a separate Cargo package with no dependency on this plugin crate or Ruff, and uses the same frozen v1
signature fixture as the Go test. It verifies static prefix discovery, metadata parity, config error shapes, repeated
Unicode checks, input/output ownership, and fuel-based interruption under a host memory limit. Its runtime dependency is
kept out of the guest build.

A future Rust rewrite may use a different compatible core runtime without changing the contract. The typed WIT
component is separately versioned and keeps the core v1 artifact available for existing hosts. Both adapters call
the same typed Rust analysis functions. A core module does not become a component by relabeling metadata.
Runtime-specific compilation caches stay local to each host;
redistributable plugins remain portable artifacts. Native Rust linking may be offered separately, but Rust ABI/trait
compatibility is not a replacement for this serialized boundary.

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
  "metadata_version": 1,
  "abi_version": 1,
  "transport": "wasm-core-json",
  "id": "actionlint-plugin-ruff",
  "name": "Ruff",
  "description": "Hermetic Ruff engine for actionlint, with a core WebAssembly JSON ABI",
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

The actual `config_schema` is generated by `schemars` from the linked Ruff `Options` type, with unsupported embedded
options removed and plugin defaults annotated. Embed it as a complete schema document, preserving its `$defs` and
references. JSON Schema cannot express every Ruff semantic error; `validate_config` is authoritative.

### Discovery and plugin selection

`id` is the stable plugin identity; `plugin_version` is its release version. Project URLs refer to this wrapper,
while `engine` identifies the linked Ruff version. `update_url` with `update_format: "github-releases-v1"` identifies
GitHub's releases API, not a WASM download URL or a request to update automatically. There are currently no published
releases; an empty feed means no update is available. An updater must select an eligible release, obtain its artifacts
and checksums, and check compatibility before replacing an installed plugin. Never derive a supported interface from a
plugin or Ruff version number.

`languages` uses editor language identifiers. `shells` contains canonical actionlint shell identifiers, so `python`
covers snippets the host has identified as Python. The host continues to parse interpreter commands such as
`python3.12 -u {0}`, sanitize expressions, and select snippets; the plugin does not parse shell command lines.
`file_extensions` contains lowercase extensions without the leading dot. `.pyi` selects stub parsing. These are
selection hints, not a reason to reject a sanitized Python snippet with a virtual filename.

`conflicts` lists plugin IDs that cannot be active together. An empty list declares no explicit cross-plugin conflict.
Identical plugin IDs are alternate versions of the same plugin. An operation's `exclusive_group` declares
interchangeable
providers: the host chooses one provider in that group for that operation on a matching snippet, preventing duplicate
Ruff diagnostics. Overlapping language support alone does not constitute a conflict. Operation groups are independent,
so a future formatter can coexist with a checker. The host controls selection and overrides; metadata never disables
another plugin by itself.

### Interface compatibility and evolution

Compatibility has three independent versions:

| Field                              | Versions                                                                                    |
| ---------------------------------- | ------------------------------------------------------------------------------------------- |
| `metadata_version`                 | Discovery object format. Absent on legacy artifacts means the original v1 metadata.         |
| `abi_version`                      | Core memory layout, allocation ownership, calling convention, and host-import requirements. |
| `interfaces.<operation>[].version` | That operation's JSON request, response, coordinates, and semantics.                        |

Version numbers are exact positive integers, not minimum host versions. Hosts explicitly select an understood operation
version and its advertised `export`. An absent operation or unsupported version disables that optional feature; a
missing required operation prevents activation. Unknown optional fields and operation names must be ignored. The
provided `resolveInterface` and `requireInterface` helpers implement these rules without a runtime. The development
host checks the operation versions before checking a snippet. Legacy artifacts without `metadata_version` or
`interfaces` expose only the original `check`, `validate_config`, and `metadata` v1 exports.

Keep every advertised v1 contract usable. Add optional response/discovery fields with existing fields and meanings
preserved. Existing `check` requests reject unknown fields, so a host cannot send new request fields merely because a
plugin is newer. A new request shape, changed required field, coordinate unit, error meaning, or result shape needs a
new
operation version and a separate export, for example `check_v2`. Advertise both versions while retaining v1 for old
hosts. Memory/ownership or calling-convention changes need a new ABI version; incompatible discovery changes need a new
metadata version. Plugin release versions do not substitute for these contract versions. Ruff rule/default changes
remain engine behavior and are governed by the engine release and user configuration, not the memory ABI.

For example, a later plugin could add `format: [{ "version": 1, "export": "format_v1" }]` and
`fix: [{ "version": 1, "export": "fix_v1" }]` alongside the existing interfaces. This build has neither export:
`capabilities.format` and `capabilities.fixes` are false. Adding them requires specified and tested contracts first;
changing a boolean is insufficient. A future fix contract must distinguish safe and unsafe fixes and define edit
coordinates, non-overlap, and the exact source revision to which edits apply. Producing edits does not authorize
applying
them: the CLI or LSP host owns automatic application, unsafe-fix opt-in, and mapping back to workflow YAML.

An actionlint LSP can reuse the check interface for diagnostics and later negotiate format/fix interfaces for formatting
and code actions. The host translates one-based Unicode positions to its negotiated LSP position encoding, maps snippet
edits to YAML, and rejects edits spanning sanitized expressions or stale document versions. The engine does not
advertise
an LSP transport; LSP methods and document lifetime belong to the host. Formatting, fix generation, and automatic edit
application remain separate capabilities.

The frozen `tests/fixtures/abi-v1.json` signatures are checked against the actual compiled module by the Go and Rust
host integration tests. Existing Rust, TypeScript, and Go probes retain the v1 requests, error shapes, buffer ownership,
and Unicode
coordinates. Metadata tests cover legacy discovery, coexistence of operation versions, unsupported required versions,
unknown optional extensions, unsupported transports, and ambiguous descriptors. Run these checks against rebuilt
artifacts before accepting any
interface change; extending the fixture to excuse a v1 break is not a compatible update.

The `Contract checks` workflow runs these checks on pull requests and pushes to `master`. It builds and packages the
engine before exercising static discovery, operation negotiation, runtime behavior, CLI handling, and both Go and Rust
hosts,
then checks formatting, Rust/TypeScript linting, and Go cleanup errors with `errcheck`. Generated artifacts remain build
outputs rather than committed
catalogs.

### Static metadata without instantiation

The packaged module has an `actionlint.plugin` custom section immediately after the eight-byte core WASM header. Its
payload is UTF-8 JSON, byte-for-byte identical to `metadata.json`, including the trailing newline and full schema. It
contains the same object returned by the runtime `metadata()` export.

To inspect the module without a runtime, read the core header, section id `0`, a u32 LEB128 section length, a u32 LEB128
name length, the UTF-8 name `actionlint.plugin`, then the remaining section bytes as JSON. The reader can stop there;
it does not need to read, compile, validate, or execute the code section. The provided TypeScript reader rejects invalid
headers, overflowing/truncated lengths, malformed metadata, and metadata payloads above 1 MiB.

This layout supports reading a file prefix or an HTTP Range response containing the complete first section. For network
discovery, the identical `metadata.json` sidecar can be fetched directly. Gzip assets must be decompressed before
section
inspection. Reading metadata does not validate the executable module; normal module validation happens when it is loaded
for checking Python.

The packaging step adds or replaces this section after WASM optimization, then regenerates the gzip asset and checksums.
The builder generates the payload from the engine rather than maintaining a separate manifest or schema snapshot.

The build obtains `schema.json` by calling `metadata` on the final module, keeping editor completion and the engine on
the same version without a schema snapshot in the source tree. Upstream references are
[Ruff's options](https://github.com/astral-sh/ruff/blob/5b27c1ccfaa230fb195855f5cf3404a21e3379cd/crates/ruff_workspace/src/options.rs)
and its
[WASM checker](https://github.com/astral-sh/ruff/blob/5b27c1ccfaa230fb195855f5cf3404a21e3379cd/crates/ruff_wasm/src/lib.rs).

## Host responsibilities

A Go or Rust host can compile the embedded module once and pool instances. It forwards sanitized Python and opaque Ruff
options,
unwraps the response, then applies expression-span filtering and actionlint's YAML source mapping. The plugin does not
interpret `${{ }}`, workflow YAML, shell strings, or actionlint suppressions. The host supplies virtual filenames and
interpreter-derived targets. Package discovery is disabled, so checks requiring package roots lack repository context.

The native utility is for development and consumes the same JSON requests. It does not implement the previous generic
workflow/snippet-batch process protocol. Actionlint's public loader supports the legacy core module and the typed
script-engine component; neither backend requires a native Ruff executable.

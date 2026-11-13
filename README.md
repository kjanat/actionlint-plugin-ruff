# actionlint-plugin-ruff

Run Ruff on Python snippets in GitHub Actions workflows through actionlint. The plugin packages Ruff as WebAssembly, so
its host does not need Python or a Ruff executable.

Use `plugin.component.wasm` for a new integration. It implements
`kjanat:actionlint/script-engine@0.2.0`. Existing integrations can use `plugin.wasm`, which provides the version 1
core WebAssembly JSON ABI. Both packages use the same Rust analysis and configuration code. See the
[plugin contract](docs/plugin-contract.md) for host requirements and interface versions.

## Use with actionlint

Build the artifacts, then pass the component to actionlint:

```sh
deno task build
actionlint check --plugin /path/to/artifacts/plugin.component.wasm
```

The host selects the Ruff engine by its script-engine role. It handles workflow expressions, script selection, YAML
source locations, severity, and suppressions. Configure Ruff through `tools.ruff`. Explicitly selecting a competing
backend is an error. The core JSON transport loads `plugin.wasm` instead.

Ruff validates options, selects rules, parses Python, and produces diagnostics. The plugin ignores `noqa` suppressions,
returns Unicode character columns, and does not apply fixes. The host provides a virtual filename. The engine uses it
for stub parsing, per-file targets, and per-file ignores, but never opens it. Import sorting uses configured known
modules and does not scan the filesystem for first-party imports.

Pass a Ruff options object as `config`:

```json
{
  "source": "import os\nprint(missing)  # noqa\n",
  "filename": "ci.step.py",
  "config": {
    "target-version": "py314",
    "lint": { "extend-select": ["B", "UP"], "ignore": ["F401"] }
  }
}
```

Without `select`, Ruff uses its own defaults. An empty `select` list disables lint rules, but syntax errors still
appear.
The default Python target is `py314`; the host may supply a target inferred from the interpreter. Ruff accepts both
`lint` options and legacy top-level lint options. The host handles the actionlint `enabled` setting.

The engine rejects options that read configuration, expand environment-dependent paths, or cause CLI side effects:
`extend`, `cache-dir`, `src`, `namespace-packages`, `fix`, `fix-only`, `unsafe-fixes`, `show-fixes`, and `output-file`.
Ruff accepts file discovery and output-format options, but they do not change this single-snippet API. The generated
schema lists supported options, and Ruff performs further semantic validation.

## Build and test

Install Rust 1.99.1 with the `wasm32-unknown-unknown` target, Go 1.27.3, Deno 2, and Binaryen `wasm-opt` 133.
`rust-toolchain.toml` configures Rust; `mise install` supplies Go, Deno, Binaryen, and the documentation tools.

```sh
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
deno task build
deno task test:wasm
deno task test:cli
go -C tests/wazero test -mod=readonly -v ./...
go -C tests/component test -mod=readonly -v ./...
cargo test --locked --manifest-path tests/rust-host/Cargo.toml
cargo clippy --locked --manifest-path tests/rust-host/Cargo.toml --all-targets -- -D warnings
```

The build uses `Cargo.lock` and fails if dependency resolution would change it. It builds both transports, optimizes the
core module, checks that the module has no imports, and obtains its schema through `metadata()`. A Cargo-locked
`wit-component` encoder turns the guest into a component and packages its WIT digest with it.

The build writes these files to `artifacts/`:

| File                                                | Contents                                                  |
| --------------------------------------------------- | --------------------------------------------------------- |
| `plugin.component.wasm`, `plugin.component.wasm.gz` | Typed component and gzip copy                             |
| `component.metadata.json`                           | Component identity, WIT digest, engine, rules, and schema |
| `plugin.wasm`, `plugin.wasm.gz`                     | Core JSON module and gzip copy                            |
| `metadata.json`                                     | Core engine version, ABI, capabilities, and schema        |
| `schema.json`                                       | Ruff option schema from the core module                   |
| `SHA256SUMS`                                        | Package checksums                                         |
| `LICENSE`, `LICENSE-RUFF`                           | Project license and Ruff attribution notices              |

The component and core module need no filesystem, environment, clock, or network imports. The project replaces Ruff's
`web-time` dependency with a local adapter that uses `std::time`; an unexpected clock call traps. Ruff's `typed-path`
dependency requires an unstable Rust feature on `wasm32-wasip2`, so the guest uses `wasm32-unknown-unknown`.
Component encoding itself works on stable Rust.

The gzip file reduces the bytes stored in a host executable, not the compiled engine's memory use. A Go host can use
`compress/gzip` before compiling the module with wazero. Tests load the packaged module with wazero, Wasmi, and the
Deno WebAssembly runtime. CI also runs both packaged transports through actionlint on Linux, macOS, and Windows,
including first loads with the host's default limits.

## Inspect metadata

The core module's first custom section is `actionlint.plugin`. Its contents match `metadata.json`, including the schema.
A reader only needs the file through the end of that section; it does not need a WebAssembly runtime. HTTP servers can
provide the prefix through a Range request or serve `metadata.json` separately. The `readMetadata` helper in
`scripts/metadata.ts` reads the section.

```sh
deno task inspect
deno task inspect artifacts/plugin.wasm.gz
deno task inspect --help
```

The builder reads metadata from Ruff, embeds it after optimization, and generates the gzip file and checksums from the
result. Repackaging replaces an existing metadata section.

## Develop and update Ruff

The native utility accepts the same JSON requests:

```sh
printf '%s\n' '{"source":"print(missing)\n","filename":"step.py"}' | cargo run --locked -- check
printf '%s\n' '{"lint":{"select":["F821"]}}' | cargo run --locked -- validate-config
cargo run --locked -- metadata
cargo run --locked -- schema
```

`Cargo.toml` declares the Ruff crates from crates.io, and `Cargo.lock` pins their resolved versions. To update Ruff,
change all Ruff dependencies, update and review `Cargo.lock`, and run the checks above. The build obtains the version
from
`ruff_linter::VERSION` and the schema from `ruff_workspace::options::Options`; there is no separate rule catalog or
schema snapshot to update. Ruff API changes may require changes to the wrapper.

Cargo builds the component with the actionlint SDK at revision
`ab420b66fb98af9d993c176e97506ecec9d3a396`, pinned in `crates/component/Cargo.toml` and `Cargo.lock`.
The Go integration tests use the actionlint revision in `tests/component/go.mod`. Their unpublished modules use the
actionlint module prefix to access private host packages. A build does not need a sibling actionlint checkout or module
replacement.
The vendored WIT is a digest test fixture, recorded in the
[snapshot manifest](vendor/actionlint-extension-api.snapshot.json). It does not supply the component build's SDK.
Update that fixture as a unit and do not edit its WIT separately.

## Publish

Set the package versions in `Cargo.toml` and `crates/component/Cargo.toml` to `X.Y.Z`, refresh `Cargo.lock`, and enable
GitHub release immutability. Push a signed `vX.Y.Z` tag through the existing release process.

The Release workflow runs CI for the tag. CI uses the registry's pinned actionlint host to
[check the candidate](https://github.com/kjanat/actionlint-registry/blob/master/docs/publishing.md). It installs the
component into a fresh project, runs [`tests/installation.ts`](tests/installation.ts), and repeats the test without
network access. The registry publish action uploads the tested packages, submits the component, waits for `published`,
and runs the fixture against the public catalogue. It retains publication and installation receipts as workflow
artifacts. `SHA256SUMS` covers the package files.

After a recoverable failure, rerun the Release workflow on the same tag. The workflows do not create tags or upgrade an
existing release.

The project uses the MIT license. `LICENSE-RUFF` contains Ruff's MIT license and upstream attribution notices; every
artifact bundle includes those notices.

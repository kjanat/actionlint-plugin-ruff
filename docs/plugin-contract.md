# actionlint external checker contract

Reader: actionlint maintainer replacing the pyflakes hook with a generic checker interface, and the author of
`actionlint-plugin-ruff`.

Ruff facts below come from `astral-sh/ruff` at [`5b27c1ccfaa230fb195855f5cf3404a21e3379cd`] (0.17.0).

## 1. What ruff offers

| Surface             | Transport                 | Fit for actionlint              |
| ------------------- | ------------------------- | ------------------------------- |
| `ruff check -`      | process, stdin → stdout   | yes                             |
| `ruff server`       | LSP over stdio            | only for an LSP-mode actionlint |
| `ruff_wasm`         | npm package, wasm-bindgen | no, needs JS glue               |
| `ruff_linter` crate | Rust link                 | no, actionlint is Go            |

### 1.1 `ruff check` CLI

Invocation actionlint needs:

```sh
ruff check --isolated --no-cache --output-format json --exit-zero \
  --select F --target-version py3XX --stdin-filename <virtual path> -
```

- `-` plus `--stdin-filename` reads stdin. `--stdin-filename` alone also forces stdin and ignores file args with a
  warning (`crates/ruff/src/lib.rs` `is_stdin`).
- `--stdin-filename` resolves config discovery, exclusion, package root and the `filename` field. Ruff absolutizes it
  against cwd. The file does not have to exist.
- `--isolated` skips every `pyproject.toml` and `ruff.toml`. Without it ruff walks up from the stdin filename and
  applies the repo config: `select`, `ignore`, `per-file-ignores`, `target-version`, `preview`. A repo with `select =
  ["ALL"]` then lints workflow snippets with every rule. `exclude` patterns only apply to stdin when the config sets
  `force-exclude = true` (`crates/ruff/src/commands/check_stdin.rs`, default `false` in
  `crates/ruff_workspace/src/settings.rs`); a match returns zero diagnostics with exit 0.
- `--config '<KEY> = <VALUE>'` appends inline TOML overrides, repeatable, highest precedence.
- `--select`, `--extend-select`, `--ignore` take codes or prefixes. `F` selects the whole Pyflakes set.
- `--ignore-noqa` disables `# noqa` honouring.
- `--preview` changes the JSON: `code` becomes nullable and `severity` becomes real instead of a constant `"error"`.
- `--extension ext:python` maps unknown extensions. Give the virtual path a `.py` suffix and this is unnecessary.
- Env: `RUFF_OUTPUT_FORMAT`, `RUFF_NO_CACHE`, `RUFF_CACHE_DIR`, `RUFF_OUTPUT_FILE`. Pass flags explicitly so the user's
  env cannot change the output shape.
- `@argfile` expands to a file list, useful when linting many temp files in one process.

Exit codes (`crates/ruff/src/lib.rs`):

| code | meaning                              |
| ---- | ------------------------------------ |
| 0    | no violations, or `--exit-zero`      |
| 1    | violations found                     |
| 2    | ruff failed: config error, panic, IO |

With `--exit-zero` the fatal-severity branch is skipped too, so a panic inside ruff still exits 0. The plugin must look
at `severity` and stderr, never trust exit code alone when `--exit-zero` is set. Without `--exit-zero`, 1 means
"diagnostics on stdout", 2 means "stdout untrustworthy, show stderr".

### 1.2 JSON diagnostic shape

`--output-format json` prints an array, `json-lines` prints one object per line. Same object
(`crates/ruff_db/src/diagnostic/render/json.rs`):

```json
{
  "cell": null,
  "code": "F821",
  "name": "undefined-name",
  "severity": "error",
  "message": "Undefined name `a`",
  "filename": "/abs/path/from/stdin-filename.py",
  "location": { "row": 1, "column": 4 },
  "end_location": { "row": 1, "column": 5 },
  "noqa_row": 1,
  "url": "https://docs.astral.sh/ruff/rules/undefined-name",
  "fix": {
    "applicability": "safe",
    "message": "Remove unused import: `os`",
    "edits": [
      {
        "content": "",
        "location": { "row": 1, "column": 1 },
        "end_location": { "row": 2, "column": 1 }
      }
    ]
  }
}
```

- `row` and `column` are 1-based. `end_location` is exclusive.
- Syntax errors: `code` is `"invalid-syntax"` (non-preview) or `null` (preview), `name` is `"invalid-syntax"`, `url`
  null, `noqa_row` null.
- `fix.applicability` is `safe`, `unsafe` or `display`.
- `location` and `filename` are nullable in the schema. In practice they are set for every stdin diagnostic.

### 1.3 SARIF

`--output-format sarif` emits SARIF 2.1.0 with `tool.driver.name = "ruff"`, rule descriptors, `file://` URIs from the
absolutized stdin filename, and `fixes` with `deletedRegion` + `insertedContent`. Syntax errors appear as results
without a rule descriptor. Usable as the neutral format if actionlint wants one parser for every tool, but it is bulkier
and needs URI → path mapping.

### 1.4 Metadata commands

- `ruff version --output-format json` → `{ "version": "0.17.0", "commit_info": {...}|null }`.
- `ruff rule F821 --output-format json` → `{ name, code, linter, summary, message_formats, fix, fix_availability,
  explanation, preview, status, category, source_location }`.
- `ruff rule --all --output-format json` → array of the above.
- `ruff linter --output-format json` → `[{ prefix, name, url, categories? }]`.

Enough for the plugin to validate a user's `select` list and to render rule docs links offline.

### 1.5 Pyflakes rule set in ruff

`crates/ruff_linter/src/codes.rs` registers 43 `F` rules: F401–F407, F501–F509, F521–F525, F541, F601, F602, F621, F622,
F631–F634, F701, F702, F704, F706, F707, F722, F811, F821–F823, F841, F842, F901. `--select F` is the pyflakes
replacement.

### 1.6 `ruff server`

LSP over stdio, `ruff server [--preview] [--untrusted-workspace]`. Negotiates position encoding from client
capabilities. Documents enter through `textDocument/didOpen` with `languageId`. Supports push diagnostics and pull
diagnostics (`textDocument/diagnostic`) when the client advertises it. Settings per workspace via
`initializationOptions` / `workspace/configuration`: `configuration` (path or inline TOML object),
`configurationPreference`, `lint.select`, `lint.extendSelect`, `lint.ignore`, `lint.preview`, `exclude`, `lineLength`,
`showSyntaxErrors`, `logLevel`.

Relevant only if actionlint gains a long-lived mode. One ruff process per actionlint run with virtual `untitled:`
documents per step would amortize startup, but ruff startup is already single-digit ms.

## 2. Contract actionlint sets for external checkers

Shape: actionlint owns the mapping from a workflow step to a snippet and back. The checker owns the language. Everything
between is one JSON request and one JSON response over a child process.

The contract is actionlint's. actionlint carries no knowledge of any tool's flags, config files, output formats or exit
codes. A tool joins by shipping an adapter that speaks this protocol. Ruff, shellcheck, PSScriptAnalyzer each get an
adapter; actionlint stays unchanged when a tool changes its CLI.

### 2.1 Discovery

actionlint config (`actionlint.yaml`) declares checkers:

```yaml
checkers:
  - name: ruff
    command: [actionlint-plugin-ruff]
    languages: [python]
```

- `name` is the diagnostic source shown to users and the key for inline suppression (`# actionlint-ignore ruff/F401`).
- `command` is argv, exec'd without a shell. Resolution via `PATH`.
- `languages` are actionlint's shell identifiers: `bash`, `sh`, `pwsh`, `powershell`, `python`, `cmd`. actionlint
  computes the effective shell per step (step `shell` → job `defaults.run.shell` → workflow `defaults.run.shell` →
  runner default) and only dispatches matching steps.
- One process per workflow file, not per step. The request carries every matching step of that file.

Builtin shellcheck and the pyflakes code are reimplemented as checkers under this contract, so the contract has two
first-party consumers from day one.

### 2.2 Request

actionlint writes one JSON object to the checker's stdin, then closes stdin.

```json
{
  "protocol": 1,
  "workflow": "/abs/.github/workflows/ci.yml",
  "snippets": [
    {
      "id": "jobs.test.steps[2]",
      "language": "python",
      "source": "import os\nprint(x)\n",
      "line": 42,
      "column": 9,
      "indent": 8,
      "expressions": [
        { "line": 2, "column": 7, "end_column": 23, "text": "${{ matrix.name }}" }
      ],
      "env": ["GITHUB_TOKEN", "FOO"]
    }
  ]
}
```

- `protocol` is the contract version. The checker rejects unknown majors with exit 3.
- `source` is the `run:` scalar after YAML folding, exactly what the runner would write to the script file. Line 1 of
  `source` is `line` in the workflow. `column` is the column of the scalar's first character. `indent` is the block
  indent stripped by YAML, so column mapping is `workflow_col = snippet_col + indent` for every line after the first and
  `column + snippet_col - 1` for line 1 when the scalar starts inline.
- `expressions` lists `${{ }}` spans inside the snippet in snippet coordinates. actionlint replaces them with a
  placeholder before sending (upstream pyflakes hook does the same with `${{ }}` → `0`). The checker can use the spans
  to drop diagnostics that sit entirely inside a placeholder.
- `env` is the set of names from `env:` at step, job and workflow level. Optional, lets a checker know about
  `os.environ` keys. Not needed for ruff.

### 2.3 Response

The checker writes one JSON object to stdout.

```json
{
  "protocol": 1,
  "checker": { "name": "ruff", "version": "0.17.0" },
  "diagnostics": [
    {
      "snippet": "jobs.test.steps[2]",
      "rule": "F821",
      "severity": "error",
      "message": "Undefined name `x`",
      "line": 2,
      "column": 7,
      "end_line": 2,
      "end_column": 8,
      "url": "https://docs.astral.sh/ruff/rules/undefined-name",
      "fix": {
        "applicability": "safe",
        "message": "Remove unused import: `os`",
        "edits": [{ "line": 1, "column": 1, "end_line": 2, "end_column": 1, "content": "" }]
      }
    }
  ],
  "errors": ["ruff exited 2: ..."]
}
```

- Coordinates are 1-based, snippet-relative, `end_*` exclusive. actionlint translates to workflow coordinates. The
  checker never sees or needs workflow line numbers for output.
- `rule` is the checker's own code. actionlint renders `[ruff/F821]` and matches suppressions on `ruff/F821`, `ruff/F`,
  `ruff`.
- `severity` is one of `error`, `warning`, `info`. actionlint decides exit status from its own severity policy, not the
  checker's.
- `fix` is optional. actionlint ignores it today, keeps it for a future `-fix`.
- `errors` are checker-level failures that did not prevent other snippets from being checked. Rendered once per workflow
  as a single actionlint error, not per step.

Exit codes:

| code          | meaning                                                                 |
| ------------- | ----------------------------------------------------------------------- |
| 0             | response on stdout, possibly with diagnostics and `errors`              |
| 3             | protocol mismatch, stdout empty, stderr explains                        |
| anything else | checker crashed, stdout discarded, stderr shown as one actionlint error |

Diagnostics are not an exit code. Exit code only says whether the response is valid.

### 2.4 Behaviour rules

- Hermetic: the response is a function of the request and the `config` object alone. A checker reads no config files,
  env vars or dotfiles on its own. A tool that discovers config from the filesystem gets that behaviour disabled by its
  adapter, and re-enabled only through an explicit `config` key.
- Deterministic order: diagnostics sorted by `snippet`, `line`, `column`, `rule`.
- No network, no file writes outside the temp dir, no reading the workflow file. Everything needed is in the request.
- Timeout per process is actionlint's. Default 30 s, configurable in `checkers[].timeout`.
- cwd is the workflow's repo root. The request carries `workflow` as an absolute path, so an adapter that was told via
  `config` to honour project config knows where to look.
- Stderr is for humans. actionlint shows it only on a non-zero exit.

## 3. `actionlint-plugin-ruff` on that contract

Per request: one `ruff check` process per snippet, or one process with `@argfile` over temp files when `snippets` > N.
Start with per-snippet via stdin, it is simplest and ruff startup is cheap.

Mapping:

| request / config                            | ruff flag                                                                                      |
| ------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| snippet `source`                            | stdin                                                                                          |
| `workflow` + snippet `id`                   | `--stdin-filename <workflow>.<id>.py`, only to give the `.py` suffix and a readable `filename` |
| always, unless `project_config: true`       | `--isolated`                                                                                   |
| plugin config `select` (default `["F"]`)    | `--select`                                                                                     |
| plugin config `ignore`                      | `--ignore`                                                                                     |
| plugin config `target_version`              | `--target-version`                                                                             |
| plugin config `extra_config` (TOML strings) | `--config ...` each                                                                            |
| always                                      | `--no-cache --output-format json --exit-zero --quiet -`                                        |

Response mapping from ruff JSON:

| ruff                                | contract              |
| ----------------------------------- | --------------------- |
| `code` (fallback `name` when null)  | `rule`                |
| `severity`                          | `severity`            |
| `message`                           | `message`             |
| `location.row/column`               | `line/column`         |
| `end_location.row/column`           | `end_line/end_column` |
| `url`                               | `url`                 |
| `fix.applicability/message/edits[]` | `fix`                 |

Drop a diagnostic when its range lies fully inside an `expressions` span: that code was `${{ }}` before placeholder
substitution.

`--isolated` is the default. With it the result depends on `config` alone, the same guarantee pyflakes gave.
`project_config: true` drops `--isolated`; ruff then walks up from the `--stdin-filename` path and applies the repo's
`pyproject.toml` or `ruff.toml`. The adapter still passes `--select` and `--ignore` from `config`, which override the
file. With `force-exclude = true` in that file a matching `exclude` silently yields zero diagnostics, so the adapter
passes `--no-force-exclude` whenever `project_config` is on.

Failure handling: ruff exit 2 or any `severity: "fatal"` → append to `errors`, keep diagnostics from the other snippets.
Ruff missing from `PATH` → exit 1 with a clear stderr line, actionlint renders that once.

Plugin config lives in `actionlint.yaml` under the checker entry and is forwarded verbatim as `config` in the request:

```yaml
checkers:
  - name: ruff
    command: [actionlint-plugin-ruff]
    languages: [python]
    config:
      select: [F, E9]
      target_version: py312
      project_config: false
```

So the request gains one field: `"config": { ...checker-specific... }`, opaque to actionlint.

## 4. Why a process contract and not a Go plugin API

- actionlint is a static Go binary. Go plugins need identical toolchain and are Linux-only. Process boundary keeps
  actionlint's build untouched.
- Checkers in any language. Ruff's own wasm is JS-only, so the plugin wraps the CLI anyway.
- One request per workflow keeps process count at `workflows × checkers`, not `steps × checkers`.
- Same contract for shellcheck, pyflakes-compat, pwsh `PSScriptAnalyzer`, future ones.

## Unresolved

- Steps inside placeholder: drop diagnostic fully inside `${{ }}` span, or report with note? Pick drop.
- Protocol as one-request-per-workflow or one-request-per-run (all workflows)? Per workflow simpler, per run fewer
  spawns. Pick per workflow.
- Keep `shellcheck` and `pyflakes` as builtin or move to this contract in same release? Builtin keeps users'
  `-shellcheck` flags working. Decide.
- Suppression key `ruff/F401` syntax vs reuse ruff `# noqa` inside `run:` text. Both can work, `# noqa` already works
  via ruff.
- Fix application: actionlint `-fix` not in scope now. Keep `fix` field or drop until needed?

[`5b27c1ccfaa230fb195855f5cf3404a21e3379cd`]: https://github.com/astral-sh/ruff/commit/5b27c1ccfaa230fb195855f5cf3404a21e3379cd

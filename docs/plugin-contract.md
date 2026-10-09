# actionlint external checker contract

Reader: actionlint maintainer replacing the pyflakes hook with a generic checker interface, and the author of
`actionlint-plugin-ruff`.

Ruff facts come from `astral-sh/ruff` at [`5b27c1ccfaa230fb195855f5cf3404a21e3379cd`] (0.17.0). actionlint facts come
from `kjanat/actionlint` branch `fix/actions-expression-semantics` at [`6567fe7a`] plus its uncommitted worktree.

## 1. What ruff offers

| Surface             | Transport                 | Fit for actionlint              |
| ------------------- | ------------------------- | ------------------------------- |
| `ruff check -`      | process, stdin → stdout   | yes                             |
| `ruff server`       | LSP over stdio            | only for an LSP-mode actionlint |
| `ruff_wasm`         | npm package, wasm-bindgen | no, needs JS glue               |
| `ruff_linter` crate | Rust link                 | no, actionlint is Go            |

### 1.1 `ruff check` CLI

Invocation the adapter needs:

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

With `--exit-zero` the fatal-severity branch is skipped too, so a panic inside ruff still exits 0. The adapter must look
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

Enough for the adapter to validate a user's `select` list and to render rule docs links offline.

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

## 2. What actionlint already has

Pyflakes is gone on this branch. `--pyflakes` is accepted and ignored (`internal/cli/root.go`, `internal/cli/check.go`),
the Action input prints a deprecation warning (`internal/githubaction/main.go`), and the `UPSTREAM.md` row for
rhysd/actionlint#439 names a generic external-linter protocol as the replacement.

Seams the contract reuses:

| concern                  | existing seam                                                                                                                                                                                     |
| ------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| process scheduling       | `concurrentProcess` (semaphore, `Context` cancels children), `externalCommand.runInDirectory`, `cmdExecution.run` in `process.go`                                                                 |
| tool resolution          | `ExternalCommandOptions{Executable, Arguments, Environment, WorkingDir}`, `ResolveExternalCommandOptions`, `execabs.LookPath`                                                                     |
| CLI / env surface        | `--shellcheck` flag; `ACTIONLINT_<TOOL>_BIN`, `_FLAGS`, `_ENV` through `externalToolEnvironment(tool)` in `internal/cli/env.go`; `doctor` rows with `available`/`unavailable`/`disabled`          |
| config                   | `Config.Tools` (`ToolsConfig`), `lint.external` rule group, `lint.disable`, `cfg.diagnosticLevel(name)`                                                                                           |
| rule registry            | `ruleDescriptor{Category: "external", build, enabled}` in `rule_registry.go`; `analysis_engine.go` skips external rules whose level is `off`                                                      |
| shell resolution         | `resolveRunShell` (step → job → workflow → runner/container), `shellcheckShell.analysis()` derives the dialect from the shell template                                                            |
| snippet → YAML positions | `ExecRun.source` (`*scriptSource`), `pos`/`endPos`, spans skip YAML-synthesized bytes, `literal` gates fixes                                                                                      |
| expression sanitizing    | `sanitizeExpressionsInScript`: `${{ … }}` → underscores, same length, newlines kept                                                                                                               |
| diagnostic metadata      | `Error.Kind` (rule name), `Error.code`, `Error.severity`, `Error.fixes`, `Error.endPosition`, `Error.source` for findings in other files; `Diagnostic.Rule/Code/Severity/Fixes` in JSON and SARIF |
| files read by a tool     | `inputFiles.add` through `rule.onInput`, surfaces as `AnalysisResult.Inputs`                                                                                                                      |
| inline suppression       | `# actionlint:ignore RULE -- reason`, rule names only, whitelist `isInlineSuppressibleRule`                                                                                                       |
| tool requirement query   | `AnalysisSession.RequiredTools` → `ExternalToolRequirements{Shellcheck bool}`                                                                                                                     |
| Go-level extension       | `LinterOptions.OnRulesCreated func([]Rule) []Rule`                                                                                                                                                |
| test harness             | `TestKnownShellCommandHelper`: the test binary re-executes itself as the fake tool                                                                                                                |

Everything above is shellcheck-shaped by name only. The contract generalizes the names, keeps the mechanisms.

## 3. The contract

actionlint owns the workflow: which steps, which language, positions, suppression, output. The checker owns the
language. Between them: one JSON request on stdin, one JSON response on stdout, per workflow file.

The contract is actionlint's. actionlint carries no knowledge of any tool's flags, config files, output formats or exit
codes. A tool joins by shipping an adapter that speaks this protocol. Ruff, shellcheck, PSScriptAnalyzer each get an
adapter; actionlint stays unchanged when a tool changes its CLI.

### 3.1 Registration

`actionlint.yaml`:

```yaml
tools:
  shellcheck: { enabled: true }
  ruff:
    command: [actionlint-plugin-ruff]
    languages: [python]
    config:
      select: [F]
lint:
  external:
    ruff: on
```

- `tools.<name>` is the checker. `<name>` becomes `Error.Kind`, the rule ID in `lint.external`, `lint.disable`,
  `# actionlint:ignore <name> -- reason`, and the `doctor` row. Registering it adds it to `isInlineSuppressibleRule`.
- `command` is argv, exec'd without a shell, resolved with `execabs.LookPath`. `ACTIONLINT_<NAME>_BIN`, `_FLAGS`,
  `_ENV` override it the way `SHELLCHECK` does today.
- `languages` are the normalized shell names actionlint already derives: command basename of the resolved shell
  template, lowercased, `.exe` stripped. `python`, `bash`, `sh`, `pwsh`, `powershell`, `cmd`, `node`. A step whose
  shell cannot be resolved (`shellValueUnknown`) is never dispatched.
- `config` is opaque to actionlint and forwarded verbatim.
- Each `tools.<name>` entry synthesizes a `ruleDescriptor` with `Category: "external"` at config load, so presets,
  `lint.external`, `doctor`, `RequiredTools` and the JSON schema see it without per-tool code.
  `ExternalToolRequirements` becomes `map[string]bool`.
- shellcheck keeps its builtin rule. `ShellcheckConfig` and `-x` source following are shellcheck-only and stay put.

### 3.2 Request

```json
{
  "protocol": 1,
  "workflow": "/abs/.github/workflows/ci.yml",
  "config": { "select": ["F"] },
  "snippets": [
    {
      "id": "jobs.test.steps[2]",
      "language": "python",
      "shell": "python -u {0}",
      "source": "import os\nprint(_____________)\n",
      "expressions": [{ "line": 2, "column": 7, "end_column": 20 }],
      "working_directory": "/abs/repo/src",
      "env": ["GITHUB_TOKEN", "FOO"]
    }
  ]
}
```

- `source` is `ExecRun.Run.Value` after `sanitizeExpressionsInScript`. Response coordinates refer to this text,
  1-based, so `scriptSource.pos`/`endPos` map them back and the checker never learns YAML exists. Underscores keep
  columns stable.
- `expressions` are the sanitized `${{ }}` spans in `source` coordinates. A checker drops a diagnostic that lies fully
  inside one.
- `shell` is the resolved template, `language` its normalized name. The checker picks flags from either.
- `working_directory` is `runPaths.effectiveRunDirectory` resolved locally, or `null` when it is `directoryUnknown`.
- `env` lists names from `env:` at step, job and workflow level. Names only.

### 3.3 Response

```json
{
  "protocol": 1,
  "tool": { "name": "ruff", "version": "0.17.0" },
  "diagnostics": [
    {
      "snippet": "jobs.test.steps[2]",
      "code": "F821",
      "severity": "error",
      "message": "Undefined name `x`",
      "line": 2,
      "column": 7,
      "end_line": 2,
      "end_column": 8,
      "url": "https://docs.astral.sh/ruff/rules/undefined-name",
      "fix": {
        "description": "Remove unused import: `os`",
        "edits": [{ "line": 1, "column": 1, "end_line": 2, "end_column": 1, "replacement": "" }]
      }
    }
  ],
  "inputs": ["/abs/repo/scripts/lib.sh"],
  "errors": ["ruff exited 2: ..."]
}
```

- `code` → `Error.code`, `severity` → `Error.severity`. `Error.Message` becomes
  `<name> reported issue in this script: <code>:<severity>:<line>:<col>: <message>`, the shellcheck wording. `url`
  feeds `Diagnostic` and SARIF `helpUri`.
- `line`/`column`/`end_line`/`end_column` are `source` coordinates, `end_*` exclusive. actionlint calls `source.pos`
  and `source.endPos`; when mapping fails the finding lands on `RunPos`, the shellcheck fallback.
- `path` (optional, absolute) on a diagnostic reports a finding in another file. actionlint builds it the way
  `sourcedDiagnostic` does and adds the path to `inputs`.
- `fix.edits` pass the `shellcheckDiagnosticFixes` gate: literal block only, one line per edit, no overlap, indent
  preserved. Otherwise the fix is dropped and the diagnostic kept.
- `severity` is `error`, `warning`, `info` or `style`. `sarifLevel` already maps these.
- `inputs` are files the checker read. They join `AnalysisResult.Inputs`.
- `errors` are checker-level failures. actionlint reports them once per workflow as a rule error, the path a shellcheck
  JSON parse failure takes today.

Exit codes:

| code          | actionlint behaviour                                                                          |
| ------------- | --------------------------------------------------------------------------------------------- |
| 0             | parse stdout                                                                                  |
| 3             | protocol mismatch, stdout empty, stderr becomes the error                                     |
| anything else | `cmdExecution.run` today: non-empty stdout is parsed, empty stdout is an error quoting stderr |

Diagnostics never change the exit code. A tool that exits non-zero on findings still works through the existing stdout
rule.

### 3.4 Behaviour rules

- Hermetic: the response is a function of the request and `config`. A checker reads no config files, env vars or
  dotfiles on its own. A tool that discovers config from the filesystem gets that disabled by its adapter, re-enabled
  only through an explicit `config` key.
- Deterministic order: diagnostics sorted by `snippet`, `line`, `column`, `code`.
- No network. File reads only under `working_directory` or the repo, every one listed in `inputs`.
- One process per workflow file. `VisitWorkflowPre` collects, `VisitWorkflowPost` sends and waits. The
  `concurrentProcess` semaphore still bounds parallelism across files.
- cwd is `ruleContext.projectRoot`, else `workingDir`. Cancelling `LinterOptions.Context` kills the child.
- Stderr is for humans. Shown only when the response is invalid.

## 4. `actionlint-plugin-ruff` on that contract

Per request: one `ruff check` process per snippet over stdin. `@argfile` batching can come later without touching the
contract.

Request → ruff:

| request / config                      | ruff flag                                                                                      |
| ------------------------------------- | ---------------------------------------------------------------------------------------------- |
| snippet `source`                      | stdin                                                                                          |
| `workflow` + snippet `id`             | `--stdin-filename <workflow>.<id>.py`, only to give the `.py` suffix and a readable `filename` |
| always, unless `project_config: true` | `--isolated`                                                                                   |
| `config.select` (default `["F"]`)     | `--select`                                                                                     |
| `config.ignore`                       | `--ignore`                                                                                     |
| `config.target_version`               | `--target-version`                                                                             |
| `config.extra_config` (TOML strings)  | `--config ...` each                                                                            |
| always                                | `--no-cache --output-format json --exit-zero --quiet -`                                        |

Ruff JSON → response:

| ruff                               | contract                   |
| ---------------------------------- | -------------------------- |
| `code` (fallback `name` when null) | `code`                     |
| `severity`                         | `severity`                 |
| `message`                          | `message`                  |
| `location.row/column`              | `line/column`              |
| `end_location.row/column`          | `end_line/end_column`      |
| `url`                              | `url`                      |
| `fix.message`, `fix.edits[]`       | `fix.description`, `edits` |

Drop a diagnostic whose range lies fully inside an `expressions` span.

`--isolated` is the default. With it the result depends on `config` alone, the same guarantee pyflakes gave.
`project_config: true` drops `--isolated`; ruff then walks up from the `--stdin-filename` path and applies the repo's
`pyproject.toml` or `ruff.toml`. The adapter still passes `--select` and `--ignore` from `config`, which override the
file. With `force-exclude = true` in that file a matching `exclude` silently yields zero diagnostics, so the adapter
passes `--no-force-exclude` whenever `project_config` is on.

Failure handling: ruff exit 2 or any `severity: "fatal"` → append to `errors`, keep diagnostics from the other snippets.
Ruff missing from `PATH` → exit 1 with one stderr line.

`actionlint.yaml`:

```yaml
tools:
  ruff:
    command: [actionlint-plugin-ruff]
    languages: [python]
    config:
      select: [F, E9]
      target_version: py312
      project_config: false
```

## 5. Why a process contract and not a Go plugin API

- actionlint is a static Go binary. Go plugins need an identical toolchain and are Linux-only. The process boundary
  keeps actionlint's build untouched.
- `OnRulesCreated` already serves Go consumers who want in-process rules.
- Checkers in any language. Ruff's own wasm is JS-only, so the adapter wraps the CLI anyway.
- One request per workflow keeps process count at `workflows × tools`, not `steps × tools`.
- Same contract for pyflakes-compat, pwsh `PSScriptAnalyzer`, future ones. shellcheck can migrate later.

## Unresolved

- Diagnostic fully inside `${{ }}` span: drop, or report with a note? Pick drop.
- Per-workflow request vs per-step (current shellcheck model)? Per-workflow fewer spawns, needs collect-then-send in the
  rule. Pick per-workflow.
- Code-level suppression `# actionlint:ignore ruff/F401`: extend `isInlineSuppressibleRule` for `<tool>/<code>`, or
  rule-level only? Rule-level works today; code-level is new grammar.
- `tools.<name>` synthesizing a `ruleDescriptor` vs one fixed `external-tool` rule that fans out. Synthesized keeps
  `lint.external.<name>` and `doctor` per tool. Pick synthesized.
- Move shellcheck onto the contract now or keep builtin? Builtin keeps `-shellcheck`, `tools.shellcheck.config`, `-x`
  intact. Keep builtin.
- `fix` field from day one, since `Error.fixes` and the gate exist, or drop until `-fix`? Keep.

[`5b27c1ccfaa230fb195855f5cf3404a21e3379cd`]: https://github.com/astral-sh/ruff/commit/5b27c1ccfaa230fb195855f5cf3404a21e3379cd
[`6567fe7a`]: https://github.com/kjanat/actionlint/commit/6567fe7a

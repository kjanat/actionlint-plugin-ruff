# Changelog

## [0.3.0] - 2026-11-08

### Changed

- Breaking change: component metadata uses `wit_sha256` for the WIT checksum. The registry rejects new releases that
  use `contract_sha256`.
- Upgrade `ruff_linter` to 0.17.4.
- Generate bindings from the public actionlint SDK at `99a250b29aaa`. Rule metadata no longer reports fix availability
  for an entire rule.

## [0.2.0] - 2026-10-29

### Changed

- Breaking change: the component exports `kjanat:actionlint/script-engine@0.2.0` for actionlint v2. Hosts that only
  support version `0.1.0` reject it.
- Generate bindings from the public actionlint SDK at `8836017f3c5150fcd6eba7b4ae64658b2d5c53e1`.

### Added

- Classify findings affected by masked expressions through `kjanat:actionlint/template-effects@0.2.0` and core `check`
  operation version 2.

## [0.1.0] - 2026-10-20

Initial release.

[0.3.0]: https://github.com/kjanat/actionlint-plugin-ruff/releases/tag/v0.3.0
[0.2.0]: https://github.com/kjanat/actionlint-plugin-ruff/releases/tag/v0.2.0
[0.1.0]: https://github.com/kjanat/actionlint-plugin-ruff/releases/tag/v0.1.0

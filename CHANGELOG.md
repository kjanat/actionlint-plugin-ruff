# Changelog

## [0.3.0] - 2026-11-08

### Changed

- **Breaking:** component metadata declares the WIT checksum as `wit_sha256`. The registry rejects new releases that declare `contract_sha256`.
- Update Ruff to `ruff_linter` 0.17.4.
- Build bindings from the public SDK vendored from actionlint `99a250b29aaa`. Metadata omits rule-wide fix availability.

## [0.2.0] - 2026-10-29

### Changed

- **Breaking:** export `kjanat:actionlint/script-engine@0.2.0` for actionlint v2 hosts; hosts supporting only `0.1.0` reject this component.
- Build bindings from the public SDK vendored from actionlint `8836017f3c5150fcd6eba7b4ae64658b2d5c53e1`.

### Added

- Classify findings on expression-masked snippets through `kjanat:actionlint/template-effects@0.2.0` and core `check` operation version 2.

## [0.1.0] - 2026-10-20

*First release.*

[0.3.0]: https://github.com/kjanat/actionlint-plugin-ruff/releases/tag/v0.3.0
[0.2.0]: https://github.com/kjanat/actionlint-plugin-ruff/releases/tag/v0.2.0
[0.1.0]: https://github.com/kjanat/actionlint-plugin-ruff/releases/tag/v0.1.0

# Changelog

All notable changes to this project will be documented in this file.

This format follows Keep a Changelog. The project uses Semantic Versioning for
the published CLI contract.

## [Unreleased]

### Added

- Release metadata for crates.io publishing
- CLI contract tests for help, version, usage errors, and environment checks
- GitHub Actions CI workflow for the release gates
- GitHub release workflow for tagged macOS binaries
- CLI writing guide and release checklist

### Changed

- Updated the project version to `0.1.0`
- Converted the crate to a CLI-only public product
- Added structured debug logging with `--verbose` and `RUST_LOG`
- Standardized error and status message wording

## [0.1.0] - 2026-04-06

### Added

- Initial public release candidate

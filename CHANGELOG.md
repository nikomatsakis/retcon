# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.1.0](https://github.com/nikomatsakis/retcon/compare/v1.0.0...v1.1.0) - 2026-05-03

### Added

- stream build/test command output through hooks

### Other

- bump to 1.1
- Fix multi-line commit messages in status line, add SKIP for stuck commits
- Add configurable agent via --agent flag and ~/.retcon/config.toml
- Restructure execution as TOML-persisted state machine
- Guide planning agent to write commit messages with WHY, not just WHAT
- Add Response history entry for auto-resolving stuck commits
- send diff --stat in prompts, let agent run git diff
- replace ratatui TUI with scrollback-based terminal output

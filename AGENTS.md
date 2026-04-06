# AGENTS.md

Update the AGENTS.md index after every task is completed.

## Fundamental Principle
For any updates to AGENTS.md, only the sections under Maintain by Robot should be updated; all other parts must not be modified.

## Maintain by Human
- Do clean code.
- As little code as possible.
- The code should be elegant and abstract.
- Don't touch .codex Cargo.toml.

## Maintain by Robot

### File Index And Description
- `README.md`: End-to-end user guide covering install, generate/run flow, schema, events, structured outputs, `on_max_retries` policy behavior, managed file safety, and `--force`/`--yes` overwrite confirmation behavior.
- `hook-bridge.yaml`: Local hook configuration that runs the stop-event verification and automated review gates for this repository.
- `Makefile`: Developer convenience targets for common local workflows such as build, test, lint, and review checks.
- `examples/basic.yaml`: Basic shared hooks for session start plus pre/post tool logging.
- `examples/claude-extended-events.yaml`: Claude-only native event examples for permission decisions, elicitation replies, notifications, subagent stop control, and teammate-idle feedback.
- `scripts/check_file_lines.sh`: Repository guard that safely scans Rust source and test files, including paths with whitespace, and fails when files exceed the configured line-count limit.
- `scripts/hooks/auto_review.py`: Git-aware review hook that gathers changed-file context, invokes Codex for automatic review, and enforces concise failure output.
- `scripts/hooks/review_prompt.md`: Review prompt template consumed by the automated review hook.
- `src/app.rs`: Application command dispatch, runtime abstraction wiring, and app-level routing tests.
- `src/cli.rs`: CLI argument definitions, including the default `hook-bridge.yaml` config path, optional single-platform filter for `generate`, and `--force`/`--yes` overwrite controls (`--yes` requires `--force`).
- `src/config/mod.rs`: Config module wiring that re-exports normalized config types, including retry-threshold policy enums, and enables config-focused tests.
- `src/config/schema.rs`: YAML schema definitions for top-level defaults, hooks, and platform overrides, including shared `on_max_retries` fields.
- `src/config/normalize.rs`: Config validation and normalization into platform-specific runtime rules, including validated `on_max_retries` policy inheritance and event-capability checks for `stop`/`block` when retries are enabled.
- `src/config/tests.rs`: Validation and normalization tests for config parsing rules, including `on_max_retries` inheritance, defaults, invalid-value rejection, and unsupported-event retry-guard validation.
- `src/error.rs`: Domain error types and stable process exit-code mapping for CLI and runtime failures.
- `src/generate/build.rs`: Converts normalized hooks into Claude/Codex managed hook handler JSON.
- `src/generate/managed.rs`: Managed-file metadata helpers, target-path mapping, runtime-cwd-aware path resolution, and conflict preflight checks for generated hook outputs.
- `src/generate/mod.rs`: Executes `generate`, including cwd-consistent config/target path normalization, target-platform selection, force-overwrite confirmation (`dialoguer`), non-interactive `--yes` enforcement, force-mode target replaceability checks (reject directory/non-file targets), conflict/writability preflight, and managed file writes.
- `src/generate/tests.rs`: Shared helpers and submodule wiring for generation tests.
- `src/generate/tests/generation_core.rs`: Generation tests for rule expansion, event mapping, and managed hook output fields.
- `src/generate/tests/managed_files.rs`: Generation tests for managed file conflicts, metadata validation, writable-target checks, and force-overwrite confirmation branches.
- `src/lib.rs`: Top-level program entrypoints, `run_cli`/`run_program` wiring, and program outcome helpers with library-level tests.
- `src/main.rs`: Binary entrypoint that runs the library program flow, emits buffered output, and returns the final exit code.
- `src/platform/capability.rs`: Platform capability matrix describing supported events, matcher availability, decision types, and native extra fields.
- `src/platform/claude.rs`: Claude payload parsing plus translation of bridge/runtime results into Claude-native hook outputs.
- `src/platform/claude_tests.rs`: Claude platform tests covering payload parsing and output translation behavior.
- `src/platform/codex.rs`: Codex payload parsing and translation of bridge/runtime results into Codex hook JSON output.
- `src/platform/mod.rs`: Shared platform types, event normalization, and dispatch to platform-specific output translators.
- `src/run/context.rs`: Runtime payload parsing into normalized execution context fields shared across platforms.
- `src/run/mod.rs`: Runtime hook execution, including policy-driven retry guards, non-zero exit handling, and formatted failure summaries.
- `src/run/retry.rs`: Retry-state storage, policy-driven retry-guard decisions, and failure persistence/reset keyed by config path, session, and rule.
- `src/run/tests.rs`: Shared fixtures and submodule wiring for runtime unit tests.
- `src/run/tests/command_output.rs`: Runtime tests for process execution inputs, output summaries, and bridge-output parsing.
- `src/run/tests/context_execute.rs`: Runtime tests for context parsing, execute-path validation, and shared helper behavior.
- `src/run/tests/retry_state.rs`: Runtime tests for retry-state persistence, `stop`/`block`/`allow_and_reset` guard behavior, and execute-rule retry updates.
- `src/runtime/clock.rs`: Clock abstraction with system and fixed implementations for deterministic retry-state tests.
- `src/runtime/fs.rs`: Filesystem abstraction implementations, current-directory lookup, metadata inspection (file/directory/other + readonly), atomic-write helpers, and filesystem-focused tests.
- `src/runtime/io.rs`: Stdio abstraction and helpers for reading stdin plus writing stdout/stderr with test doubles.
- `src/runtime/mod.rs`: Runtime trait and production wiring for filesystem, clock, process, and stdio dependencies.
- `src/runtime/process.rs`: Process runner implementation, child cleanup behavior, and process execution tests.
- `tests/cli_meta.rs`: CLI parsing and top-level parameter validation tests, including `generate --platform` plus `generate --force/--yes` parsing behavior and `--yes`-requires-`--force` validation.
- `tests/cli_generate.rs`: Shared helpers and submodule wiring for `generate` integration tests.
- `tests/cli_generate/basic.rs`: Core `generate` integration tests for default config handling, managed writes, force overwrite behavior (`--force`/`--yes`), directory-target replaceability failures, and no-partial-output conflicts.
- `tests/cli_generate/platform_filter.rs`: `generate --platform` integration tests covering selected-target writes, conflict scope, and untouched unselected files.
- `tests/cli_generate/events_and_errors.rs`: `generate` integration tests for config error exit codes and native event-name output behavior.
- `tests/cli_run.rs`: Shared helpers and submodule wiring for `run` integration tests.
- `tests/cli_run/basic.rs`: `run` integration tests for core execution, managed config resolution, and basic failure handling.
- `tests/cli_run/platform_outputs.rs`: `run` integration tests for Claude/Codex protocol translations and payload validation.
- `tests/cli_run/retry_policy.rs`: `run` integration tests for `on_max_retries` policy behavior, including `stop` fallback, `block`, `allow_and_reset`, and invalid stop-only or side-effect-only event combinations.
- `tests/cli_run/retry_state.rs`: `run` integration tests for retry-state isolation, persistence/reset semantics, and translate-time failure tracking.
- `tests/cli_run/stop_and_feedback.rs`: `run` integration tests for stop-event summaries and Claude exit-code-two feedback behavior.
- `tests/runtime_fs.rs`: Filesystem integration tests covering `OsFileSystem`/`FakeFileSystem` metadata behavior for symlink/invalid-parent/existing-path branches.
- `examples/platform-overrides.yaml`: Shared-hook example showing Claude/Codex-specific commands, protocol overrides, and disabled platform mappings.
- `examples/retry-and-env.yaml`: Example config demonstrating merged environment variables, retry-count and `on_max_retries` overrides, and absolute working directory behavior.
- `examples/stop-hooks.yaml`: Platform-specific stop-event examples for Claude and Codex native stop responses.

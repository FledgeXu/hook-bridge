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
- `src/cli.rs`: CLI argument definitions, including the default `hook-bridge.yaml` config path for `generate`.
- `src/config/schema.rs`: YAML schema definitions for top-level defaults, hooks, and platform overrides.
- `src/config/normalize.rs`: Config validation and normalization into platform-specific runtime rules.
- `src/config/tests.rs`: Validation and normalization tests for config parsing rules.
- `src/generate/build.rs`: Converts normalized hooks into Claude/Codex managed hook handler JSON.
- `src/generate/tests.rs`: Generation tests for managed hook output structure and field mapping.
- `src/lib.rs`: Top-level program entrypoints, `run_cli`/`run_program` wiring, and program outcome helpers with library-level tests.
- `src/platform/claude.rs`: Claude payload parsing plus translation of bridge/runtime results into Claude-native hook outputs.
- `src/platform/codex.rs`: Codex payload parsing and translation of bridge/runtime results into Codex hook JSON output.
- `src/platform/mod.rs`: Shared platform types, event normalization, and dispatch to platform-specific output translators.
- `src/run/mod.rs`: Runtime hook execution, including non-zero exit handling and formatted failure summaries.
- `src/run/tests.rs`: Unit tests for command execution results, multi-section failure-summary formatting, and retry-related runtime behavior.
- `src/runtime/fs.rs`: Filesystem abstraction implementations plus atomic-write helpers and filesystem-focused tests.
- `src/runtime/process.rs`: Process runner implementation, child cleanup behavior, and process execution tests.
- `tests/cli_meta.rs`: CLI parsing and top-level parameter validation tests.
- `tests/cli_generate.rs`: Integration tests for `generate`, including default config path behavior.
- `tests/cli_run.rs`: Integration tests for `run`, including precise `reason` and retry-state assertions for command failures.
- `examples/platform-overrides.yaml`: Example config showing platform overrides and shared hook fields like `status_message`.

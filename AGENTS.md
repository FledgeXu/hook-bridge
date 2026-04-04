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
- Implemented `impl-docs/hook-bridge-implementation/01-project-bootstrap-and-test-harness.md` project bootstrap, CLI skeleton, runtime side-effect abstractions, unified error model, smoke tests, and coverage gate command.
- Static-check and CLI regression fixes (file index): `src/main.rs`, `src/lib.rs`, `src/app.rs`, `src/generate/mod.rs`, `src/run/mod.rs`, `src/runtime/fs.rs`, `src/runtime/io.rs`, `src/runtime/process.rs`, `tests/cli_generate.rs`, `tests/cli_run.rs`, `tests/cli_meta.rs`.
- Full implementation pass (file index): `src/config/mod.rs`, `src/platform/mod.rs`, `src/platform/capability.rs`, `src/platform/claude.rs`, `src/platform/codex.rs`, `src/generate/mod.rs`, `src/run/mod.rs`, `src/runtime/mod.rs`, `src/runtime/fs.rs`, `src/runtime/io.rs`, `src/runtime/process.rs`, `src/cli.rs`, `src/app.rs`, `tests/cli_generate.rs`, `tests/cli_run.rs`, `Makefile`.
- Retry/safety/runtime fixes (file index): `src/run/mod.rs`, `src/config/mod.rs`, `tests/cli_run.rs`.
- Process pipe-drain and clippy-gate fixes (file index): `src/runtime/process.rs`, `tests/cli_run.rs`.
- Stdin EOF and atomic temp-file race fixes (file index): `src/runtime/process.rs`, `src/runtime/fs.rs`, `AGENTS.md`.
- Runtime error propagation and temp-dir abstraction fixes (file index): `src/run/mod.rs`, `src/runtime/mod.rs`, `src/app.rs`, `AGENTS.md`.
- Absolute source-config and honest coverage gate fixes (file index): `src/generate/mod.rs`, `src/run/mod.rs`, `tests/cli_run.rs`, `Makefile`, `AGENTS.md`.
- Protocol-output-on-process-failure and managed-version-check fixes (file index): `src/run/mod.rs`, `src/generate/mod.rs`, `tests/cli_run.rs`, `AGENTS.md`.
- Child cleanup and exists-error-semantics fixes (file index): `src/runtime/process.rs`, `src/runtime/fs.rs`, `AGENTS.md`.
- Coverage-gate alignment fix (file index): `Makefile`, `README.md`, `AGENTS.md`.
- Retry-state cross-project isolation fix (file index): `src/run/mod.rs`, `tests/cli_run.rs`, `AGENTS.md`.
- Protocol field compatibility and gate rollback fixes (file index): `src/platform/codex.rs`, `src/platform/claude.rs`, `src/run/mod.rs`, `tests/cli_run.rs`, `Makefile`, `README.md`, `AGENTS.md`.
- Config model and YAML validation implementation for phase 02 (file index): `src/config/mod.rs`, `AGENTS.md`.
- Static-check follow-up fixes: clippy `never_loop`/test-lint cleanup, top-level `enabled` rejection test, and AGENTS constraint alignment (file index): `src/config/mod.rs`, `AGENTS.md`.

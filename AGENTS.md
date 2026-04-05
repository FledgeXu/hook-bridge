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
- `src/config/schema.rs`: YAML schema definitions for top-level defaults, hooks, and platform overrides.
- `src/config/normalize.rs`: Config validation and normalization into platform-specific runtime rules.
- `src/config/tests.rs`: Validation and normalization tests for config parsing rules.
- `src/generate/build.rs`: Converts normalized hooks into Claude/Codex managed hook handler JSON.
- `src/generate/tests.rs`: Generation tests for managed hook output structure and field mapping.
- `examples/platform-overrides.yaml`: Example config showing platform overrides and shared hook fields like `status_message`.

# hook_bridge

`hook_bridge` keeps Claude Code and Codex hooks in one YAML file. You define rules once, generate the native managed files, and let `hook_bridge run` translate each incoming hook payload back into the platform-specific response format.

## Why It Exists

- One config for both platforms.
- One runtime for validation, command execution, retries, and output translation.
- Shared event aliases for common flows such as session start and tool hooks.
- Platform-specific overrides when Claude and Codex need different commands, events, or response fields.
- Managed output files so generated hooks can be safely regenerated.

## Install

Install from the current checkout:

```bash
cargo install --path .
```

For development:

```bash
cargo build
```

## Commands

Generate managed hook files from YAML:

```bash
hook_bridge generate
hook_bridge generate --config ./hook-bridge.yaml
hook_bridge generate --config ./hook-bridge.yaml --platform claude
hook_bridge generate --config ./hook-bridge.yaml --platform codex
hook_bridge generate --force
hook_bridge generate --force --yes
```

Run a generated rule handler:

```bash
hook_bridge run --platform claude --rule-id guard.destructive.commands
hook_bridge run --platform codex --rule-id log.post_tool
```

Defaults:

- Default config path: `hook-bridge.yaml`
- Claude output: `.claude/settings.json`
- Codex output: `.codex/hooks.json`

## Quick Start

Create `hook-bridge.yaml`:

```yaml
version: 1

defaults:
  shell: sh
  timeout_sec: 15

hooks:
  - id: guard.destructive.commands
    description: Block obviously destructive shell commands before execution.
    event: before_command
    matcher: "^(rm(\\s+-[A-Za-z-]*[rf][A-Za-z-]*)?\\b|git\\s+reset\\s+--hard\\b)"
    command: |
      printf '%s\n' 'dangerous command rejected by hook_bridge'
      exit 1
    platforms:
      claude:
        decision: block
        reason: Dangerous shell command rejected by policy.
      codex:
        systemMessage: Use a non-destructive command or ask for approval.
```

Generate the native files:

```bash
hook_bridge generate
```

This writes:

- `.claude/settings.json`
- `.codex/hooks.json`

Each generated handler runs:

```bash
hook_bridge run --platform <claude|codex> --rule-id <rule-id>
```

At runtime the platform sends JSON to `stdin`, `hook_bridge` executes your shell command, then returns the appropriate Claude or Codex hook output.

## Config Schema

Supported top-level shape:

```yaml
version: 1

defaults:
  shell: sh
  timeout_sec: 30
  max_retries: 0
  on_max_retries: stop
  working_dir: /absolute/path

hooks:
  - id: example.rule
    description: Optional note
    status_message: Optional platform-visible status text
    event: before_command
    matcher: ".*"
    shell: sh
    timeout_sec: 30
    max_retries: 0
    on_max_retries: stop
    working_dir: /absolute/path
    env:
      KEY: value
    command: echo ok
    platforms:
      claude:
        enabled: true
        event: PreToolUse
        matcher: "Bash"
        command: echo claude
        env:
          PLATFORM_NAME: claude
        decision: block
        reason: Claude-specific reason
      codex:
        enabled: true
        event: PreToolUse
        command: echo codex
        env:
          PLATFORM_NAME: codex
        continue: true
        stopReason: Optional stop reason
        systemMessage: Optional Codex system message
```

Normalization rules:

- `version` must be `1`.
- `hooks` must not be empty.
- `id` must be unique and match `[A-Za-z0-9._-]+`.
- `command` must not be empty.
- Unknown top-level or rule fields are rejected.
- Shared fields inherit from `defaults`, then from the rule, then from `platforms.<name>`.
- `env` merges as `rule env` plus platform env overriding duplicate keys.
- `platforms.<name>.enabled: false` disables that platform mapping for the rule.
- Platform-specific fields such as Claude `decision` / `reason` or Codex `continue` / `stopReason` / `systemMessage` must live inside the matching platform block.

## Shared Fields

- `shell`: command runner, executed as `<shell> -lc '<command>'`
- `timeout_sec`: command timeout in seconds
- `max_retries`: retry allowance for repeated failures
- `on_max_retries`: post-threshold retry policy: `stop`, `block`, or `allow_and_reset`
  When `max_retries > 0`, the selected policy must be representable by that platform event.
- `working_dir`: absolute directory override; if omitted, hook payload cwd is used when present
- `matcher`: only valid for events that support matching on that platform
- `status_message`: optional generated-hook status text

## Events

Unified aliases accepted in config:

- `session_start` -> `SessionStart`
- `before_command` -> `PreToolUse`
- `after_command` -> `PostToolUse`

Codex events:

- `SessionStart`
- `PreToolUse`
- `PostToolUse`
- `UserPromptSubmit`
- `Stop`

Claude events:

- `SessionStart`
- `InstructionsLoaded`
- `UserPromptSubmit`
- `PreToolUse`
- `PermissionRequest`
- `PermissionDenied`
- `PostToolUse`
- `PostToolUseFailure`
- `Notification`
- `SubagentStart`
- `SubagentStop`
- `TaskCreated`
- `TaskCompleted`
- `Stop`
- `StopFailure`
- `TeammateIdle`
- `ConfigChange`
- `CwdChanged`
- `FileChanged`
- `WorktreeCreate`
- `WorktreeRemove`
- `PreCompact`
- `PostCompact`
- `SessionEnd`
- `Elicitation`
- `ElicitationResult`

Matcher support:

- Codex: `SessionStart`, `PreToolUse`, `PostToolUse`
- Claude: `SessionStart`, `PreToolUse`, `PermissionRequest`, `PostToolUse`, `PostToolUseFailure`, `Notification`, `SubagentStart`, `SubagentStop`, `Elicitation`, `ElicitationResult`

If a rule uses an event or matcher combination the platform does not support, generation fails during normalization.

## Runtime Model

`hook_bridge run` performs this flow:

1. Load managed metadata from the generated native file.
2. Reopen the original source config from `_hook_bridge.source_config`.
3. Read the raw platform payload from `stdin`.
4. Parse and validate the runtime context.
5. Find the selected rule by `--platform` and `--rule-id`.
6. Confirm the incoming event matches the configured event.
7. Execute the command with the raw payload on `stdin`.
8. Translate the result into Claude or Codex native hook output.

Your command receives:

- Raw incoming JSON on `stdin`
- `HOOK_BRIDGE_PLATFORM`
- `HOOK_BRIDGE_EVENT`
- `HOOK_BRIDGE_RULE_ID`

## Returning Results From Commands

If your command exits `0` and prints plain text:

- On Codex `SessionStart` and `UserPromptSubmit`, plain text is promoted to additional context.
- On other events, plain text is treated as ordinary command output and does not become a structured decision.

For explicit structured behavior, print JSON on `stdout`:

```json
{"hook_bridge":{"kind":"block","reason":"blocked by policy","system_message":"ask for approval first"}}
```

Supported `hook_bridge.kind` values:

- `success`
- `block`
- `stop`
- `additional_context`
- `permission_decision`
- `permission_retry`
- `worktree_path`
- `elicitation_response`
- `error`

Examples:

```json
{"hook_bridge":{"kind":"additional_context","text":"Load workspace conventions before editing."}}
```

```json
{"hook_bridge":{"kind":"stop","reason":"Add a short final summary before stopping.","system_message":"Summarize changed files and verification status before exit."}}
```

```json
{"hook_bridge":{"kind":"permission_decision","behavior":"deny","reason":"Interactive shell access is disabled.","additional_context":"Use a non-interactive command or ask for approval."}}
```

## Non-Zero Exit Behavior

If your command exits non-zero and does not emit a valid structured result, `hook_bridge` converts the failure into a platform-native block or stop-style response when possible. The summary includes:

- Exit code
- Original command
- Tail of `stderr`
- Tail of `stdout`

## Retry Behavior

- `max_retries` defaults to `0`.
- `on_max_retries` defaults to `stop`.
- Retry state is tracked per runtime context.
- Repeated failures can trigger a retry guard before the command is executed again.
- `stop` preserves current behavior: short-circuit execution and emit a stop result, degrading to block on events that cannot stop.
- `block` short-circuits execution and always emits a block result, so it is only valid on events whose platform protocol supports block decisions.
- `allow_and_reset` short-circuits execution, returns normal success output, and clears retry state immediately.
- Events that support neither `stop` nor `block` cannot use retry guards; configs with `max_retries > 0` for those events are rejected during normalization.
- Structured `stop` results are not treated as failures for retry-state purposes.

## Managed File Safety

By default, `generate` only overwrites files already managed by `hook_bridge`.

If either target file already exists without `_hook_bridge.managed_by = "hook_bridge"`, generation fails with a conflict and does not partially update the other platform file.

Use `--force` to allow overwriting non-managed target files.

- Interactive terminal (`stdin` and `stderr` are TTY): `--force` asks for a single confirmation (`Proceed with force overwrite?`), default is deny.
- Non-interactive environment: `--force` must be paired with `--yes` or generation fails with a parameter error.
- `--yes` is only valid together with `--force`.

## Example Configs

- `examples/basic.yaml`: Basic shared hooks for session start plus pre/post tool logging.
- `examples/platform-overrides.yaml`: Shared rules with platform-specific commands, decisions, and disabled mappings.
- `examples/retry-and-env.yaml`: Merged environment variables, retry settings, and fixed working directory behavior.
- `examples/claude-extended-events.yaml`: Claude-only native events including permission, elicitation, notification, and teammate handling.
- `examples/stop-hooks.yaml`: Stop-event responses for Claude and Codex with native stop semantics.

## Development

```bash
make test
make coverage
make verify
```

Notes:

- `make coverage` requires `cargo-llvm-cov`.
- `make verify` runs formatting, clippy, tests, and coverage checks defined by the project.

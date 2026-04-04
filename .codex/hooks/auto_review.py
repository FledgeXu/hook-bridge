#!/usr/bin/env python3
from contextlib import suppress
import hashlib
import json
import os
import stat
import subprocess
import sys
import tempfile
from pathlib import Path


ENABLE_ENV = "CODEX_AUTO_REVIEW"
MAX_ROUNDS_ENV = "CODEX_AUTO_REVIEW_MAX_ROUNDS"
CHILD_ENV = "CODEX_AUTO_REVIEW_CHILD"
INCLUDE_UNTRACKED_CONTENT_ENV = "CODEX_AUTO_REVIEW_INCLUDE_UNTRACKED_CONTENT"
MAX_REVIEW_INPUT_CHARS = 120000

HOOK_DIR = Path(__file__).resolve().parent
SCHEMA_PATH = HOOK_DIR / "review_schema.json"
PROMPT_PATH = HOOK_DIR / "review_prompt.md"
DEFAULT_CODE_PATTERNS = (
    ".py,.pyi,.ipynb,.js,.jsx,.mjs,.cjs,.ts,.tsx,.vue,.svelte,.go,.rs,.java,"
    ".kt,.kts,.swift,.c,.cc,.cpp,.cxx,.h,.hh,.hpp,.hxx,.cs,.fs,.fsi,.fsx,.vb,"
    ".rb,.php,.m,.mm,.scala,.sc,.sh,.bash,.zsh,.ps1,.psm1,.lua,.pl,.pm,.r,.R,"
    ".dart,.erl,.hrl,.ex,.exs,.elm,.clj,.cljs,.cljc,.groovy,.gradle,.tf,"
    ".tfvars,.hcl,.json,.jsonc,.yaml,.yml,.toml,.ini,.cfg,.conf,.properties,"
    ".envrc,.sql,Dockerfile,Makefile,Justfile,CMakeLists.txt,Jenkinsfile,"
    "Gemfile,Podfile,Vagrantfile"
)
DEFAULT_UNTRACKED_CONTENT_PATTERNS = (
    ".py,.pyi,.js,.jsx,.mjs,.cjs,.ts,.tsx,.vue,.svelte,.go,.rs,.java,.kt,.kts,"
    ".swift,.c,.cc,.cpp,.cxx,.h,.hh,.hpp,.hxx,.cs,.fs,.fsi,.fsx,.vb,.rb,.php,"
    ".m,.mm,.scala,.sc,.sh,.bash,.zsh,.ps1,.psm1,.lua,.pl,.pm,.r,.dart,.erl,"
    ".hrl,.ex,.exs,.elm,.clj,.cljs,.cljc,.groovy,.gradle,Dockerfile,Makefile,"
    "Justfile,CMakeLists.txt,Jenkinsfile"
)
IGNORED_FILES = {"package-lock.json"}
REVIEW_TRIGGER_FILES = {".codex/hooks/review_prompt.md"}
PROMPT_RULES = """
# 输出要求

- 必须严格符合给定 JSON Schema。
- `summary`、`title`、`explanation`、`fix_hint` 全部使用中文。
- `findings` 保持简洁、可执行、可定位。
- 你必须输出 `verdict`，且只能是 `pass` 或 `block`。
- `verdict=pass` 时，`findings` 必须为空。
- `verdict=block` 时，`findings` 必须至少包含一个可确认的问题。
""".strip()


def pattern_sets(patterns: str) -> tuple[set[str], set[str]]:
    names, suffixes = set(), set()
    for item in patterns.split(","):
        pattern = item.strip().lower()
        if pattern:
            (suffixes if pattern.startswith(".") else names).add(pattern)
    return names, suffixes


CODE_PATTERNS = pattern_sets(DEFAULT_CODE_PATTERNS)
UNTRACKED_CONTENT_PATTERNS = pattern_sets(DEFAULT_UNTRACKED_CONTENT_PATTERNS)


def state_path(cwd: str) -> Path:
    key = hashlib.sha256(str(Path(cwd).resolve()).encode("utf-8")).hexdigest()[:16]
    return Path(tempfile.gettempdir()) / "codex-auto-review" / f"{key}.json"


def emit(payload: dict) -> int:
    print(json.dumps(payload, ensure_ascii=False))
    return 0


def allow() -> int:
    return emit({"continue": True})


def block(reason: str) -> int:
    return emit({"decision": "block", "reason": reason})


def stop(message: str) -> int:
    return emit(
        {
            "continue": False,
            "stopReason": "automated review blocked stop",
            "systemMessage": message,
        }
    )


def env_enabled(name: str, default: bool = True) -> bool:
    value = os.environ.get(name)
    return default if value is None else value.strip().lower() not in {"", "0", "false", "no", "off"}


def env_int(name: str, default: int) -> int:
    try:
        return int(os.environ.get(name, default))
    except ValueError:
        return default


def auto_review_enabled() -> bool:
    return env_enabled(ENABLE_ENV)


def is_child_process() -> bool:
    return env_enabled(CHILD_ENV, default=False)


def include_untracked_content() -> bool:
    return env_enabled(INCLUDE_UNTRACKED_CONTENT_ENV, default=False)


def max_review_rounds() -> int:
    return max(1, env_int(MAX_ROUNDS_ENV, 3))


def run(
    args: list[str],
    cwd: str,
    input_text: str | None = None,
    extra_env: dict | None = None,
) -> subprocess.CompletedProcess[str]:
    env = os.environ.copy()
    if extra_env:
        env.update(extra_env)
    return subprocess.run(
        args,
        cwd=cwd,
        text=True,
        input=input_text,
        capture_output=True,
        env=env,
    )


def read_state(cwd: str) -> dict[str, int]:
    try:
        data = json.loads(state_path(cwd).read_text(encoding="utf-8"))
    except Exception:
        return {}
    if not isinstance(data, dict):
        return {}
    return {
        str(key): value
        for key, value in data.items()
        if isinstance(value, int) and value >= 0
    }


def write_state(cwd: str, state: dict[str, int]) -> None:
    path = state_path(cwd)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(state, ensure_ascii=False, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def clear_turn(cwd: str, turn_id: str) -> None:
    state = read_state(cwd)
    if turn_id not in state:
        return
    del state[turn_id]
    if state:
        write_state(cwd, state)
    else:
        with suppress(Exception):
            state_path(cwd).unlink(missing_ok=True)


def set_turn_round(cwd: str, turn_id: str, round_count: int) -> None:
    state = read_state(cwd)
    state[turn_id] = round_count
    write_state(cwd, state)


def git_paths(cwd: str, args: list[str], error_message: str) -> list[str]:
    proc = run(args, cwd)
    if proc.returncode != 0:
        raise RuntimeError(proc.stderr.strip() or error_message)
    return [item for item in proc.stdout.split("\0") if item]


def git_name_status_entries(cwd: str, staged: bool) -> list[tuple[str, ...]]:
    args = ["git", "diff", "--name-status", "-z", "--find-renames", "--", "."]
    if staged:
        args = ["git", "diff", "--cached", "--name-status", "-z", "--find-renames", "--", "."]
    items = git_paths(cwd, args, f"{' '.join(args[:3])} failed")
    entries, i = [], 0
    while i < len(items):
        status = items[i]
        i += 1
        if status[:1] in {"R", "C"} and i + 1 < len(items):
            entries.append((status, items[i], items[i + 1]))
            i += 2
        elif i < len(items):
            entries.append((status, items[i]))
            i += 1
    return entries


def is_ignored_path(path: str) -> bool:
    return bool(path) and Path(path).name.lower() in IGNORED_FILES


def matches_path(path: str, names: set[str], suffixes: set[str]) -> bool:
    if not path or is_ignored_path(path):
        return False
    name = Path(path).name.lower()
    return name in names or any(name.endswith(suffix) for suffix in suffixes)


def is_review_path(path: str, names: set[str], suffixes: set[str]) -> bool:
    return path in REVIEW_TRIGGER_FILES or matches_path(path, names, suffixes)


def tracked_changed_paths(cwd: str, staged: bool) -> list[str]:
    seen, paths = set(), []
    args = ["git", "diff", "--name-only", "-z", "--", "."]
    if staged:
        args = ["git", "diff", "--cached", "--name-only", "-z", "--", "."]
    for path in git_paths(cwd, args, f"{' '.join(args[:3])} failed"):
        if not path or is_ignored_path(path) or path in seen:
            continue
        seen.add(path)
        paths.append(path)
    return paths


def untracked_paths(cwd: str) -> list[str]:
    return [
        path
        for path in git_paths(
            cwd,
            ["git", "ls-files", "-z", "--others", "--exclude-standard"],
            "git ls-files failed",
        )
        if path and not is_ignored_path(path)
    ]


def has_reviewable_changes(cwd: str) -> bool:
    code_names, code_suffixes = CODE_PATTERNS
    for staged in (False, True):
        for entry in git_name_status_entries(cwd, staged):
            if any(is_review_path(path, code_names, code_suffixes) for path in entry[1:]):
                return True
    if not include_untracked_content():
        return False
    untracked_names, untracked_suffixes = UNTRACKED_CONTENT_PATTERNS
    return any(is_review_path(path, untracked_names, untracked_suffixes) for path in untracked_paths(cwd))


def escape_review_input(text: str) -> str:
    return text.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def append_limited(parts: list[str], chunk: str, notice: str | None = None) -> bool:
    size = len("".join(parts))
    escaped = escape_review_input(chunk)
    if size + len(escaped) <= MAX_REVIEW_INPUT_CHARS:
        parts.append(escaped)
        return True
    if not notice:
        return False
    escaped_notice = escape_review_input(notice)
    remaining = MAX_REVIEW_INPUT_CHARS - size
    if remaining <= len(escaped_notice):
        return False
    low, high = 0, len(chunk)
    while low < high:
        mid = (low + high + 1) // 2
        if len(escape_review_input(chunk[:mid])) <= remaining - len(escaped_notice):
            low = mid
        else:
            high = mid - 1
    if low:
        parts.append(escape_review_input(chunk[:low]))
    parts.append(escaped_notice)
    return False


def code_fence_for(content: str) -> str:
    longest = current = 0
    for char in content:
        if char == "`":
            current += 1
            longest = max(longest, current)
        else:
            current = 0
    return "`" * max(3, longest + 1)


def read_text_up_to(path: Path, max_chars: int) -> tuple[str, bool]:
    if max_chars <= 0:
        return "", True
    chunks, total = [], 0
    with path.open("r", encoding="utf-8") as handle:
        while total < max_chars:
            chunk = handle.read(min(8192, max_chars - total))
            if not chunk:
                return "".join(chunks), False
            chunks.append(chunk)
            total += len(chunk)
        return "".join(chunks), bool(handle.read(1))


def safe_untracked_path(repo_root: Path, rel: str) -> tuple[Path | None, str | None]:
    path = repo_root / rel
    try:
        st = path.lstat()
    except Exception as exc:
        return None, f"<<skipped: failed to stat file: {exc}>>\n"
    if stat.S_ISLNK(st.st_mode):
        return None, "<<skipped: symlink is not allowed>>\n"
    if not stat.S_ISREG(st.st_mode):
        return None, "<<skipped: non-regular file is not allowed>>\n"
    if st.st_nlink > 1:
        return None, "<<skipped: hard-linked file is not allowed>>\n"
    try:
        resolved = path.resolve(strict=True)
        resolved.relative_to(repo_root.resolve(strict=True))
    except Exception:
        return None, "<<skipped: file resolves outside repository>>\n"
    return resolved, None


def render_untracked_file(parts: list[str], path: Path) -> str:
    budget = max(0, MAX_REVIEW_INPUT_CHARS - len("".join(parts)) - 16)
    while True:
        content, truncated = read_text_up_to(path, budget)
        fence = code_fence_for(content)
        suffix = "\n<<truncated: file content clipped>>" if truncated else ""
        block = f"{fence}\n{content}\n{fence}{suffix}\n"
        remaining = MAX_REVIEW_INPUT_CHARS - len("".join(parts))
        if budget <= 0:
            return "<<skipped: file content omitted due to review input limit>>\n"
        if len(block) <= remaining:
            return block
        budget = max(0, budget - max(len(block) - remaining, 256))


def append_untracked_code_changes(
    parts: list[str],
    cwd: str,
    files: list[str],
    names: set[str],
    suffixes: set[str],
) -> list[str]:
    files = [path for path in files if matches_path(path, names, suffixes)]
    if not files or not append_limited(parts, "\n## Untracked code files\n"):
        return []

    included, repo_root = [], Path(cwd).resolve()
    for rel in files[:200]:
        if not append_limited(parts, f"\n### {rel}\n", "<<truncated: more untracked code files omitted>>\n"):
            break
        safe_path, note = safe_untracked_path(repo_root, rel)
        if note:
            append_limited(parts, note, "<<truncated: more untracked code files omitted>>\n")
            included.append(rel)
            continue
        if safe_path is None:
            included.append(rel)
            continue
        try:
            append_limited(
                parts,
                render_untracked_file(parts, safe_path),
                "<<truncated: file content clipped>>\n",
            )
        except UnicodeDecodeError:
            append_limited(parts, "<<skipped: non-utf8 file>>\n", "<<truncated: more untracked code files omitted>>\n")
        except Exception as exc:
            append_limited(
                parts,
                f"<<skipped: failed to read file: {exc}>>\n",
                "<<truncated: more untracked code files omitted>>\n",
            )
        included.append(rel)
        if len("".join(parts)) >= MAX_REVIEW_INPUT_CHARS:
            break
    return included


def append_diff_section(parts: list[str], cwd: str, title: str, staged: bool) -> bool:
    paths = tracked_changed_paths(cwd, staged)
    if not paths:
        return True
    diff = run(["git", "diff", *(["--cached"] if staged else []), "--", *paths], cwd)
    if diff.returncode != 0:
        raise RuntimeError(diff.stderr.strip() or "git diff failed")
    if not diff.stdout.strip():
        return True
    if parts:
        append_limited(parts, "\n")
    return append_limited(parts, title) and append_limited(parts, diff.stdout, "<<truncated: diff content clipped>>\n")


def collect_changes(cwd: str) -> str:
    parts: list[str] = []
    for title, staged in (("## Unstaged changes\n", False), ("## Staged changes\n", True)):
        if not append_diff_section(parts, cwd, title, staged):
            break

    files = untracked_paths(cwd)
    included = []
    if include_untracked_content():
        included = append_untracked_code_changes(parts, cwd, files, *UNTRACKED_CONTENT_PATTERNS)
    skipped = [path for path in files if path not in included]
    if skipped and append_limited(parts, "\n## Skipped untracked files\n"):
        append_limited(
            parts,
            "Only selected untracked source files above are sent with content; the files below are listed by name only.\n",
        )
        for rel in skipped[:200]:
            if not append_limited(parts, f"- {rel}\n"):
                append_limited(parts, "<<truncated: more skipped untracked files omitted>>\n")
                break
    return "".join(parts).strip()


def run_review(
    cwd: str,
    review_input: str,
    last_assistant_message: str,
) -> subprocess.CompletedProcess[str]:
    message = (last_assistant_message or "").strip()
    context = (
        ""
        if not message
        else "<last_assistant_message>\n"
        f"{escape_review_input(message)}\n"
        "</last_assistant_message>\n\n"
    )
    return run(
        ["codex", "exec", "--disable", "codex_hooks", "--output-schema", str(SCHEMA_PATH), "-"],
        cwd,
        input_text=f"{PROMPT_PATH.read_text(encoding='utf-8').strip()}\n\n{PROMPT_RULES}\n\n{context}<changes>\n{review_input}\n</changes>\n",
        extra_env={CHILD_ENV: "1"},
    )


def format_review(review: dict) -> str:
    lines = ["## 结论", "", f"`{review.get('verdict', 'unknown')}`"]
    if review.get("summary"):
        lines += ["", "## 总结", "", str(review["summary"])]
    findings = review.get("findings") or []
    if findings:
        lines.append("")
        lines.append("## 问题")
    for index, item in enumerate(findings, start=1):
        if not isinstance(item, dict):
            continue
        lines += [
            "",
            f"{index}. `[{item.get('severity', 'unknown')}]` `{item.get('file', '<unknown>')}`: {item.get('title', 'Untitled finding')}",
        ]
        if item.get("explanation"):
            lines += ["", f"说明：{item['explanation']}"]
        if item.get("fix_hint"):
            lines += ["", f"修复建议：{item['fix_hint']}"]
    return "\n".join(lines).strip() or json.dumps(review, ensure_ascii=False, indent=2)


def finalize_review(cwd: str, turn_id: str, review: dict) -> int:
    if review["verdict"] == "pass":
        clear_turn(cwd, turn_id)
        return allow()

    current_round = read_state(cwd).get(turn_id, 0)
    message = format_review(review)[:12000]
    if current_round >= max_review_rounds():
        clear_turn(cwd, turn_id)
        return allow()

    next_round = current_round + 1
    set_turn_round(cwd, turn_id, next_round)
    return block(
        "# 自动审查发现问题\n\n"
        "请先修复下面的问题，再继续当前任务。\n\n"
        f"- 重试轮次：`{next_round}/{max_review_rounds()}`\n"
        f"\n{message}"
    )


def review_result(
    cwd: str,
    turn_id: str,
    review_input: str,
    last_assistant_message: str,
) -> int:
    try:
        review_proc = run_review(cwd, review_input, last_assistant_message)
    except Exception as exc:
        clear_turn(cwd, turn_id)
        return stop(f"自动审查配置错误：读取 review 模版失败。\n\n{exc}")
    if review_proc.returncode != 0:
        clear_turn(cwd, turn_id)
        return stop(
            "自动审查进程执行失败。\n\n"
            f"stdout:\n{review_proc.stdout[:4000]}\n\n"
            f"stderr:\n{review_proc.stderr[:4000]}"
        )
    try:
        review = json.loads((review_proc.stdout or "").strip())
    except json.JSONDecodeError as exc:
        clear_turn(cwd, turn_id)
        return stop(
            "自动审查返回了无效 JSON。\n\n"
            f"解析错误: {exc}\n\n"
            f"原始输出:\n{(review_proc.stdout or '')[:6000]}"
        )
    return finalize_review(cwd, turn_id, review)


def parse_payload() -> tuple[str, str, str]:
    payload = json.load(sys.stdin)
    return payload["cwd"], str(payload.get("turn_id") or "unknown-turn"), str(payload.get("last_assistant_message") or "")


def main() -> int:
    cwd, turn_id, last_assistant_message = parse_payload()
    if not auto_review_enabled() or is_child_process():
        clear_turn(cwd, turn_id)
        return allow()

    try:
        review_input = "" if not has_reviewable_changes(cwd) else collect_changes(cwd)
    except Exception as exc:
        clear_turn(cwd, turn_id)
        return stop(f"自动审查在收集改动时失败。\n\n{exc}")

    if not review_input:
        clear_turn(cwd, turn_id)
        return allow()
    if not SCHEMA_PATH.is_file():
        clear_turn(cwd, turn_id)
        return stop(f"自动审查配置错误：缺少 schema 文件 {SCHEMA_PATH}")
    return review_result(cwd, turn_id, review_input, last_assistant_message)


if __name__ == "__main__":
    raise SystemExit(main())

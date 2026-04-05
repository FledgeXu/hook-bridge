#!/usr/bin/env python3
import json
import os
import stat
import subprocess
import sys
from pathlib import Path


ENABLE_ENV = "CODEX_AUTO_REVIEW"
CHILD_ENV = "CODEX_AUTO_REVIEW_CHILD"
INCLUDE_UNTRACKED_CONTENT_ENV = "CODEX_AUTO_REVIEW_INCLUDE_UNTRACKED_CONTENT"
MAX_REVIEW_INPUT_CHARS = 120000

HOOK_DIR = Path(__file__).resolve().parent
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
REVIEW_TRIGGER_FILES = {"scripts/hooks/review_prompt.md"}
PASS_MARKER = "PASS"


def pattern_sets(patterns: str) -> tuple[set[str], set[str]]:
    names, suffixes = set(), set()
    for item in patterns.split(","):
        pattern = item.strip().lower()
        if pattern:
            (suffixes if pattern.startswith(".") else names).add(pattern)
    return names, suffixes


CODE_PATTERNS = pattern_sets(DEFAULT_CODE_PATTERNS)
UNTRACKED_CONTENT_PATTERNS = pattern_sets(DEFAULT_UNTRACKED_CONTENT_PATTERNS)


def allow() -> int:
    return 0


def fail(message: str) -> int:
    text = message.strip()
    if text:
        print(text)
    return 1


def env_enabled(name: str, default: bool = True) -> bool:
    value = os.environ.get(name)
    return default if value is None else value.strip().lower() not in {"", "0", "false", "no", "off"}


def auto_review_enabled() -> bool:
    return env_enabled(ENABLE_ENV)


def is_child_process() -> bool:
    return env_enabled(CHILD_ENV, default=False)


def include_untracked_content() -> bool:
    return env_enabled(INCLUDE_UNTRACKED_CONTENT_ENV, default=False)


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
        ["codex", "exec", "--disable", "codex_hooks", "-"],
        cwd,
        input_text=f"{PROMPT_PATH.read_text(encoding='utf-8').strip()}\n\n{context}<changes>\n{review_input}\n</changes>\n",
        extra_env={CHILD_ENV: "1"},
    )


def normalize_review_output(text: str) -> str:
    return text.strip()


def finalize_review(review_output: str) -> int:
    if review_output == PASS_MARKER:
        return allow()
    if not review_output:
        return fail("自动审查返回空输出，无法判断是否通过。")
    return fail(review_output[:12000])


def review_result(
    cwd: str,
    review_input: str,
    last_assistant_message: str,
) -> int:
    try:
        review_proc = run_review(cwd, review_input, last_assistant_message)
    except Exception as exc:
        return fail(f"自动审查配置错误：读取 review 模版失败。\n\n{exc}")
    if review_proc.returncode != 0:
        return fail(
            "自动审查进程执行失败。\n\n"
            f"stdout:\n{review_proc.stdout[:4000]}\n\n"
            f"stderr:\n{review_proc.stderr[:4000]}"
        )
    return finalize_review(normalize_review_output(review_proc.stdout or ""))


def parse_payload() -> tuple[str, str]:
    payload = json.load(sys.stdin)
    return payload["cwd"], str(payload.get("last_assistant_message") or "")


def main() -> int:
    cwd, last_assistant_message = parse_payload()
    if not auto_review_enabled() or is_child_process():
        return allow()

    try:
        review_input = "" if not has_reviewable_changes(cwd) else collect_changes(cwd)
    except Exception as exc:
        return fail(f"自动审查在收集改动时失败。\n\n{exc}")

    if not review_input:
        return allow()
    if not PROMPT_PATH.is_file():
        return fail(f"自动审查配置错误：缺少 prompt 文件 {PROMPT_PATH}")
    return review_result(cwd, review_input, last_assistant_message)


if __name__ == "__main__":
    raise SystemExit(main())

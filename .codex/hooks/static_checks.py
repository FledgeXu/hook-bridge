#!/usr/bin/env python3
from contextlib import suppress
import hashlib
import json
import subprocess
import sys
import tempfile
from collections.abc import Sequence
from pathlib import Path
from typing import Self, TypedDict

HOOK_DIR = Path(__file__).resolve().parent
CONFIG_PATH = HOOK_DIR / "static_checks.json"
MAX_OUTPUT_CHARS = 12000


class ConfigError(ValueError):
    def __init__(self, detail: str) -> None:
        super().__init__(f"静态检查配置无效：{detail}")

    @classmethod
    def missing_file(cls, path: Path) -> Self:
        return cls(f"未找到检查配置文件：{path}")

    @classmethod
    def invalid_json(cls, detail: str) -> Self:
        return cls(f"检查配置文件不是合法 JSON：{detail}")

    @classmethod
    def invalid_checks(cls) -> Self:
        return cls("`checks` 必须是非空数组")

    @classmethod
    def invalid_name(cls) -> Self:
        return cls("检查项的 `name` 必须是非空字符串")

    @classmethod
    def invalid_command(cls, name: object) -> Self:
        return cls(f"检查项 `{name}` 的 `command` 必须是非空字符串数组")


class ConfigTypeError(TypeError):
    def __init__(self, detail: str) -> None:
        super().__init__(f"静态检查配置类型错误：{detail}")

    @classmethod
    def invalid_item(cls) -> Self:
        return cls("每个检查项都必须是对象")

    @classmethod
    def invalid_root(cls) -> Self:
        return cls("配置文件顶层必须是对象")


class Check(TypedDict):
    name: str
    command: Sequence[str]


def emit(payload: dict) -> int:
    print(json.dumps(payload, ensure_ascii=False))
    return 0


def allow() -> int:
    return emit({"continue": True})


def block(reason: str) -> int:
    return emit({"decision": "block", "reason": reason})


def block_once(cwd: str, turn_id: str, reason: str) -> int:
    if turn_id:
        mark_resumed_turn(cwd, turn_id)
    return block(reason)


def join_command(command: Sequence[str]) -> str:
    return " ".join(command)


def state_dir(cwd: str) -> Path:
    key = hashlib.sha256(str(Path(cwd).resolve()).encode("utf-8")).hexdigest()[:16]
    return Path(tempfile.gettempdir()) / "codex-static-checks" / key


def turn_marker_path(cwd: str, turn_id: str) -> Path:
    key = hashlib.sha256(turn_id.encode("utf-8")).hexdigest()
    return state_dir(cwd) / key


def prune_state_dir(cwd: str) -> None:
    with suppress(Exception):
        state_dir(cwd).rmdir()


def has_resumed_turn(cwd: str, turn_id: str) -> bool:
    with suppress(OSError):
        return turn_marker_path(cwd, turn_id).exists()
    return False


def mark_resumed_turn(cwd: str, turn_id: str) -> bool:
    try:
        path = turn_marker_path(cwd, turn_id)
        path.parent.mkdir(parents=True, exist_ok=True)
        with suppress(FileExistsError), path.open("x", encoding="utf-8") as handle:
            handle.write("")
    except OSError:
        return False
    return True


def clear_resumed_turn(cwd: str, turn_id: str) -> None:
    with suppress(OSError):
        turn_marker_path(cwd, turn_id).unlink()
    prune_state_dir(cwd)


def load_checks() -> tuple[Check, ...]:
    try:
        data = json.loads(CONFIG_PATH.read_text(encoding="utf-8"))
    except FileNotFoundError as exc:
        raise ConfigError.missing_file(CONFIG_PATH) from exc
    except json.JSONDecodeError as exc:
        raise ConfigError.invalid_json(str(exc)) from exc

    if not isinstance(data, dict):
        raise ConfigTypeError.invalid_root()

    checks = data.get("checks")
    if not isinstance(checks, list) or not checks:
        raise ConfigError.invalid_checks()

    def parse_check(item: object) -> Check:
        if not isinstance(item, dict):
            raise ConfigTypeError.invalid_item()
        name = item.get("name")
        command = item.get("command")
        if not isinstance(name, str) or not name.strip():
            raise ConfigError.invalid_name()
        if (
            not isinstance(command, list)
            or not command
            or any(not isinstance(part, str) or not part.strip() for part in command)
        ):
            raise ConfigError.invalid_command(name)
        return {"name": name.strip(), "command": tuple(command)}

    return tuple(parse_check(item) for item in checks)


def run_check(cwd: str, name: str, command: Sequence[str]) -> str | None:
    try:
        proc = subprocess.run(
            list(command),
            cwd=cwd,
            text=True,
            capture_output=True,
        )
    except FileNotFoundError:
        return f"{name} 检查失败：找不到命令：{join_command(command)}"

    if proc.returncode == 0:
        return None

    output = "\n".join(part for part in (proc.stdout, proc.stderr) if part).strip()
    details = output[:MAX_OUTPUT_CHARS] if output else f"{name} 检查失败，退出码：{proc.returncode}"
    return f"$ {join_command(command)}\n{details}"


def main() -> int:
    payload = json.load(sys.stdin)
    cwd = payload["cwd"]
    turn_id = str(payload.get("turn_id") or "")
    stop_hook_active = bool(payload.get("stop_hook_active"))

    # `stop_hook_active` 是最终兜底；有 turn_id 时优先使用独立标记文件，避免共享状态冲突。
    if stop_hook_active:
        if turn_id:
            clear_resumed_turn(cwd, turn_id)
        return allow()
    if turn_id and has_resumed_turn(cwd, turn_id):
        clear_resumed_turn(cwd, turn_id)
        return allow()

    try:
        checks = load_checks()
    except (ConfigError, ConfigTypeError) as exc:
        return block_once(cwd, turn_id, str(exc))

    failures = tuple(
        filter(
            None,
            (run_check(cwd, check["name"], check["command"]) for check in checks),
        )
    )

    if not failures:
        if turn_id:
            clear_resumed_turn(cwd, turn_id)
        return allow()

    # Stop hook 在 exit 0 时必须输出 JSON；返回 decision=block 会让 Codex 自动继续一轮
    return block_once(
        cwd,
        turn_id,
        "请先执行并修复以下静态检查问题，然后再结束本轮任务。\n\n"
        f"检查输出：\n\n{'\n\n'.join(failures)}"
    )


if __name__ == "__main__":
    raise SystemExit(main())

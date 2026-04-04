#!/usr/bin/env python3
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


def join_command(command: Sequence[str]) -> str:
    return " ".join(command)


def state_dir(cwd: str) -> Path:
    key = hashlib.sha256(str(Path(cwd).resolve()).encode("utf-8")).hexdigest()[:16]
    return Path(tempfile.gettempdir()) / "codex-static-checks" / key


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

    try:
        checks = load_checks()
    except (ConfigError, ConfigTypeError) as exc:
        return block(str(exc))

    failures = tuple(
        filter(
            None,
            (run_check(cwd, check["name"], check["command"]) for check in checks),
        )
    )

    if not failures:
        return allow()

    return block(
        "请先执行并修复以下静态检查问题，然后再结束本轮任务。\n\n"
        f"检查输出：\n\n{'\n\n'.join(failures)}"
    )


if __name__ == "__main__":
    raise SystemExit(main())

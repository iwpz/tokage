#!/usr/bin/env python3
"""Report Tauri commands with no production TypeScript caller.

口径见 docs/fix-todo.md 附录 F。退出码：存在未豁免的无调用方命令时为 1。
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path


def repo_root() -> Path:
    return Path(__file__).resolve().parent.parent


def load_commands(rust_path: Path) -> list[str]:
    text = rust_path.read_text(encoding="utf-8")
    return re.findall(
        r"#\[tauri::command\]\s*(?:pub\s+)?(?:async\s+)?fn\s+([a-z0-9_]+)",
        text,
    )


def load_wrappers(ipc_path: Path) -> dict[str, str]:
    lines = ipc_path.read_text(encoding="utf-8").splitlines()
    decl = re.compile(
        r"^export\s+(?:async\s+)?(?:function\s+([A-Za-z0-9_]+)|const\s+([A-Za-z0-9_]+)\s*=)"
    )
    cur: str | None = None
    wrapper: dict[str, str] = {}
    for ln in lines:
        m = decl.match(ln)
        if m:
            cur = m.group(1) or m.group(2)
        for c in re.findall(r"invoke[^(]*\(\s*['\"]([a-z0-9_]+)['\"]", ln):
            if cur is not None:
                wrapper.setdefault(c, cur)
    return wrapper


def production_sources(src_root: Path) -> list[str]:
    out: list[str] = []
    for p in src_root.rglob("*"):
        if p.suffix not in (".ts", ".tsx"):
            continue
        if p.name == "ipc.ts" or ".test." in p.name:
            continue
        out.append(p.read_text(encoding="utf-8"))
    return out


def is_used(name: str, sources: list[str]) -> bool:
    pat = re.compile(r"\b" + re.escape(name) + r"\b")
    return any(pat.search(t) for t in sources)


def load_allowlist(path: Path) -> dict[str, str]:
    if not path.is_file():
        return {}
    allowed: dict[str, str] = {}
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        parts = line.split(None, 1)
        cmd = parts[0]
        reason = parts[1] if len(parts) > 1 else ""
        if not reason:
            raise SystemExit(f"allowlist entry missing reason: {raw!r}")
        allowed[cmd] = reason
    return allowed


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--root",
        type=Path,
        default=repo_root(),
        help="repository root (default: parent of scripts/)",
    )
    args = parser.parse_args()
    root: Path = args.root.resolve()

    cmds = load_commands(root / "src-tauri" / "src" / "lib.rs")
    wrappers = load_wrappers(root / "src" / "lib" / "ipc.ts")
    prod = production_sources(root / "src")
    allowlist = load_allowlist(root / "scripts" / "ipc-wiring-allowlist.txt")

    missing = [
        c for c in cmds if c not in wrappers or not is_used(wrappers[c], prod)
    ]
    unexempted = [c for c in missing if c not in allowlist]

    print(f"commands={len(cmds)} no_production_caller={len(unexempted)}")
    if unexempted:
        print(*unexempted, sep="\n")
        return 1

    stale = sorted(set(allowlist) - set(missing))
    if stale:
        print(
            "warning: allowlist entries that now have production callers:",
            ", ".join(stale),
            file=sys.stderr,
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())

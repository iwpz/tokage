#!/usr/bin/env python3
"""Verify archived m2c delivery artifacts using Git, without invoking any model."""
import json
import os
from pathlib import Path
import subprocess
import tempfile

EVIDENCE = Path(__file__).resolve().parents[1] / "docs/reliability-evidence/m2c/git"
ENV = {k: v for k, v in os.environ.items() if k not in {"GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_COMMON_DIR"}}
ENV["GIT_TERMINAL_PROMPT"] = "0"


def git(cwd, *args):
    return subprocess.run(["git", "-C", str(cwd), *args], env=ENV, check=True,
                          capture_output=True, timeout=30).stdout.strip()


def main():
    patch = json.loads((EVIDENCE / "patch-and-kept-branch.json").read_text())
    merge = json.loads((EVIDENCE / "merged-delivery.json").read_text())
    with tempfile.TemporaryDirectory(prefix="tokage-m2c-review-") as temporary:
        root = Path(temporary)
        for name in ("patch-and-kept-branch", "merged-delivery"):
            git(root, "clone", "--bare", "--quiet", str(EVIDENCE / f"{name}.bundle"), str(root / name))
        repo = root / "patch-and-kept-branch"
        git(repo, "config", "core.autocrlf", "false")
        assert git(repo, "rev-parse", f"refs/heads/{patch['retainedBranch']}").decode() == patch["source"]
        checkout = root / "checkout"
        git(repo, "worktree", "add", "--detach", str(checkout), patch["base"])
        git(checkout, "apply", "--index", str(EVIDENCE / "delivery.patch"))
        tree = git(checkout, "write-tree").decode()
        assert tree == patch["sourceTree"] == patch["roundTripIndexTree"]
        assert patch["worktreeRemoved"] is True
        assert (checkout / "binary").read_bytes() == bytes([0, 255, 1, 0, 128])
        assert (checkout / "non-utf8").read_bytes() == bytes([255, 254, 10, 128])
        delivered = root / "merged-delivery"
        assert git(delivered, "rev-parse", "main").decode() == merge["deliveredHead"]
        assert git(delivered, "rev-parse", "main^2").decode() == merge["acceptedSource"] == merge["deliveredSecondParent"]
        assert git(delivered, "show", "main:packages/app/a") == b"A"
        assert git(delivered, "show", "main:packages/app/b") == b"B"
    print("PASS m2c: exported patch restores exact tree; retained branch resolves; merge contains accepted source and both upstream outputs")


if __name__ == "__main__":
    main()

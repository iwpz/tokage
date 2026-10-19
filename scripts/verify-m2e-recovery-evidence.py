#!/usr/bin/env python3
"""Independently reconstruct a recovered delivery from local, synthetic Git evidence."""
import json
import os
from pathlib import Path
import subprocess
import tempfile


def main():
    evidence = Path(__file__).resolve().parents[1] / "docs/reliability-evidence/m2e/git"
    receipt = json.loads((evidence / "git-receipt.json").read_text())
    with tempfile.TemporaryDirectory(prefix="tokage-m2e-verify-") as temporary:
        root = Path(temporary)
        env = {key: value for key, value in os.environ.items()
               if key not in {"GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_COMMON_DIR"}}
        env.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull,
                   GIT_INDEX_FILE=str(root / "recovered.index"))

        def git(*args):
            return subprocess.run(["git", "-C", str(root), *args], env=env,
                                  check=True, capture_output=True).stdout

        git("init", "--bare", "-q")
        git("fetch", "--no-tags", str(evidence / "recovered.bundle"), "refs/heads/*:refs/heads/*")
        git("read-tree", receipt["baseCommit"])
        git("apply", "--cached", "--binary", str(evidence / "delivery.patch"))
        assert git("write-tree").decode().strip() == receipt["expectedTree"]
        assert git("rev-parse", receipt["deliveredCommit"] + "^{tree}").decode().strip() == receipt["expectedTree"]
        assert git("rev-parse", receipt["deliveredCommit"] + "^1").decode().strip() == receipt["baseCommit"]
        assert git("rev-parse", receipt["deliveredCommit"] + "^2").decode().strip() == receipt["sourceCommit"]
        assert git("rev-parse", "refs/heads/" + receipt["retainedBranch"]).decode().strip() == receipt["sourceCommit"]
        assert git("show", receipt["deliveredCommit"] + ":result.bin") == bytes([0, 255, 3, 4])
        assert git("show", receipt["deliveredCommit"] + ":base") == b"keep\n"
    print("PASS m2e: recovered tree equals accepted tree and patch reconstruction; merge parents and retained source match")


if __name__ == "__main__":
    main()

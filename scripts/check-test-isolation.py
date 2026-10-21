#!/usr/bin/env python3
"""Reject known acceptance hooks in shipped source; no app/model invocation.

This is a regression fence, not a complete semantic audit. Review new runtime
branches and the production dependency graph as required by testing-isolation.md.
"""
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FORBIDDEN = re.compile(
    r'liveDesktopAcceptance|liveSessionScrollProbe|liveNaturalReview|live_acceptance|'
    r'VITE_TOKAGE_LIVE_ACCEPTANCE|TOKAGE_LIVE_DESKTOP_CONFIG|'
    r'TOKAGE_(?:IDLE_TIMEOUT_MS|HARD_TIMEOUT_MS|DISABLE_SANDBOX|BWRAP_PATH)|'
    r'CLAUDE_CODE_DISABLE_AUTO_MEMORY|npm_config_cache|data-stream-size|'
    r'''["']m3a(?:-\d+)?["']|(?:from\s*|import\s*(?:\(\s*)?)["'][^"']*(?:tests/|scripts/|tests\\|scripts\\)|'''
    r'live-acceptance'
)


def violations(root):
    errors = []
    for base in ('src', 'src-tauri/src', 'src-tauri/crates', 'cli/src'):
        for path in sorted((root / base).rglob('*')):
            if path.suffix not in ('.ts', '.tsx', '.rs', '.toml') or '.test.' in path.name or '.spec.' in path.name:
                continue
            # These are external Cargo test targets, never application dependencies.
            if 'tests' in path.relative_to(root / base).parts:
                continue
            for index, line in enumerate(path.read_text().splitlines(), 1):
                if FORBIDDEN.search(line):
                    errors.append(f'{path.relative_to(root)}:{index}: forbidden acceptance hook')
    manifest = root / 'src-tauri/Cargo.toml'
    if 'live-acceptance' in manifest.read_text():
        errors.append('src-tauri/Cargo.toml: removed acceptance feature restored')
    return errors


def main():
    errors = violations(ROOT)
    if errors:
        raise SystemExit('\n'.join(errors))
    print('PASS test isolation: no known internal acceptance hooks in application source')


if __name__ == '__main__':
    main()

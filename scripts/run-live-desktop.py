#!/usr/bin/env python3
"""Retired desktop driver. Model baseline constants remain for offline audits only."""
CHOICES = {
    "CODEX_CLI": ("gpt-6-luna", None),
    "OPENCODE": ("deepseek/deepseek-flash", None),
    "REASONIX": ("deepseek/deepseek-flash", None),
    "KIRO_CLI": ("claude-haiku-4.5", None),
    "AIDER": ("deepseek/deepseek-flash", None),
    "CLAUDE_CODE": ("haiku", None),
    "CURSOR_CLI": ("auto", None),
    "COPILOT_CLI": ("auto", "efficiency"),
    "ANTIGRAVITY_CLI": ("gemini-3.8-flash-low", None),
}

def main():
    raise SystemExit("Retired internal driver; use docs/testing-isolation.md and scripts/prepare-desktop-test.py")

if __name__ == "__main__":
    main()

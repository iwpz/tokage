#!/usr/bin/env python3
"""Offline integrity check for the selected CLI baselines; never invokes a CLI."""
import ast
import hashlib
import json
from pathlib import Path


def verify(root: Path) -> None:
    baseline = json.loads((root / 'scripts/cli-baselines.json').read_text())
    choices = next(ast.literal_eval(node.value) for node in ast.parse((root / 'scripts/run-live-desktop.py').read_text()).body if isinstance(node, ast.Assign) and any(isinstance(t, ast.Name) and t.id == 'CHOICES' for t in node.targets))
    targets = json.loads((root / 'src/lib/reliabilitySupport.json').read_text())['targets']
    assert set(baseline) == {t['provider'] for t in targets}, 'selected providers differ'
    for target in targets:
        entry = baseline[target['provider']]
        assert (entry['version'], entry['model']) == (target['version'], target['model']), 'baseline changed: upgrade evidence and manifest required'
        assert choices[target['provider']][0] == entry['model'], 'desktop model differs from baseline'
        assert entry['fixtures'], 'protocol fixtures required'
        for fixture in entry['fixtures']:
            data = (root / fixture['path']).read_bytes()
            assert hashlib.sha256(data).hexdigest() == fixture['sha256'], f"fixture changed: review protocol evidence: {fixture['path']}"
        print(f"PASS {target['provider']} {entry['version']} {entry['model']} protocol integrity (offline, not live certification)")


if __name__ == '__main__':
    verify(Path(__file__).resolve().parent.parent)

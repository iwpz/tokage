#!/usr/bin/env python3
"""Historical crash evidence readers; the injected desktop runner is retired."""
import hashlib
import json
import time

def receipts(root):
    return {str(p.relative_to(root)): hashlib.sha256(p.read_bytes()).hexdigest()
            for p in root.glob('app-data/**/turn-receipts/*.json')}


def wait_event(root, process, step, timeout):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        path = root / 'progress.jsonl'
        for line in path.read_text().splitlines() if path.exists() else []:
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            if event.get('step') == step:
                return event
            if event.get('status') == 'failed' or (step == 'crash-ready' and event.get('step') == 'native-error'):
                raise RuntimeError(f'Real CLI or desktop failure before {step}; evidence: {root}')
        if process.poll() is not None:
            raise RuntimeError(f'Desktop exited {process.returncode} before {step}; evidence: {root}')
        time.sleep(.1)
    raise TimeoutError(f'Timed out waiting for {step}; evidence: {root}')



def main():
    raise SystemExit("Retired internal driver; use docs/testing-isolation.md and scripts/prepare-desktop-test.py")

if __name__ == "__main__":
    main()

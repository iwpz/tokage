import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { TASK_STATE_TRANSITIONS, canTransition } from "./taskState";
import type { TaskState } from "./ipc";

const GOLD_PATH = resolve(
  __dirname,
  "../../src-tauri/crates/tokage-orchestration/tests/fixtures/task_state_transitions.json",
);

describe("taskState", () => {
  it("matches the Rust gold file entry-by-entry", () => {
    const gold = JSON.parse(readFileSync(GOLD_PATH, "utf8")) as Record<string, string[]>;
    const states = Object.keys(TASK_STATE_TRANSITIONS) as TaskState[];
    expect(states.sort()).toEqual(Object.keys(gold).sort());

    for (const from of states) {
      expect([...TASK_STATE_TRANSITIONS[from]].sort()).toEqual([...(gold[from] ?? [])].sort());
      for (const to of states) {
        const allowed = from === to || (gold[from] ?? []).includes(to);
        expect(canTransition(from, to)).toBe(allowed);
      }
    }
  });
});

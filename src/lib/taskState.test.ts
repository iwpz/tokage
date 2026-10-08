import { describe, expect, it } from "vitest";
import { TASK_STATE_TRANSITIONS, canTransition } from "./taskState";
import type { TaskState } from "./ipc";
import goldJson from "../../src-tauri/crates/tokage-orchestration/tests/fixtures/task_state_transitions.json";

const gold = goldJson as Record<string, string[]>;

describe("taskState", () => {
  it("matches the Rust gold file entry-by-entry", () => {
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

import { describe, expect, it } from "vitest";
import { resolvePlanningDomains, resolveValidDomains } from "./planningDomains";
import { useKnowledge } from "../stores/knowledge";

describe("planningDomains", () => {
  it("excludes role-only planning from task domains", () => {
    useKnowledge.getState().resetToBuiltin();
    const domains = resolvePlanningDomains();
    expect(domains).toContain("frontend-ui");
    expect(domains).toContain("data-model");
    expect(domains).toContain("infra");
    expect(domains).not.toContain("database");
    expect(domains).not.toContain("devops");
    expect(domains).not.toContain("security");
    expect(domains).not.toContain("planning");
  });

  it("includes full pack domains for validation", () => {
    useKnowledge.getState().resetToBuiltin();
    const domains = resolveValidDomains();
    expect(domains).toContain("planning");
    expect(domains).toContain("data-model");
    expect(domains).not.toContain("database");
  });
});

import { describe, expect, it } from "vitest";
import { formatModelName, modelDisplay, providerDisplayName } from "./modelDisplay";

describe("prototype-named display identifiers", () => {
  it.each(["constructor", "toString", "__proto__"])("formats %s without inherited dictionary entries", (id) => {
    const name = formatModelName(id);
    expect(typeof name).toBe("string");
    expect(name).not.toContain("function");
    const provider = providerDisplayName(id, "Historical provider");
    expect(provider).toBe("Historical provider");
    expect(typeof providerDisplayName(id)).toBe("string");
    expect(modelDisplay({ provider: id, requestedModel: id }).label).not.toContain("[object Object]");
    expect(modelDisplay({ provider: id, requestedModel: id }).label).not.toContain("function");
  });
});

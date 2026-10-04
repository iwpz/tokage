import { beforeEach, describe, expect, it } from "vitest";
import { authLayers, resolveMode, useAuth } from "./auth";
import { useSettings } from "./settings";

describe("authorization scopes", () => {
  beforeEach(() => {
    useSettings.setState({ authMode: "ask_sensitive", authProjectModes: {} });
    useAuth.setState({ sessionModes: {} });
  });

  it("layers narrower scopes over wider ones", () => {
    expect(resolveMode(authLayers("p1", "s1"), false)).toEqual({ mode: "ask_sensitive", scope: "global" });
    useSettings.setState({ authProjectModes: { p1: "ask_always" } });
    expect(resolveMode(authLayers("p1", "s1"), false)).toEqual({ mode: "ask_always", scope: "project" });
    expect(resolveMode(authLayers("p2", null), false).scope).toBe("global");
    useAuth.getState().setSessionMode("s1", "ask_sensitive");
    expect(resolveMode(authLayers("p1", "s1"), false)).toEqual({ mode: "ask_sensitive", scope: "session" });
    useAuth.getState().setSessionMode("s1", null);
    expect(authLayers("p1", "s1").session).toBeNull();
  });

  it("skips full_auto where it is not allowed", () => {
    useAuth.getState().setSessionMode("s1", "full_auto");
    expect(resolveMode(authLayers(null, "s1"), true)).toEqual({ mode: "full_auto", scope: "session" });
    expect(resolveMode(authLayers(null, "s1"), false)).toEqual({ mode: "ask_sensitive", scope: "global" });
  });
});

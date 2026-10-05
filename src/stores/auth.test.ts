import { beforeEach, describe, expect, it } from "vitest";
import { authLayers, calculateFullAutoExpiry, resolveMode, useAuth } from "./auth";
import { useSettings } from "./settings";

describe("authorization scopes", () => {
  beforeEach(() => {
    useSettings.setState({ authMode: "ask_sensitive", authProjectModes: {} });
    useAuth.setState({ sessionModes: {}, fullAutoGrants: {} });
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

  it("handles full_auto grants with TTL and expiration (P2.m6a)", () => {
    const fixedNow = 1700000000000;
    expect(calculateFullAutoExpiry("1h", fixedNow)).toBe(fixedNow + 3600000);
    expect(calculateFullAutoExpiry("session", fixedNow)).toBeNull();

    useAuth.getState().grantFullAuto("s1", "1h");
    expect(useAuth.getState().isFullAutoActive("s1")).toBe(true);
    expect(resolveMode(authLayers(null, "s1"), true, "s1")).toEqual({ mode: "full_auto", scope: "session" });

    // Mock expired grant
    useAuth.setState({
      fullAutoGrants: {
        s1: { ttl: "1h", grantedAt: fixedNow - 7200000, expiresAt: fixedNow - 3600000 },
      },
    });
    expect(useAuth.getState().isFullAutoActive("s1")).toBe(false);
    // Should have automatically fallen back
    expect(resolveMode(authLayers(null, "s1"), true, "s1")).toEqual({ mode: "ask_sensitive", scope: "global" });

    // Revoke full auto
    useAuth.getState().grantFullAuto("s2", "session");
    expect(useAuth.getState().isFullAutoActive("s2")).toBe(true);
    useAuth.getState().revokeFullAuto("s2");
    expect(useAuth.getState().isFullAutoActive("s2")).toBe(false);
    expect(useAuth.getState().sessionModes["s2"]).toBeUndefined();
  });
});

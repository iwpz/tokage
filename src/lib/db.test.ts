import { describe, expect, it } from "vitest";
import { diff, hasLegacy, readLegacy, writeLegacy, type Persisted } from "./db";

class MemStore {
  map = new Map<string, string>();
  getItem(k: string) {
    return this.map.get(k) ?? null;
  }
  setItem(k: string, v: string) {
    this.map.set(k, v);
  }
  removeItem(k: string) {
    this.map.delete(k);
  }
}

const legacy = () => {
  const s = new MemStore();
  const put = (key: string, state: unknown, version: number) => s.setItem(key, JSON.stringify({ state, version }));
  put(
    "tokage.projects",
    {
      projects: [
        {
          id: "p1",
          name: "app",
          path: "/app",
          expanded: true,
          trusted: true,
          sessions: [
            { id: "s1", title: "first", state: "draft", updatedAt: 1000 },
            { id: "s2", title: "fork", state: "draft", updatedAt: 900, forkedFrom: "s1", archived: true },
          ],
        },
      ],
      activeProjectId: "p1",
      activeSessionId: "s1",
    },
    1,
  );
  put(
    "tokage.chat",
    {
      conversations: {
        s1: {
          thread: { ref: { provider: "CLAUDE_CODE", model: "opus" }, id: "cli-1" },
          messages: [
            { id: "u1", role: "user", text: "hi" },
            // v1 shape: tool steps beside the text.
            {
              id: "a1",
              role: "assistant",
              text: "done",
              mode: "direct",
              ref: { provider: "CLAUDE_CODE", model: "opus" },
              tools: [{ label: "edit a.ts", ok: true }],
            },
          ],
        },
        orphan: { thread: null, messages: [{ id: "x", role: "user", text: "lost" }] },
      },
      picks: { plan: null, direct: null },
    },
    1,
  );
  put("tokage.settings", { theme: "dark", language: "zh", welcomeSeen: true }, 1);
  put("tokage.providers", { providers: [{ id: "OLD_SHAPE" }], lastScan: 5 }, 1);
  put("tokage.ui", { sidebarWidth: 300, sidebarCollapsed: true, previewWidth: 600 }, 1);
  put("tokage.knowledge", { url: "https://x/p.json", custom: null }, 1);
  return s;
};

describe("readLegacy", () => {
  it("reads every old key and upgrades v1 messages", () => {
    const s = readLegacy(legacy());
    expect(s.projects[0].sessions).toHaveLength(2);
    expect(s.activeSessionId).toBe("s1");
    expect(s.values["settings.theme"]).toBe("dark");
    expect(s.values["settings.language"]).toBe("zh");
    expect(s.values["ui.sidebarWidth"]).toBe(300);
    expect(s.values["knowledge.url"]).toBe("https://x/p.json");
    const a1 = s.conversations.s1.messages[1];
    expect(a1.parts?.[0]).toMatchObject({ type: "tool", tool: "edit", title: "a.ts" });
    expect("tools" in a1).toBe(false);
    // Provider cache from an older ProviderInfo shape is dropped.
    expect(s.providers).toEqual([]);
    expect(s.values["providers.lastScan"]).toBeNull();
  });

  it("survives missing and corrupt keys", () => {
    const store = new MemStore();
    expect(hasLegacy(store)).toBe(false);
    store.setItem("tokage.chat", "{not json");
    expect(hasLegacy(store)).toBe(true);
    const s = readLegacy(store);
    expect(s.projects).toEqual([]);
    expect(s.conversations).toEqual({});
  });

  it("round-trips through writeLegacy", () => {
    const first = readLegacy(legacy());
    const store = new MemStore();
    writeLegacy(store, first);
    const again = readLegacy(store);
    expect(again.projects).toEqual(first.projects);
    expect(again.conversations).toEqual(first.conversations);
    expect(again.values["settings.theme"]).toBe("dark");
  });
});

describe("diff", () => {
  const base = (): Persisted => readLegacy(legacy());

  it("writes everything on import, skipping conversations without a session", () => {
    const b = diff(null, base());
    expect(b.projects.map((p) => p.id)).toEqual(["p1"]);
    expect(b.sessions.map((s) => [s.id, s.position])).toEqual([
      ["s1", 0],
      ["s2", 1],
    ]);
    expect(b.messages.map((m) => m.message)).toHaveLength(2);
    expect(b.messages.every((m) => m.sessionId === "s1")).toBe(true);
    // The prompt is attributed to the reply's model and mode.
    expect(b.messages[0]).toMatchObject({ provider: "CLAUDE_CODE", model: "opus", mode: "direct", turnIndex: 0 });
    expect(b.sessionChat).toHaveLength(1);
    expect(b.settings.some((s) => s.key === "settings.theme")).toBe(true);
    expect(b.deleteSessions).toEqual([]);
  });

  it("writes only what changed", () => {
    const prev = base();
    const conv = prev.conversations.s1;
    const reply = { ...conv.messages[1], text: "done!" };
    const next: Persisted = {
      ...prev,
      conversations: { ...prev.conversations, s1: { ...conv, messages: [conv.messages[0], reply] } },
      values: { ...prev.values, "settings.theme": "light" },
    };
    const b = diff(prev, next);
    expect(b.projects).toEqual([]);
    expect(b.sessions).toEqual([]);
    expect(b.sessionChat).toEqual([]);
    // The changed reply, plus nothing else: the prompt before it is unchanged.
    expect(b.messages.map((m) => (m.message as { id: string }).id)).toEqual(["a1"]);
    expect(b.settings).toEqual([{ key: "settings.theme", value: "light" }]);
    expect(b.providers).toBeNull();
  });

  it("re-attributes a prompt when its reply arrives", () => {
    const prev = base();
    const conv = prev.conversations.s1;
    const prompt = { id: "u2", role: "user" as const, text: "more" };
    const withPrompt: Persisted = {
      ...prev,
      conversations: { ...prev.conversations, s1: { ...conv, messages: [...conv.messages, prompt] } },
    };
    const reply = { id: "a2", role: "assistant" as const, text: "", mode: "plan" as const, ref: { provider: "CODEX_CLI" as const, model: "gpt" } };
    const withReply: Persisted = {
      ...withPrompt,
      conversations: { ...prev.conversations, s1: { ...conv, messages: [...conv.messages, prompt, reply] } },
    };
    const b = diff(withPrompt, withReply);
    expect(b.messages.map((m) => [(m.message as { id: string }).id, m.provider, m.turnIndex])).toEqual([
      ["u2", "CODEX_CLI", 1],
      ["a2", "CODEX_CLI", 1],
    ]);
  });

  it("deletes removed sessions and projects", () => {
    const prev = base();
    const p = prev.projects[0];
    const oneSession: Persisted = { ...prev, projects: [{ ...p, sessions: [p.sessions[0]] }] };
    expect(diff(prev, oneSession).deleteSessions).toEqual(["s2"]);
    const none: Persisted = { ...prev, projects: [] };
    const b = diff(prev, none);
    expect(b.deleteProjects).toEqual(["p1"]);
    expect(b.deleteSessions).toEqual(["s1", "s2"]);
  });

  it("writes a conversation in full when its session row appears later", () => {
    const prev = base();
    const p = prev.projects[0];
    const before: Persisted = { ...prev, projects: [{ ...p, sessions: [p.sessions[1]] }] };
    expect(diff(null, before).messages).toEqual([]);
    const b = diff(before, prev);
    expect(b.sessions.map((s) => s.id)).toContain("s1");
    expect(b.messages).toHaveLength(2);
    expect(b.sessionChat).toHaveLength(1);
  });

  it("clears the custom pack", () => {
    const prev: Persisted = { ...base(), customPack: { url: "u", loadedAt: 1, pack: {} as never, warnings: [] } };
    const b = diff(prev, { ...prev, customPack: null });
    expect(b.clearCustomPack).toBe(true);
  });
});

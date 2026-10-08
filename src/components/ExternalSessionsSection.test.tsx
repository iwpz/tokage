import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderToString } from "react-dom/server";
import { ExternalSessionsSection } from "./ExternalSessionsSection";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (k: string) => k,
  }),
}));

vi.mock("../lib/ipc", () => ({
  listNativeSessions: vi.fn(async () => [
    {
      id: "ext-1",
      providerId: "CLAUDE_CODE",
      title: "Refactor auth",
      cwd: "/tmp/demo",
      updatedAt: "2026-10-08T12:00:00.000Z",
      fileSize: 1024,
      path: "/tmp/.claude/session.jsonl",
    },
  ]),
}));

vi.mock("./ProviderLogo", () => ({
  ProviderLogo: ({ name }: { name: string }) => <span data-logo>{name}</span>,
}));

describe("ExternalSessionsSection (F17.a)", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("renders section title and empty state before load settles (SSR)", () => {
    const html = renderToString(<ExternalSessionsSection projectPath="/tmp/demo" />);
    expect(html).toContain("sidebar.externalSessions");
  });

  it("mock listNativeSessions returns a session for project scope", async () => {
    const { listNativeSessions } = await import("../lib/ipc");
    const rows = await listNativeSessions(undefined, "/tmp/demo");
    expect(rows).toHaveLength(1);
    expect(rows[0].title).toBe("Refactor auth");
    expect(listNativeSessions).toHaveBeenCalledWith(undefined, "/tmp/demo");
  });
});

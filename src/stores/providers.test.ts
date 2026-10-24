import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ProviderInfo } from "../lib/ipc";
import { getOnlineUsagePermission, queryProviderQuota, getProviderUtilizationNow, verifyAntigravityApiKey, verifyReasonixApiKey } from "../lib/ipc";
import { useProviders } from "./providers";
import { useOnlineUsage } from "./onlineUsage";

vi.mock("../lib/ipc", async (importOriginal) => ({
  ...await importOriginal<typeof import("../lib/ipc")>(),
  getOnlineUsagePermission: vi.fn().mockResolvedValue({ allowOnlineUsageQuery: true, generation: 0 }),
  onOnlineUsagePermissionChanged: vi.fn().mockResolvedValue(() => {}),
  queryProviderQuota: vi.fn(),
  getProviderUtilizationNow: vi.fn(),
  verifyAntigravityApiKey: vi.fn(),
  verifyReasonixApiKey: vi.fn(),
}));
const unverified = { id: "ANTIGRAVITY_CLI", status: "UNKNOWN_AUTH", authMethod: "Gemini API key" } as ProviderInfo;

beforeEach(() => {
  vi.mocked(getOnlineUsagePermission).mockResolvedValue({ allowOnlineUsageQuery: true, generation: 0 });
  useOnlineUsage.setState({ ready: true, busy: false, allowOnlineUsageQuery: true, generation: 0, error: null });
  vi.mocked(verifyAntigravityApiKey).mockReset();
  useProviders.setState({ providers: [unverified], realProviders: [unverified], scanning: false, pending: [], error: null });
});

describe("explicit Antigravity key verification", () => {
  it("updates the provider only after successful verification", async () => {
    const verified = { ...unverified, status: "READY" as const };
    let resolve!: (value: ProviderInfo) => void;
    vi.mocked(verifyAntigravityApiKey).mockReturnValue(new Promise((r) => { resolve = r; }));
    const run = useProviders.getState().verifyAntigravityKey();
    expect(useProviders.getState().pending).toContain("ANTIGRAVITY_CLI");
    expect(useProviders.getState().providers[0].status).toBe("UNKNOWN_AUTH");
    await useProviders.getState().verifyAntigravityKey();
    expect(verifyAntigravityApiKey).toHaveBeenCalledTimes(1);
    resolve(verified);
    await run;
    expect(useProviders.getState().providers[0].status).toBe("READY");
    expect(useProviders.getState().realProviders[0].status).toBe("READY");
    expect(useProviders.getState().pending).toEqual([]);
  });
  it("keeps an unverified provider unverified on failure and exposes the error", async () => {
    vi.mocked(verifyAntigravityApiKey).mockRejectedValue(new Error("verification failed"));
    await useProviders.getState().verifyAntigravityKey();
    expect(useProviders.getState().providers[0].status).toBe("UNKNOWN_AUTH");
    expect(useProviders.getState().error).toContain("verification failed");
    expect(useProviders.getState().pending).toEqual([]);
  });
});

describe("explicit Reasonix key verification", () => {
  it("requires a successful check before READY", async () => {
    const configured = { id: "REASONIX", status: "UNKNOWN_AUTH" } as ProviderInfo;
    useProviders.setState({ providers: [configured], realProviders: [configured] });
    vi.mocked(verifyReasonixApiKey).mockResolvedValue({ ...configured, status: "READY" });
    await useProviders.getState().verifyReasonixKey();
    expect(useProviders.getState().providers[0].status).toBe("READY");
    expect(useProviders.getState().realProviders[0].status).toBe("READY");
    expect(useProviders.getState().pending).toEqual([]);
    vi.mocked(verifyReasonixApiKey).mockRejectedValue(new Error("invalid key"));
    useProviders.setState({ providers: [configured], realProviders: [configured] });
    await useProviders.getState().verifyReasonixKey();
    expect(useProviders.getState().providers[0].status).toBe("UNKNOWN_AUTH");
    expect(useProviders.getState().error).toContain("invalid key");
  });
});


describe("CLI quota refresh", () => {
  it("publishes the returned snapshot to both provider catalogs", async () => {
    const provider = { ...unverified, id: "COPILOT_CLI", account: { official: true, billing: "SUBSCRIPTION", endpoint: null, plan: null, balance: null, usage: null } } as ProviderInfo;
    useProviders.setState({ providers: [provider], realProviders: [provider] });
    const usage = { windows: [{ windowMinutes: 0, usedPercent: 29, resetsAt: null }], observedAt: "2026-10-21T00:00:00Z", source: "CLI_OUTPUT" as const };
    vi.mocked(queryProviderQuota).mockResolvedValue(usage);
    await useProviders.getState().refreshQuota("COPILOT_CLI");
    expect(queryProviderQuota).toHaveBeenCalledWith("COPILOT_CLI");
    expect(useProviders.getState().realProviders[0].account?.usage).toEqual(usage);
    expect(useProviders.getState().providers[0].account?.usage).toEqual(usage);
    expect(getProviderUtilizationNow).toHaveBeenCalledWith("COPILOT_CLI", 0);
  });
  it("keeps the existing snapshot when no quota is available", async () => {
    const usage = { windows: [], observedAt: null, source: "CLI_OUTPUT" as const };
    const provider = { ...unverified, id: "COPILOT_CLI", account: { official: true, billing: "SUBSCRIPTION", endpoint: null, plan: null, balance: null, usage } } as ProviderInfo;
    useProviders.setState({ providers: [provider], realProviders: [provider] });
    vi.mocked(queryProviderQuota).mockResolvedValue(null);
    await useProviders.getState().refreshQuota("COPILOT_CLI");
    expect(useProviders.getState().realProviders[0].account?.usage).toEqual(usage);
  });
});

it("does not invoke quota IPC when native consent is disabled", async () => {
  vi.mocked(getOnlineUsagePermission).mockResolvedValue({ allowOnlineUsageQuery: false, generation: 1 });
  await useProviders.getState().refreshQuota("CURSOR_CLI");
  expect(queryProviderQuota).not.toHaveBeenCalled();
  expect(useProviders.getState().error).toContain("disabled");
});
it("discards a late IPC reply after permission revocation", async () => {
  const provider = { ...unverified, account: { official: true, usage: null } } as ProviderInfo;
  useProviders.setState({ realProviders: [provider], providers: [provider] });
  let finish!: (value: { windows: []; observedAt: null; source: "CLI_OUTPUT" }) => void;
  vi.mocked(queryProviderQuota).mockReturnValue(new Promise((resolve) => { finish = resolve; }));
  const pending = useProviders.getState().refreshQuota("ANTIGRAVITY_CLI");
  await vi.waitFor(() => expect(queryProviderQuota).toHaveBeenCalled());
  useOnlineUsage.setState({ allowOnlineUsageQuery: false, generation: 1 });
  finish({ windows: [], observedAt: null, source: "CLI_OUTPUT" }); await pending;
  expect(useProviders.getState().realProviders[0].account?.usage).toBeNull();
});

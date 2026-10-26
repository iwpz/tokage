import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountUsageResult, UsageSnapshot, ProviderInfo } from "../lib/ipc";
import { getOnlineUsagePermission, queryProviderQuota, getProviderUtilizationNow, verifyAntigravityApiKey, verifyReasonixApiKey, refreshProvider } from "../lib/ipc";
import { useProviders } from "./providers";
import { useOnlineUsage } from "./onlineUsage";
import { useSettings } from "./settings";

vi.mock("../lib/ipc", async (importOriginal) => ({
  ...await importOriginal<typeof import("../lib/ipc")>(),
  getOnlineUsagePermission: vi.fn().mockResolvedValue({ allowOnlineUsageQuery: true, generation: 0 }),
  onOnlineUsagePermissionChanged: vi.fn().mockResolvedValue(() => {}),
  queryProviderQuota: vi.fn(),
  getProviderUtilizationNow: vi.fn(),
  verifyAntigravityApiKey: vi.fn(),
  verifyReasonixApiKey: vi.fn(),
  refreshProvider: vi.fn(),
}));
function result(display: UsageSnapshot | null): AccountUsageResult {
  return { status: "LATEST", identityKey: "opaque", identityGeneration: 1, latestSuccess: display ? {
    schema: 2, scope: { provider: "COPILOT_CLI", backend: "github", endpoint: null, accountLabel: null, organization: null, workspace: null }, identityKey: "opaque", identityGeneration: 1, fetchedAt: 1, validUntil: 100, source: "CLI", windows: [], quantities: [], display,
  } : null, fromCache: false, attemptedAt: 1, nextRetryAt: 16, error: null };
}
const unverified = { id: "ANTIGRAVITY_CLI", status: "UNKNOWN_AUTH", authMethod: "Gemini API key" } as ProviderInfo;

beforeEach(() => {
  vi.mocked(getOnlineUsagePermission).mockResolvedValue({ allowOnlineUsageQuery: true, generation: 0 });
  useOnlineUsage.setState({ ready: true, busy: false, allowOnlineUsageQuery: true, generation: 0, error: null });
  vi.mocked(verifyAntigravityApiKey).mockReset();
  vi.mocked(verifyReasonixApiKey).mockReset();
  vi.mocked(refreshProvider).mockReset();
  useProviders.setState({ providers: [unverified], realProviders: [unverified], scanning: false, pending: [], modelVerificationPending: [], usageResults: {}, usageVersions: {}, usagePending: [], usageActive: [], providerErrors: {}, usageErrors: {}, error: null });
});

afterEach(() => vi.useRealTimers());

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
    expect(useProviders.getState().providerErrors.ANTIGRAVITY_CLI).toContain("verification failed");
    expect(useProviders.getState().pending).toEqual([]);
  });
});

describe("explicit Reasonix key verification", () => {
  it("requires a successful check before READY", async () => {
    const configured = { id: "REASONIX", status: "UNKNOWN_AUTH" } as ProviderInfo;
    useProviders.setState({ providers: [configured], realProviders: [configured] });
    vi.mocked(verifyReasonixApiKey).mockResolvedValue({ ...configured, status: "READY" });
    await useProviders.getState().verifyReasonixKey("deepseek/flash");
    expect(verifyReasonixApiKey).toHaveBeenCalledWith("deepseek/flash");
    expect(useProviders.getState().providers[0].status).toBe("READY");
    expect(useProviders.getState().realProviders[0].status).toBe("READY");
    expect(useProviders.getState().pending).toEqual([]);
    vi.mocked(verifyReasonixApiKey).mockRejectedValue(new Error("invalid key"));
    vi.mocked(refreshProvider).mockResolvedValue({ ...configured, status: "NEEDS_LOGIN", modelVerification: { state: "REJECTED", model: "deepseek/flash", verifiedAt: null, expiresAt: null } });
    useProviders.setState({ providers: [configured], realProviders: [configured] });
    await useProviders.getState().verifyReasonixKey();
    expect(useProviders.getState().providers[0].status).toBe("NEEDS_LOGIN");
    expect(useProviders.getState().providers[0].modelVerification?.state).toBe("REJECTED");
    expect(useProviders.getState().providerErrors.REASONIX).toContain("invalid key");
  });
});


it("discards verification replies after the Reasonix executable source changes", async () => {
  const configured = { id: "REASONIX", status: "UNKNOWN_AUTH" } as ProviderInfo;
  const previousPaths = useSettings.getState().providerPaths;
  useProviders.setState({ providers: [configured], realProviders: [configured] });
  let resolve!: (info: ProviderInfo) => void;
  vi.mocked(verifyReasonixApiKey).mockReturnValue(new Promise((r) => { resolve = r; }));
  const run = useProviders.getState().verifyReasonixKey("deepseek/flash");
  useSettings.setState({ providerPaths: { ...previousPaths, REASONIX: "/another-cli" } });
  resolve({ ...configured, status: "READY" });
  await run;
  expect(useProviders.getState().realProviders[0].status).toBe("UNKNOWN_AUTH");
  expect(useProviders.getState().pending).toEqual([]);
  useSettings.setState({ providerPaths: previousPaths });
});


describe("CLI quota refresh", () => {
  it("publishes the returned snapshot to both provider catalogs", async () => {
    const provider = { ...unverified, id: "COPILOT_CLI", account: { official: true, billing: "SUBSCRIPTION", endpoint: null, plan: null, balance: null, usage: null } } as ProviderInfo;
    useProviders.setState({ providers: [provider], realProviders: [provider] });
    const usage = { windows: [{ windowMinutes: 0, usedPercent: 29, resetsAt: null }], observedAt: "2026-10-21T00:00:00Z", source: "CLI_OUTPUT" as const };
    vi.mocked(queryProviderQuota).mockResolvedValue(result(usage));
    await useProviders.getState().refreshQuota("COPILOT_CLI");
    expect(queryProviderQuota).toHaveBeenCalledWith("COPILOT_CLI", false);
    expect(useProviders.getState().realProviders[0].account?.usage).toEqual(usage);
    expect(useProviders.getState().providers[0].account?.usage).toEqual(usage);
    expect(getProviderUtilizationNow).toHaveBeenCalledWith("COPILOT_CLI", 0);
  });
  it("keeps the existing snapshot when no quota is available", async () => {
    const usage = { windows: [], observedAt: null, source: "CLI_OUTPUT" as const };
    const provider = { ...unverified, id: "COPILOT_CLI", account: { official: true, billing: "SUBSCRIPTION", endpoint: null, plan: null, balance: null, usage } } as ProviderInfo;
    useProviders.setState({ providers: [provider], realProviders: [provider] });
    vi.mocked(queryProviderQuota).mockResolvedValue(result(null));
    await useProviders.getState().refreshQuota("COPILOT_CLI");
    expect(useProviders.getState().realProviders[0].account?.usage).toEqual(usage);
  });
});

it("does not invoke quota IPC when native consent is disabled", async () => {
  vi.mocked(getOnlineUsagePermission).mockResolvedValue({ allowOnlineUsageQuery: false, generation: 1 });
  await useProviders.getState().refreshQuota("CURSOR_CLI");
  expect(queryProviderQuota).not.toHaveBeenCalled();
  expect(useProviders.getState().usagePending).toEqual([]);
});
it("discards a late IPC reply after permission revocation", async () => {
  const provider = { ...unverified, account: { official: true, usage: null } } as ProviderInfo;
  useProviders.setState({ realProviders: [provider], providers: [provider] });
  let finish!: (value: AccountUsageResult) => void;
  vi.mocked(queryProviderQuota).mockReturnValue(new Promise((resolve) => { finish = resolve; }));
  const pending = useProviders.getState().refreshQuota("ANTIGRAVITY_CLI");
  await vi.waitFor(() => expect(queryProviderQuota).toHaveBeenCalled());
  useOnlineUsage.setState({ allowOnlineUsageQuery: false, generation: 1 });
  finish(result({ windows: [], observedAt: null, source: "CLI_OUTPUT" })); await pending;
  expect(useProviders.getState().realProviders[0].account?.usage).toBeNull();
});

it("routes automatic refresh through the same state publication as manual refresh", async () => {
  const provider = { ...unverified, id: "COPILOT_CLI", account: { official: true, usage: null } } as ProviderInfo;
  useProviders.setState({ providers: [provider], realProviders: [provider] });
  const usage: UsageSnapshot = { windows: [{ windowMinutes: 300, usedPercent: 1, resetsAt: null }], observedAt: null, source: "CLI_OUTPUT" };
  vi.mocked(queryProviderQuota).mockResolvedValue(result(usage));
  await useProviders.getState().refreshQuota("COPILOT_CLI", true);
  expect(queryProviderQuota).toHaveBeenCalledWith("COPILOT_CLI", true);
  expect(useProviders.getState().usageResults.COPILOT_CLI?.status).toBe("LATEST");
  expect(useProviders.getState().realProviders[0].account?.usage?.windows[0].usedPercent).toBe(1);
});
it("keeps prior data and timestamp while exposing the current failure", async () => {
  const usage: UsageSnapshot = { windows: [{ windowMinutes: 300, usedPercent: 29, resetsAt: null }], observedAt: "original-time", source: "CLI_OUTPUT" };
  const provider = { ...unverified, id: "COPILOT_CLI", account: { official: true, usage } } as ProviderInfo;
  useProviders.setState({ providers: [provider], realProviders: [provider] });
  const failed: AccountUsageResult = { ...result(usage), status: "FAILED", error: "RATE_LIMITED", fromCache: true };
  vi.mocked(queryProviderQuota).mockResolvedValue(failed);
  await useProviders.getState().refreshQuota("COPILOT_CLI");
  expect(useProviders.getState().usageResults.COPILOT_CLI?.error).toBe("RATE_LIMITED");
  expect(useProviders.getState().realProviders[0].account?.usage?.observedAt).toBe("original-time");
});
it("does not publish old account data after identity changed or became unknown", async () => {
  const provider = { ...unverified, id: "COPILOT_CLI", account: { official: true, billing: "SUBSCRIPTION", endpoint: null, plan: null, usage: { windows: [], observedAt: null, source: "CLI_OUTPUT" }, balance: "old" } } as ProviderInfo;
  useProviders.setState({ providers: [provider], realProviders: [provider] });
  vi.mocked(queryProviderQuota).mockResolvedValue({ ...result(null), status: "IDENTITY_CHANGED", identityKey: null, identityGeneration: null });
  await useProviders.getState().refreshQuota("COPILOT_CLI");
  expect(useProviders.getState().realProviders[0].account?.usage).toBeNull();
  expect(useProviders.getState().realProviders[0].account?.balance).toBeNull();
});

it("keeps a named DeepSeek wallet separate from an OpenCode session account", async () => {
  const account = { official: false, billing: "UNKNOWN", endpoint: "https://opencode.ai", plan: "free", balance: null, usage: null } as ProviderInfo["account"];
  const provider = { ...unverified, id: "OPENCODE", account, usageQuery: { backend: "deepseek", method: "HTTP", state: "IDENTITY_REQUIRED", authentication: "OFFICIAL_DEEPSEEK" } } as ProviderInfo;
  useProviders.setState({ providers: [provider], realProviders: [provider] });
  const wallet = result({ windows: [], observedAt: null, source: "CLI_OUTPUT" });
  wallet.latestSuccess!.scope.backend = "deepseek";
  wallet.latestSuccess!.quantities = [{ id: "CNY/total_balance", unit: "currency", currency: "CNY", used: null, limit: null, remaining: "12.3450" }];
  vi.mocked(queryProviderQuota).mockResolvedValue(wallet);
  await useProviders.getState().refreshQuota("OPENCODE");
  expect(useProviders.getState().realProviders[0].account).toEqual(account);
  expect(useProviders.getState().usageResults.OPENCODE?.latestSuccess?.quantities[0].remaining).toBe("12.3450");
  for (const status of ["IDENTITY_UNKNOWN", "IDENTITY_CHANGED", "FAILED"] as const) {
    vi.mocked(queryProviderQuota).mockResolvedValue({ ...wallet, status, attemptedAt: 2 });
    await useProviders.getState().refreshQuota("OPENCODE");
    expect(useProviders.getState().realProviders[0].account).toEqual(account);
  }
});


it("expires idle model evidence without consuming tokens or changing its independent wallet", async () => {
  vi.useFakeTimers(); vi.setSystemTime(1000);
  const configured: ProviderInfo = { id: "REASONIX", name: "Reasonix", executable: "reasonix", featured: true, status: "READY", version: "2.31.0", path: "/reasonix", authMethod: "api_key",
    models: [], modelGroups: [], modelsSource: "CLI", guidance: [], installMethods: [], trustArgs: [], chat: true, adapter: null, directLimit: null,
    account: { official: true, billing: "PAY_AS_YOU_GO", endpoint: null, plan: null, balance: "CNY 12", usage: null },
    modelVerification: { state: "VALID", model: "deepseek/flash", verifiedAt: 1, expiresAt: 2 } };
  const wallet = { ...result(null), status: "LATEST" as const };
  useProviders.setState({ providers: [configured], realProviders: [configured], usageResults: { REASONIX: wallet } });
  await vi.advanceTimersByTimeAsync(999);
  expect(useProviders.getState().realProviders[0].status).toBe("READY");
  await vi.advanceTimersByTimeAsync(1);
  const p = useProviders.getState().realProviders[0];
  expect(p.status).toBe("UNKNOWN_AUTH");
  expect(p.modelVerification?.state).toBe("EXPIRED");
  expect(p.guidance).toContainEqual({ code: "MODEL_VERIFICATION_EXPIRED", command: null });
  expect(p.account).toBe(configured.account);
  expect(useProviders.getState().usageResults.REASONIX).toBe(wallet);
  expect(verifyReasonixApiKey).not.toHaveBeenCalled();
  expect(queryProviderQuota).not.toHaveBeenCalled();
});


it("marks only explicit model verification as verifying and clears it on completion", async () => {
  const configured = { id: "REASONIX", status: "UNKNOWN_AUTH" } as ProviderInfo;
  useProviders.setState({ providers: [configured], realProviders: [configured] });
  let resolve!: (info: ProviderInfo) => void;
  vi.mocked(verifyReasonixApiKey).mockReturnValue(new Promise((r) => { resolve = r; }));
  const run = useProviders.getState().verifyReasonixKey("deepseek/flash");
  expect(useProviders.getState().modelVerificationPending).toEqual(["REASONIX"]);
  expect(useProviders.getState().pending).toContain("REASONIX");
  resolve({ ...configured, status: "READY" });
  await run;
  expect(useProviders.getState().modelVerificationPending).toEqual([]);
});

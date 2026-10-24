// @vitest-environment happy-dom
import { beforeEach, expect, it, vi } from "vitest";
import type { AccountUsageResult, ProviderId, ProviderInfo, UsageRevisions } from "../lib/ipc";
const f = vi.hoisted(() => ({
  invalidated: null as null | ((r: UsageRevisions) => void),
  detected: null as null | ((e: { scanId: string; provider: ProviderInfo }) => void),
  scan: vi.fn(), one: vi.fn(), summary: vi.fn(), query: vi.fn(), order: [] as string[],
}));
vi.mock("../lib/ipc", async (original) => ({ ...await original<typeof import("../lib/ipc")>(),
  getOnlineUsagePermission: vi.fn().mockResolvedValue({ allowOnlineUsageQuery: true, generation: 0 }),
  onOnlineUsagePermissionChanged: vi.fn().mockResolvedValue(() => {}),
  onUsageBindingInvalidated: vi.fn(async (cb) => { f.invalidated = cb; return () => {}; }),
  onProviderDetected: vi.fn(async (cb) => { f.detected = cb; return () => {}; }),
  onProviderMelted: vi.fn().mockResolvedValue(() => {}), onProviderRecovered: vi.fn().mockResolvedValue(() => {}),
  listProviderHealth: vi.fn().mockResolvedValue([]), getDebugProviderCatalog: vi.fn().mockResolvedValue([]),
  refreshProviders: f.scan, refreshProvider: f.one, getProviderUsageState: f.summary, queryProviderQuota: f.query,
  getProviderUtilizationNow: vi.fn().mockResolvedValue(null),
}));
import { useProviders } from "./providers";
import { useOnlineUsage } from "./onlineUsage";
import { useSettings } from "./settings";
import { startUsagePolling } from "../lib/usagePolling";
const a = { id: "CURSOR_CLI", name: "Cursor", status: "READY", usageQuery: { state: "IDENTITY_REQUIRED", method: "CLI", authentication: "OFFICIAL_SUBSCRIPTION" } } as ProviderInfo;
const b = { ...a, id: "COPILOT_CLI", name: "Copilot" } as ProviderInfo;
function result(id: ProviderId, attemptedAt = 1): AccountUsageResult {
  return { status: "LATEST", identityKey: id, identityGeneration: 1, attemptedAt, fromCache: false, nextRetryAt: null, error: null,
    latestSuccess: { schema: 2, scope: { provider: id, backend: "vendor", endpoint: null, accountLabel: null, organization: null, workspace: null }, identityKey: id, identityGeneration: 1, source: "CLI", fetchedAt: attemptedAt, validUntil: Date.now() / 1000 + 600, windows: [], quantities: [{ id: "requests", unit: "requests", used: "1", limit: "10", remaining: null, currency: null }], display: null } };
}
const deferred = <T,>() => { let resolve!: (v: T) => void; const promise = new Promise<T>((r) => { resolve = r; }); return { promise, resolve }; };
beforeEach(() => {
  vi.clearAllMocks(); f.order = [];
  useOnlineUsage.setState({ ready: true, busy: false, allowOnlineUsageQuery: true, generation: 0 });
  useSettings.setState({ providerEnabled: {}, providerPaths: {} });
  useProviders.setState({ initialized: false, initializing: false, providers: [a,b], realProviders: [a,b], pending: [], scanning: false, usagePending: [], usageActive: [], usageVersions: {}, usageRevisions: { all: 0, providers: {} }, usageResults: {}, usageErrors: {}, providerErrors: {}, error: null });
  f.scan.mockImplementation(async () => { f.order.push("scan"); return [a,b]; });
  f.one.mockImplementation(async (id) => id === a.id ? a : b);
  f.summary.mockImplementation(async (id) => { f.order.push(`restore:${id}`); return { ...result(id), status: "UNVERIFIED" }; });
  f.query.mockImplementation(async (id) => result(id, 10));
});
it("coalesces startup, restores before scan, and does not start polling while initialization is incomplete", async () => {
  const pause = deferred<AccountUsageResult>(); f.summary.mockReturnValueOnce(pause.promise);
  const stop = startUsagePolling();
  const first = useProviders.getState().load(); const second = useProviders.getState().load();
  expect(first).toBe(second);
  await vi.waitFor(() => expect(f.summary).toHaveBeenCalled());
  expect(f.query).not.toHaveBeenCalled(); expect(f.scan).not.toHaveBeenCalled();
  pause.resolve({ ...result(a.id), status: "UNVERIFIED" }); await first;
  expect(f.scan).toHaveBeenCalledTimes(1); expect(useProviders.getState().initialized).toBe(true);
  await vi.waitFor(() => expect(f.query).toHaveBeenCalledTimes(2));
  stop(); await vi.waitFor(() => expect(useProviders.getState().usagePending).toEqual([]));
});
it("refreshing A retains B's complete state and accepts B's already-running reply", async () => {
  await useProviders.getState().load();
  const oldB = result(b.id); useProviders.setState({ usageResults: { [a.id]: result(a.id), [b.id]: oldB } });
  const pause = deferred<AccountUsageResult>(); f.query.mockImplementation((id) => id === b.id ? pause.promise : Promise.resolve(result(id,20)));
  const runningB = useProviders.getState().refreshQuota(b.id);
  await vi.waitFor(() => expect(f.query).toHaveBeenCalledWith(b.id, false));
  f.one.mockImplementation(async () => { f.invalidated!({ all: 0, providers: { CURSOR_CLI: 1 } }); return a; });
  await useProviders.getState().refreshOne(a.id);
  expect(useProviders.getState().usageResults[b.id]).toBe(oldB);
  pause.resolve(result(b.id,30)); await runningB;
  expect(useProviders.getState().usageResults[b.id]?.attemptedAt).toBe(30);
  expect(useProviders.getState().usageResults[a.id]?.attemptedAt).toBe(20);
});
it("source invalidation preserves history, rejects a late old-source reply, and leaves the other card untouched", async () => {
  await useProviders.getState().load(); const oldA=result(a.id), oldB=result(b.id);
  useProviders.setState({ usageResults: { [a.id]: oldA, [b.id]: oldB } });
  const pause=deferred<AccountUsageResult>(); f.query.mockReturnValue(pause.promise);
  const running=useProviders.getState().refreshQuota(a.id); await vi.waitFor(()=>expect(f.query).toHaveBeenCalled());
  f.invalidated!({all:0,providers:{CURSOR_CLI:1}});
  expect(useProviders.getState().usageResults[a.id]?.status).toBe("UNVERIFIED");
  expect(useProviders.getState().usageResults[a.id]?.latestSuccess).toBe(oldA.latestSuccess);
  pause.resolve(result(a.id,99)); await running;
  expect(useProviders.getState().usageResults[a.id]?.attemptedAt).toBe(1);
  expect(useProviders.getState().usageResults[b.id]).toBe(oldB);
});
it("a scan does not clear quota results, errors, or another active query", async () => {
  await useProviders.getState().load(); const prior=result(a.id);
  useProviders.setState({ usageResults:{[a.id]:prior},usageErrors:{[b.id]:"network"} });
  await useProviders.getState().refresh();
  expect(useProviders.getState().usageResults[a.id]).toBe(prior);
  expect(useProviders.getState().usageErrors[b.id]).toBe("network");
});
it("late startup cache cannot overwrite a newer live result in the same generation", async () => {
  const pause=deferred<AccountUsageResult>(); f.summary.mockImplementation((id)=>id===a.id?pause.promise:Promise.resolve(result(id)));
  const startup=useProviders.getState().load(); await vi.waitFor(()=>expect(f.summary).toHaveBeenCalled());
  await useProviders.getState().refreshQuota(a.id);
  pause.resolve({...result(a.id),status:"UNVERIFIED"}); await startup;
  expect(useProviders.getState().usageResults[a.id]?.status).toBe("LATEST");
  expect(useProviders.getState().usageResults[a.id]?.attemptedAt).toBe(10);
});
it("one transport failure is stored only on that card",async()=>{
  f.query.mockRejectedValue(new Error("network"));await useProviders.getState().refreshQuota(a.id);
  expect(useProviders.getState().usageErrors[a.id]).toBe("Error: network");expect(useProviders.getState().usageErrors[b.id]).toBeUndefined();expect(useProviders.getState().error).toBeNull();
});
it("retains card state across two scheduled scans using the real stores and subscriptions",async()=>{
  vi.useFakeTimers({toFake:["Date","setInterval","clearInterval"]});
  const stop=startUsagePolling();
  try {
    await useProviders.getState().load();
    await vi.waitFor(()=>expect(useProviders.getState().usagePending).toEqual([]));
    const first=useProviders.getState().usageResults[a.id];expect(first?.status).toBe("LATEST");
    await vi.advanceTimersByTimeAsync(300_000);
    await vi.waitFor(()=>expect(f.scan).toHaveBeenCalledTimes(2));
    expect(useProviders.getState().usageResults[a.id]).toBe(first);
    await vi.advanceTimersByTimeAsync(300_000);
    await vi.waitFor(()=>expect(f.scan).toHaveBeenCalledTimes(3));
    await vi.waitFor(()=>expect(useProviders.getState().usagePending).toEqual([]));
    expect(useProviders.getState().usageResults[a.id]?.status).toBe("LATEST");
  } finally {stop();vi.useRealTimers();}
});
it("queued work is discarded after consent revocation instead of launching a late IPC",async()=>{
 const c={...a,id:"OPENCODE" as const};useProviders.setState({providers:[a,b,c],realProviders:[a,b,c]});
 const pa=deferred<AccountUsageResult>(),pb=deferred<AccountUsageResult>();
 f.query.mockImplementation((id)=>id===a.id?pa.promise:pb.promise);
 const one=useProviders.getState().refreshQuota(a.id),two=useProviders.getState().refreshQuota(b.id),three=useProviders.getState().refreshQuota(c.id);
 await vi.waitFor(()=>expect(f.query).toHaveBeenCalledTimes(2));
 expect(useProviders.getState().usageActive).not.toContain(c.id);
 useOnlineUsage.setState({allowOnlineUsageQuery:false,generation:1});
 pa.resolve(result(a.id));pb.resolve(result(b.id));await Promise.all([one,two,three]);
 expect(f.query).toHaveBeenCalledTimes(2);expect(useProviders.getState().usageResults).toEqual({});expect(useProviders.getState().usagePending).toEqual([]);
});
it("coalesces overlapping scans even before the event listener promise settles",async()=>{
 await useProviders.getState().load();f.scan.mockClear();
 const pause=deferred<ProviderInfo[]>();f.scan.mockReturnValue(pause.promise);
 const one=useProviders.getState().refresh();const two=useProviders.getState().refresh();expect(one).toBe(two);
 await vi.waitFor(()=>expect(f.scan).toHaveBeenCalledTimes(1));pause.resolve([a,b]);await Promise.all([one,two]);expect(useProviders.getState().scanning).toBe(false);
});

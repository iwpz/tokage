import { beforeEach, expect, it, vi } from "vitest";
import { getOnlineUsagePermission, setOnlineUsagePermission } from "../lib/ipc";
import { useOnlineUsage } from "./onlineUsage";
const observed = vi.hoisted(() => ({ listener: null as null | ((s: { allowOnlineUsageQuery: boolean; generation: number }) => void) }));
vi.mock("../lib/ipc", () => ({
  getOnlineUsagePermission: vi.fn(), setOnlineUsagePermission: vi.fn(),
  onOnlineUsagePermissionChanged: vi.fn(async (callback) => { observed.listener = callback; return () => {}; }),
}));
beforeEach(() => {
  useOnlineUsage.setState({ allowOnlineUsageQuery: false, generation: 0, ready: false, busy: false, error: null });
  vi.mocked(getOnlineUsagePermission).mockReset();
  vi.mocked(setOnlineUsagePermission).mockReset();
});
it("waits for native consent and fails closed when reading fails", async () => {
  vi.mocked(getOnlineUsagePermission).mockResolvedValue({ allowOnlineUsageQuery: true, generation: 2 });
  await useOnlineUsage.getState().load();
  expect(useOnlineUsage.getState()).toMatchObject({ ready: true, allowOnlineUsageQuery: true, generation: 2 });
  vi.mocked(getOnlineUsagePermission).mockRejectedValue(new Error("unavailable"));
  await useOnlineUsage.getState().load();
  expect(useOnlineUsage.getState()).toMatchObject({ ready: false, allowOnlineUsageQuery: false });
});
it("does not grant consent when native persistence fails", async () => {
  vi.mocked(setOnlineUsagePermission).mockRejectedValue(new Error("save failed"));
  await useOnlineUsage.getState().update(true);
  expect(useOnlineUsage.getState()).toMatchObject({ allowOnlineUsageQuery: false, busy: false, error: "Error: save failed" });
});
it("blocks publication immediately while disabling is pending", async () => {
  useOnlineUsage.setState({ ready: true, allowOnlineUsageQuery: true });
  let finish!: (value: { allowOnlineUsageQuery: boolean; generation: number }) => void;
  vi.mocked(setOnlineUsagePermission).mockReturnValue(new Promise((resolve) => { finish = resolve; }));
  const pending = useOnlineUsage.getState().update(false);
  expect(useOnlineUsage.getState()).toMatchObject({ allowOnlineUsageQuery: false, busy: true });
  finish({ allowOnlineUsageQuery: false, generation: 1 }); await pending;
  expect(useOnlineUsage.getState()).toMatchObject({ allowOnlineUsageQuery: false, generation: 1, busy: false });
});
it("cannot restore a stale enabled read over a newer revocation event", async () => {
  let finish!: (value: { allowOnlineUsageQuery: boolean; generation: number }) => void;
  vi.mocked(getOnlineUsagePermission).mockReturnValue(new Promise((resolve) => { finish = resolve; }));
  const pending = useOnlineUsage.getState().load();
  await vi.waitFor(() => expect(getOnlineUsagePermission).toHaveBeenCalled());
  const callback = observed.listener!;
  callback({ allowOnlineUsageQuery: false, generation: 5 });
  finish({ allowOnlineUsageQuery: true, generation: 4 }); await pending;
  expect(useOnlineUsage.getState()).toMatchObject({ allowOnlineUsageQuery: false, generation: 5 });
});

import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ProviderInfo } from "../lib/ipc";
import { verifyAntigravityApiKey, verifyReasonixApiKey } from "../lib/ipc";
import { useProviders } from "./providers";

vi.mock("../lib/ipc", async (importOriginal) => ({
  ...await importOriginal<typeof import("../lib/ipc")>(),
  verifyAntigravityApiKey: vi.fn(),
  verifyReasonixApiKey: vi.fn(),
}));
const unverified = { id: "ANTIGRAVITY_CLI", status: "UNKNOWN_AUTH", authMethod: "Gemini API key" } as ProviderInfo;

beforeEach(() => {
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

// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { renderToString } from "react-dom/server";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import en from "../../i18n/locales/en";
import { ReasonixVerification } from "./ReasonixVerification";
import { ProviderCard } from "./CliSettings";
import { useProviders } from "../../stores/providers";
import type { ProviderInfo } from "../../lib/ipc";

vi.mock("react-i18next", async (importOriginal) => ({ ...await importOriginal<typeof import("react-i18next")>(), useTranslation: () => ({ i18n: { language: "en" }, t: (key: string, args?: Record<string, unknown>) => {
  let text = key.split(".").reduce((v: any, p) => v?.[p], en) ?? key;
  for (const [k, v] of Object.entries(args ?? {})) text = text.replaceAll(`{{${k}}}`, String(v));
  return text;
} }) }));
beforeEach(() => vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true));
afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); });

it("distinguishes expired evidence, login rejection and independent wallet success", () => {
  const onVerify = vi.fn();
  const html = renderToString(<ReasonixVerification models={["deepseek/flash"]} busy={false} disabled={false} onVerify={onVerify}
    proof={{ state: "EXPIRED", model: "deepseek/flash", verifiedAt: 1, expiresAt: 86401 }} />);
  expect(html).toContain("Verification evidence expired");
  expect(html).toContain("not been confirmed invalid");
  expect(html).toContain("Wallet queries are independent");
  expect(html).toContain("uses tokens");
  expect(html).toContain("deepseek/flash");
  expect(onVerify).not.toHaveBeenCalled();
  const rejected = renderToString(<ReasonixVerification models={[]} busy={false} disabled={false} onVerify={onVerify}
    proof={{ state: "REJECTED", model: "deepseek/flash", verifiedAt: null, expiresAt: null }} error="Model verification failed" />);
  expect(rejected).toContain("rejected authentication");
  expect(rejected).toContain('role="alert"');
  expect(rejected).not.toContain("Verified:");
});

it("requires explicit model selection and click and disables duplicate requests", async () => {
  const onVerify = vi.fn().mockResolvedValue(undefined);
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  const props = { models: ["deepseek/flash", "deepseek/pro"], busy: false, disabled: false, onVerify };
  await act(async () => root.render(createElement(ReasonixVerification, props)));
  const select = container.querySelector("select")!;
  const button = container.querySelector("button")!;
  expect(button.disabled).toBe(true);
  expect(onVerify).not.toHaveBeenCalled();
  await act(async () => { select.value = "deepseek/pro"; select.dispatchEvent(new Event("change", { bubbles: true })); });
  expect(button.disabled).toBe(false);
  expect(onVerify).not.toHaveBeenCalled();
  await act(async () => button.click());
  expect(onVerify).toHaveBeenCalledExactlyOnceWith("deepseek/pro");
  await act(async () => root.render(createElement(ReasonixVerification, { ...props, busy: true })));
  expect(select.disabled).toBe(true);
  expect(button.disabled).toBe(true);
  await act(async () => root.unmount());
  container.remove();
});


it("does not label an ordinary status scan as a token-consuming model verification", async () => {
  const p: ProviderInfo = { id: "REASONIX", name: "Reasonix", executable: "reasonix", featured: true, status: "READY", version: "2.31.0", path: "/reasonix", authMethod: "api_key",
    models: [], modelGroups: [], modelsSource: "CLI", account: null, guidance: [], installMethods: [], trustArgs: [], chat: true, adapter: null, directLimit: null,
    modelVerification: { state: "VALID", model: "deepseek/flash", verifiedAt: 1, expiresAt: Math.floor(Date.now() / 1000) + 86400 } };
  useProviders.setState({ pending: ["REASONIX"], modelVerificationPending: [], scanning: false, initializing: false });
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  try {
    await act(async () => root.render(<ProviderCard provider={p} removable={false} />));
    expect(container.textContent).toContain("Verify selected model (uses tokens)");
    expect(container.textContent).not.toContain("Verifying selected model");
  } finally {
    await act(async () => root.unmount());
    useProviders.setState({ pending: [] });
    container.remove();
  }
});

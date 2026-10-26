// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { renderToString } from "react-dom/server";
import { afterEach, expect, it, vi } from "vitest";
import en from "../../i18n/locales/en";
import { ReasonixVerification } from "./ReasonixVerification";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ i18n: { language: "en" }, t: (key: string, args?: Record<string, unknown>) => {
  let text = key.split(".").reduce((v: any, p) => v?.[p], en) ?? key;
  for (const [k, v] of Object.entries(args ?? {})) text = text.replaceAll(`{{${k}}}`, String(v));
  return text;
} }) }));
afterEach(() => vi.useRealTimers());

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

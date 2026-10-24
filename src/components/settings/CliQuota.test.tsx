import { expect, it, vi } from "vitest";
import { renderToString } from "react-dom/server";
import type { AccountUsageResult } from "../../lib/ipc";
import en from "../../i18n/locales/en";
import { CliQuota } from "./CliQuota";
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string, args?: Record<string, unknown>) => {
  let text = key.split(".").reduce((v: any, p) => v?.[p], en) ?? key;
  for (const [k, v] of Object.entries(args ?? {})) text = text.replaceAll(`{{${k}}}`, String(v));
  return text;
} }) }));
function result(): AccountUsageResult {
  return { status: "LATEST", identityKey: "opaque", identityGeneration: 1, fromCache: false, attemptedAt: 1, nextRetryAt: null, error: null,
    latestSuccess: { schema: 2, scope: { provider: "KIRO_CLI", backend: "kiro", endpoint: null, accountLabel: null, organization: null, workspace: null }, identityKey: "opaque", identityGeneration: 1, fetchedAt: 1, validUntil: 600, source: "CLI", windows: [
      { id: "estimated-plan-credits", scope: "plan:estimated", period: "Estimated · KIRO PRO+ · 2026-11-01", windowMinutes: null, usedPercent: 6.2, resetsAt: null },
    ], quantities: [{ id: "estimated-plan-credits", unit: "credits", currency: null, remaining: null, limit: "2000", used: "123.40" }], display: null } };
}
it("keeps Kiro estimate, date-only reset and exact credit units visible", () => {
  const html = renderToString(<CliQuota result={result()} />);
  for (const text of ["Estimated", "KIRO PRO+", "2026-11-01", "123.40", "2000", "credits", "93.8"]) expect(html).toContain(text);
  expect(html).not.toContain("USD"); expect(html).not.toContain("tokens");
});
it("labels failed-query quota as historical rather than current", () => {
  const html = renderToString(<CliQuota result={{ ...result(), status: "FAILED" }} />);
  expect(html).toContain("Historical snapshot"); expect(html).not.toContain("Latest query");
});

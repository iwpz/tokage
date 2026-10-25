import { expect, it, vi } from "vitest";
import { renderToString } from "react-dom/server";
import type { AccountUsageResult } from "../../lib/ipc";
import en from "../../i18n/locales/en";
import { CliWallet } from "./CliWallet";
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string, args?: Record<string, unknown>) => {let text = key.split(".").reduce((v: any, p) => v?.[p], en) ?? key; for(const [k,v] of Object.entries(args??{})) text=text.replaceAll(`{{${k}}}`,String(v));return text;} }) }));
function result(): AccountUsageResult {
  return { status: "LATEST", identityKey: "opaque", identityGeneration: 1, fromCache: false, attemptedAt: 1, nextRetryAt: null, error: null,
    latestSuccess: { schema: 2, available: false, scope: { provider: "OPENCODE", backend: "deepseek", endpoint: "https://api.deepseek.com", accountLabel: null, organization: null, workspace: null }, identityKey: "opaque", identityGeneration: 1, fetchedAt: 1, validUntil: 600, source: "HTTP", windows: [], quantities: [
      { id: "CNY/total_balance", unit: "currency", currency: "CNY", remaining: "000.0100", limit: null, used: null },
      { id: "CNY/granted_balance", unit: "currency", currency: "CNY", remaining: "0.0000", limit: null, used: null },
      { id: "CNY/topped_up_balance", unit: "currency", currency: "CNY", remaining: "000.0100", limit: null, used: null },
    ], display: null } };
}
it("shows an explicitly named wallet without rounding or a fake percentage", () => {
  const html = renderToString(<CliWallet result={result()} />);
  for (const text of ["independent of the current session model", "Total balance", "Granted balance", "Topped-up balance", "000.0100", "0.0000", "insufficient balance", "Latest query"]) expect(html).toContain(text);
  expect(html).not.toContain('role="meter"'); expect(html).not.toContain("5h");
});
it("keeps failed or changed-source data visibly historical", () => {
  for (const status of ["FAILED", "IDENTITY_CHANGED", "IDENTITY_UNKNOWN"] as const) {
    const html = renderToString(<CliWallet result={{ ...result(), status, fromCache: true }} />);
    expect(html).toContain("Historical snapshot"); expect(html).not.toContain("Latest query");
  }
});
it("does not display a different backend wallet as DeepSeek", () => {
  const r = result(); r.latestSuccess!.scope.backend = "other";
  const html = renderToString(<CliWallet result={r} />);
  expect(html).not.toContain("000.0100"); expect(html).toContain("Not queried yet");
});

it("distinguishes a verified cached result from a new query and unverified history", () => {
  const html = renderToString(<CliWallet result={{ ...result(), fromCache: true }} />);
  expect(html).toContain("Cached query"); expect(html).not.toContain("Latest query"); expect(html).not.toContain("Historical snapshot");
});

it("keeps Aider wallet provenance and rejects another CLI snapshot", () => {
  const r=result();r.latestSuccess!.scope.provider="AIDER";
  const html=renderToString(<CliWallet provider="AIDER" result={r}/>);
  expect(html).toContain("configured for Aider");expect(html).not.toContain("configured for OpenCode");expect(html).toContain("000.0100");
  r.latestSuccess!.scope.provider="REASONIX";
  expect(renderToString(<CliWallet provider="AIDER" result={r}/>)).not.toContain("000.0100");
});

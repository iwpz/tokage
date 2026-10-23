import { expect, it, vi } from "vitest";
import { renderToString } from "react-dom/server";
import type { UsageSnapshot } from "../../lib/ipc";
import en from "../../i18n/locales/en";
import { CliUsage } from "./CliUsage";

vi.mock("react-i18next", () => ({ useTranslation: () => ({
  i18n: { language: "en" },
  t: (key: string, values: Record<string, string> = {}) => {
    const message = key.split(".").reduce((value: any, part) => value?.[part], en) ?? key;
    return String(message).replace(/{{(\w+)}}/g, (_, name: string) => values[name] ?? "");
  },
}) }));

it("shows independent Cursor categories and disabled on-demand without a fake meter", () => {
  const usage: UsageSnapshot = {
    windows: [{ windowMinutes: 0, usedPercent: 33, resetsAt: null }],
    observedAt: "2026-10-24T02:00:00Z", source: "CLI_OUTPUT",
    details: { plan: "Pro+", resetLabel: "Resets Nov 6", rows: [
      { kind: "INCLUDED", usedPercent: 33, value: "33% used" },
      { kind: "AUTO", usedPercent: 34, value: "34% used" },
      { kind: "API", usedPercent: 8, value: "8% used" },
      { kind: "ON_DEMAND", usedPercent: null, value: "Disabled" },
    ] },
  };
  const html = renderToString(<CliUsage usage={usage} />);
  for (const label of ["Pro+", "Resets Nov 6", "Included", "Auto", "API", "On-Demand", "Disabled", "CLI usage command"]) expect(html).toContain(label);
  expect(html.match(/role="meter"/g)).toHaveLength(3);
  expect(html).toContain('aria-valuenow="33"');
  expect(html).toContain('aria-valuenow="34"');
  expect(html).toContain('aria-valuenow="8"');
  expect(html).not.toContain('aria-valuenow="75"');
  expect(html).not.toContain("5h");
});

it("keeps legacy window snapshots readable", () => {
  const html = renderToString(<CliUsage usage={{ windows: [{ windowMinutes: 10080, usedPercent: 29, resetsAt: null }], observedAt: null, source: "SESSION_LOG" }} />);
  expect(html).toContain("Weekly");
  expect(html).toContain('aria-valuenow="29"');
});

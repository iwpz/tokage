import { describe, expect, it, vi } from "vitest";
import { renderToString } from "react-dom/server";
import { StatusDot } from "./ProviderBadge";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

describe("provider mode label", () => {
  it("shows gateway mode for usable gateways", () => {
    expect(renderToString(<StatusDot status="READY" gateway withLabel />)).toContain("status.GATEWAY");
    expect(renderToString(<StatusDot status="UNSUPPORTED_VERSION" gateway withLabel />)).toContain("status.GATEWAY");
  });
  it("keeps login and health failures visible for gateways", () => {
    for (const status of ["NEEDS_LOGIN", "UNKNOWN_AUTH", "MELTED", "DISABLED"] as const) {
      expect(renderToString(<StatusDot status={status} gateway withLabel />)).toContain(`status.${status}`);
    }
    expect(renderToString(<StatusDot status="READY" withLabel />)).toContain("status.READY");
  });
});

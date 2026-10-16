import { describe, expect, it, vi } from "vitest";
import { renderToString } from "react-dom/server";
import en from "../i18n/locales/en";
import zh from "../i18n/locales/zh";
import support from "../lib/reliabilitySupport.json";
import { ProviderReliability } from "./ProviderReliability";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string, values: Record<string, string> = {}) => {
    const message = en.reliability[key.replace("reliability.", "") as keyof typeof en.reliability];
    return message.replace(/{{(\w+)}}/g, (_, name: string) => values[name] ?? "");
  } }),
}));

describe("reliability scope disclosure", () => {
  it("never promotes an exact historical CLI version to current live or desktop acceptance", () => {
    for (const target of support.targets) {
      const html = renderToString(<ProviderReliability providerId={target.provider} version={target.version} />);
      expect(html).toContain(target.model);
      expect(html).toContain(target.platform);
      expect(html).toContain("live CLI pending");
      expect(html).toContain("natural-language desktop end-to-end pending");
      expect(html).toContain("not current acceptance");
      expect(html).not.toContain("differs from the recorded target");
    }
  });
  it("flags upgrades, prefixed versions and absent version evidence as unverified", () => {
    for (const version of ["2.1.297", "v2.1.296", null]) {
      expect(renderToString(<ProviderReliability providerId="CLAUDE_CODE" version={version} />)).toContain("differs from the recorded target; unverified");
    }
  });
  it("preserves unselected and unknown providers as outside the round", () => {
    for (const providerId of ["CODEX_CLI", "AIDER", "OPENCODE_V1", "UNKNOWN"]) {
      const html = renderToString(<ProviderReliability providerId={providerId} version="2.0.29" />);
      expect(html).toContain("Outside this round");
      expect(html).not.toContain("M2 target:");
    }
  });
  it("keeps readiness and current acceptance distinct in both languages", () => {
    expect(en.reliability.probeOnly).toContain("not reliability acceptance");
    expect(zh.reliability.probeOnly).toContain("不代表可靠性验收通过");
    expect(zh.reliability.pending).toContain("待复验");
    expect(support.currentLive).toBe("pending");
    expect(support.currentDesktop).toBe("pending");
  });
});

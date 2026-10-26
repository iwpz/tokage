import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { ContextBar } from "./ContextBar";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

describe("deferred context actions", () => {
  it("discloses the boundary even at high usage and offers no action button", () => {
    const html = renderToStaticMarkup(<ContextBar tokens={120_000} maxTokens={128_000} />);
    expect(html).toContain("workspace.context.deferred");
    expect(html).toContain('role="progressbar"');
    expect(html).not.toContain("<button");
    expect(html).not.toContain('data-testid="context-suggestion-bar"');
  });
});

it("shows unavailable inputs without reporting a measured zero percentage",()=>{
 const html=renderToStaticMarkup(<ContextBar tokens={0} known={false} source="unavailable"/>);
 expect(html).toContain("workspace.context.unavailableInput");expect(html).not.toContain('aria-valuenow="0"');expect(html).toContain("unavailable");
});

it("does not label an expired persistent cache warm",()=>{
 const html=renderToStaticMarkup(<ContextBar tokens={100} cacheType="disk_persistent" isWarm={false} ttlRemainingSeconds={0}/>);
 expect(html).toContain("workspace.context.cache.cold");expect(html).not.toContain("workspace.context.cache.persistent");
});

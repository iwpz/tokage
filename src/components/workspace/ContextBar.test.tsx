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

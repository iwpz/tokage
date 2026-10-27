import { renderToStaticMarkup } from "react-dom/server";
import { expect, it, vi } from "vitest";
vi.mock("react-i18next", () => ({useTranslation: () => ({t: (key:string) => key})}));
import { CompactPayload } from "./CompactPayload";

it("shows the exact prepared content as safely escaped text in an optional disclosure", () => {
  expect(renderToStaticMarkup(<CompactPayload />)).toBe("");
  const html = renderToStaticMarkup(<CompactPayload payload={'Keep <scope>\nSecond line'} />);
  expect(html).toContain("<details");
  expect(html).toContain("workspace.context.preparedPayload");
  expect(html).toContain("Keep &lt;scope&gt;\nSecond line");
});

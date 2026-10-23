import { expect, it, vi } from "vitest";
import { renderToString } from "react-dom/server";
const fixtures = vi.hoisted(() => ({ state: {} as any }));
vi.mock("../../stores/runtime", () => ({ useRuntime: (select: (state: unknown) => unknown) => select(fixtures.state), observeRuntime: vi.fn() }));
import "../../i18n";
import { runtimeUiSnapshot as snapshot, runtimeUiTranscripts as transcripts } from "../../../tests/fixtures/runtime-ui";
import { RuntimeView } from "./RuntimeView";
fixtures.state = { runs: { run: { snapshot, transcripts } }, errors: {} };
it("renders original chat messages plus structured task cards without wire metadata", () => {
 const html = renderToString(<RuntimeView sessionId="run" view="chat" />);
 expect(html).toContain('role="log"'); expect(html).toContain("Answer</h3>"); expect(html).toContain("<strong>Hello</strong>"); expect(html).toContain("Implement feature"); expect(html).toContain("src/file.ts"); expect(html).not.toContain("hidden-wire-thread"); expect(html).not.toContain("<textarea");
});
it("renders the existing structured Spec editor instead of a raw plan Markdown substitute", () => {
 const html = renderToString(<RuntimeView sessionId="run" view="plan" />);
 expect(html).toContain("Structured feature goal"); expect(html).toContain("Preserve API"); expect(html).toContain('role="tablist"'); expect(html).not.toContain("Unstructured fallback");
});
it("renders board controls and model assignment in pipeline, without the chat or plan body", () => {
 const html = renderToString(<RuntimeView sessionId="run" view="pipeline" />);
 expect(html).toContain("Implement feature"); expect(html).toContain('aria-pressed="true"'); expect(html).toContain("haiku"); expect(html).toContain("Review and approve"); expect(html).not.toContain('role="log"'); expect(html).not.toContain("Structured feature goal"); expect(html).not.toContain("Generate plan");
});

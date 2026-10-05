import { describe, it, expect } from "vitest";
import { renderToString } from "react-dom/server";
import { UsageDashboardView } from "./UsageDashboardView";
import { ErrorBoundary } from "../ErrorBoundary";

describe("UsageDashboardView", () => {
  it("renders without crashing when data is null", () => {
    expect(() => {
      renderToString(<UsageDashboardView />);
    }).not.toThrow();
  });

  it("renders within ErrorBoundary smoothly", () => {
    expect(() => {
      renderToString(
        <ErrorBoundary>
          <UsageDashboardView />
        </ErrorBoundary>
      );
    }).not.toThrow();
  });
});

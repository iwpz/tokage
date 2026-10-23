import { expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import "../../i18n";
import { runtimeUiSnapshot } from "../../../tests/fixtures/runtime-ui";
import { RuntimeDeliveryEvidence } from "./RuntimeDeliveryEvidence";

it("shows only the current review identity and durable evidence, collapsed by default", () => {
  const snapshot = structuredClone(runtimeUiSnapshot);
  snapshot.state = "AwaitingMerge";
  snapshot.turns = { old: { ...snapshot.turns!.planner, observedModel: "old-model" }, current: { ...snapshot.turns!.planner, observedModel: "actual-model", cleanupConfirmed: true, costUsd: 0 } };
  snapshot.delivery = { reviewId: "current", candidate: { head: "candidate-sha" }, review: { verdict: "PASS", findings: [] }, artifact: "delivered-artifact" };
  const html = renderToStaticMarkup(<RuntimeDeliveryEvidence snapshot={snapshot}/>);
  expect(html).toContain("Claude · Actual Model");
  expect(html).not.toContain("Old Model");
  expect(html).toContain("$0.000000");
  expect(html).toContain("candidate-sha");
  expect(html).toContain("delivered-artifact");
  expect(html).toContain("PASS");
  expect(html).not.toContain(" open=");
});

it("keeps unknown cost and missing accepted results explicit while reviewing", () => {
  const snapshot = structuredClone(runtimeUiSnapshot);
  snapshot.state = "Reviewing";
  snapshot.delivery = { reviewId: "missing", error: "review receipt missing" };
  const html = renderToStaticMarkup(<RuntimeDeliveryEvidence snapshot={snapshot}/>);
  expect(html).toContain('aria-busy="true"');
  expect(html).toContain('role="status"');
  expect(html).toContain("Observed model unknown");
  expect(html).toContain("Cost Unknown");
  expect(html).toContain("No accepted review result recorded");
  expect(html).toContain("review receipt missing");
  expect(html).not.toContain("PASS");
});

it("does not introduce review evidence before delivery preparation", () => {
  expect(renderToStaticMarkup(<RuntimeDeliveryEvidence snapshot={{ ...runtimeUiSnapshot, delivery: undefined }}/>)).toBe("");
});

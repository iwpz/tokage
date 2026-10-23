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
 expect(html).toContain("Implement feature"); expect(html).toContain('aria-pressed="true"'); expect(html).toContain("Claude · Haiku"); expect(html).toContain("Review and approve"); expect(html).not.toContain('role="log"'); expect(html).not.toContain("Structured feature goal"); expect(html).not.toContain("Generate plan");
});

it("uses one pipeline header and restores chat navigation inside the task board", () => {
 const pipeline=renderToString(<RuntimeView sessionId="run" view="pipeline"/>);
 expect(pipeline.split("Execution pipeline board")).toHaveLength(2);
 expect(pipeline).not.toContain("Technical plan sub-agent board");
 expect(pipeline).toContain("Back to review");
 const chat=renderToString(<RuntimeView sessionId="run" view="chat"/>);
 expect(chat).toContain("View pipeline DAG");
 expect(chat).not.toContain("Plan execution ·");
});

it("shares the bottom action panel between Plan and Pipeline without mounting it in Chat", () => {
 for (const view of ["plan","pipeline"] as const) {
  const html=renderToString(<RuntimeView sessionId="run" view={view}/>);
  expect(html).toContain('<footer data-testid="runtime-action-panel"');
  expect(html.lastIndexOf("runtime-action-panel")).toBeGreaterThan(html.indexOf(view==="plan"?"runtime-plan-body":"Implement feature"));
 }
 expect(renderToString(<RuntimeView sessionId="run" view="chat"/>)).not.toContain('runtime-action-panel');
});
it("explains blocked post-task runs from recorded unknown-cost policy evidence", () => {
 const blocked=structuredClone(snapshot);blocked.state="Blocked";blocked.turns!.planner.costUsd=null;
 fixtures.state={runs:{run:{snapshot:blocked,transcripts}},errors:{}};
 try {
  const html=renderToString(<RuntimeView sessionId="run" view="pipeline"/>);
  expect(html).toContain("Run blocked — reason");
  expect(html).toContain("Actual cost was not reported");
  expect(html).toContain("final review and delivery have not passed");
 } finally { fixtures.state={runs:{run:{snapshot,transcripts}},errors:{}}; }
});
it("shows unconfirmed cleanup before a historical idle error in Plan and Pipeline",()=>{
 const blocked=structuredClone(snapshot);blocked.state="Blocked";
 blocked.turns!.planner.error="native process idle; lost completion cannot be accepted";
 blocked.turns!.pending={...blocked.turns!.planner,id:"pending",role:"executor",taskId:"T1",status:"Unknown",ended:false,cleanupConfirmed:false,costUsd:null};
 fixtures.state={runs:{run:{snapshot:blocked,transcripts}},errors:{}};
 try {
  for(const view of ["plan","pipeline"] as const){
   const html=renderToString(<RuntimeView sessionId="run" view={view}/>);
   expect(html).toContain("Process cleanup has not been confirmed for T1");
   expect(html).toContain("Reconcile interrupted run");
   expect(html.indexOf("Process cleanup has not been confirmed")).toBeLessThan(html.indexOf("native process idle"));
  }
 } finally {fixtures.state={runs:{run:{snapshot,transcripts}},errors:{}};}
});
it("does not show interrupted attempts as actively running on a blocked board",()=>{
 const blocked=structuredClone(snapshot);blocked.state="Blocked";
 blocked.execution={tasks:{T1:{state:"RUNNING",attempts:["pending"],assignment:blocked.plan!.assignments.T1,error:null}}} as any;
 blocked.turns!.pending={...blocked.turns!.planner,id:"pending",role:"executor",taskId:"T1",status:"Unknown",ended:false,cleanupConfirmed:false,costUsd:null};
 fixtures.state={runs:{run:{snapshot:blocked,transcripts}},errors:{}};
 try {
  for(const status of ["Unknown","Failed"]){
   blocked.turns!.pending.status=status;blocked.turns!.pending.ended=status==="Failed";
   const html=renderToString(<RuntimeView sessionId="run" view="pipeline"/>);
   expect(html).toContain("Awaiting reconciliation");
   expect(html).not.toContain(">RUNNING<");
   expect(blocked.execution!.tasks.T1.state).toBe("RUNNING");
  }
  blocked.turns!.pending.status="Running";blocked.turns!.pending.ended=false;
  expect(renderToString(<RuntimeView sessionId="run" view="pipeline"/>)).toContain(">RUNNING<");
  blocked.turns!.pending.status="Unknown";blocked.turns!.pending.taskId="other-task";
  expect(renderToString(<RuntimeView sessionId="run" view="pipeline"/>)).toContain(">RUNNING<");
 } finally {fixtures.state={runs:{run:{snapshot,transcripts}},errors:{}};}
});

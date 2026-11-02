import {it,expect} from "vitest";
import {renderToString} from "react-dom/server";
import "../../i18n";
import {RuntimeRecoveryPanel} from "./RuntimeRecoveryPanel";
import {runtimeUiSnapshot} from "../../../tests/fixtures/runtime-ui";
it("retains failed and older artifacts, gate output, workspace, branch and reports",()=>{
 const s=structuredClone(runtimeUiSnapshot);s.state="Blocked";s.work={attempts:{old:{workspace:{root:"/kept",cwd:"/kept/failed",branch:"tokage/old"},error:"scope refused",gates:[],report:{summary:"human help recorded"}},current:{commit:"candidate-sha",integratedHead:"integration-sha",workspace:{root:"/kept",cwd:"/kept/current",branch:"tokage/current"},gates:[{id:"gate",command:"npm test",cleanupConfirmed:true,result:{passed:false,exitCode:17,outputTail:"assertion evidence",errorMessage:"rejected"}}]}}};
 const html=renderToString(<RuntimeRecoveryPanel snapshot={s} onRefresh={()=>{}}/>).replace(/<!--.*?-->/g,"");
 for(const text of ["/kept/failed","tokage/old","/kept/current","tokage/current","candidate-sha","integration-sha","assertion evidence","human help recorded","17","Last confirmed:","Retry reading run"])expect(html).toContain(text);
 expect(html).not.toContain("Automatically recovered");
});
it("does not render a stale terminal snapshot as newly verified after a read failure",()=>{
 const s=structuredClone(runtimeUiSnapshot);s.state="Delivered";
 const html=renderToString(<RuntimeRecoveryPanel snapshot={s} readError="repository unavailable or corrupt: disk full" onRefresh={()=>{}} onShowPipeline={()=>{}}/>);
 expect(html).toContain("last confirmed snapshot");expect(html).toContain("disk full");expect(html).toContain("Do not edit the database");expect(html).toContain("View pipeline DAG");
});
it("shows owner conflict and command uncertainty without prescribing lock deletion",()=>{
 const s=structuredClone(runtimeUiSnapshot);
 expect(renderToString(<RuntimeRecoveryPanel snapshot={s} commandError="run owner conflict" onRefresh={()=>{}}/>)).toContain("Another owner holds this run");
 expect(renderToString(<RuntimeRecoveryPanel snapshot={s} commandError="connection lost" onRefresh={()=>{}}/>)).toContain("may have been accepted");
});

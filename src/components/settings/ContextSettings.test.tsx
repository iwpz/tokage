// @vitest-environment happy-dom
import {act} from "react";
import {createRoot} from "react-dom/client";
import {it,expect,vi} from "vitest";
import i18n from "../../i18n";
vi.mock("../../lib/ipc",async original=>({...await original<object>(),contextCalibrationStatus:vi.fn(async()=>({source:"SETTLED_NATIVE_TOKEN_PRICE_V1",eligibleSamples:2,groups:[{priceKey:"key",provider:"P",model:"M",samples:2,minimumSamples:5,factor:null,status:"INSUFFICIENT_SAMPLES"}],ignoredReasons:{REPORTED_USD_UNKNOWN:1},overrides:[]}))}));
import {contextCalibrationStatus} from "../../lib/ipc";
import {ContextSettings} from "./OtherSettings";
import {useSettings} from "../../stores/settings";
import {DEFAULT_CONTEXT_TUNING} from "../../lib/contextTuning";
it("shows insufficient samples explicitly and edits shared persisted budgets without native dispatch",async()=>{
 (globalThis as any).IS_REACT_ACT_ENVIRONMENT=true;await i18n.changeLanguage("en");
 useSettings.setState({contextTuning:{...DEFAULT_CONTEXT_TUNING}});
 const host=document.createElement("div");document.body.append(host);const root=createRoot(host);
 await act(async()=>{root.render(<ContextSettings/>);});
 expect(contextCalibrationStatus).toHaveBeenCalledWith(5);expect(host.textContent).toContain("Insufficient samples");expect(host.textContent).toContain("2/5");expect(host.textContent).not.toContain("Local factor ×");
 const input=host.querySelector('input[aria-label="Retained brief budget"]') as HTMLInputElement;
 expect(input).not.toBeNull();await act(async()=>{Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,"value")!.set!.call(input,"1800");input.dispatchEvent(new Event("input",{bubbles:true}));});
 expect(useSettings.getState().contextTuning.briefTokenLimit).toBe(1800);
 await act(async()=>root.unmount());host.remove();useSettings.setState({contextTuning:{...DEFAULT_CONTEXT_TUNING}});
});

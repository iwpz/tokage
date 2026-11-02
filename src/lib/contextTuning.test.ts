import {it,expect} from "vitest";
import {DEFAULT_CONTEXT_TUNING,validContextTuning} from "./contextTuning";
import {migrateSettings,SETTINGS_VERSION} from "../stores/settings";
import {progressReportRequest} from "./compactContext";
it("migrates absent tuning and preserves explicit zero rework and nondefault budgets",()=>{
 const migrated=migrateSettings({settingsVersion:7});expect(migrated.contextTuning).toEqual(DEFAULT_CONTEXT_TUNING);expect(migrated.settingsVersion).toBe(SETTINGS_VERSION);
 const tuning={briefTokenLimit:1800,reportTokenLimit:240,reworkPhi:0,calibrationMinSamples:8};expect(migrateSettings({settingsVersion:7,contextTuning:tuning}).contextTuning).toEqual(tuning);
 expect(validContextTuning({...tuning,reportTokenLimit:1900})).toBe(false);expect(validContextTuning({...tuning,reworkPhi:NaN})).toBe(false);
 expect(migrateSettings({settingsVersion:8,contextTuning:{...tuning,reworkPhi:Infinity}}).contextTuning).toEqual(DEFAULT_CONTEXT_TUNING);
});
it("report requests use the configured budget while retaining the original task-safe contract",()=>{
 expect(progressReportRequest(240)).toContain("within 240 tokens");expect(progressReportRequest(240)).toContain("Do not edit files");expect(progressReportRequest()).toContain("within 300 tokens");
});

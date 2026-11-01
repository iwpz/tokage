export interface ContextTuning {briefTokenLimit:number;reportTokenLimit:number;reworkPhi:number;calibrationMinSamples:number}
export const DEFAULT_CONTEXT_TUNING:ContextTuning={briefTokenLimit:3000,reportTokenLimit:300,reworkPhi:0.3,calibrationMinSamples:5};
export function validContextTuning(value:unknown):value is ContextTuning {
 const t=value as ContextTuning|undefined;
 return !!t&&Number.isInteger(t.briefTokenLimit)&&t.briefTokenLimit>=256&&t.briefTokenLimit<=32000&&Number.isInteger(t.reportTokenLimit)&&t.reportTokenLimit>=32&&t.reportTokenLimit<=t.briefTokenLimit&&Number.isFinite(t.reworkPhi)&&t.reworkPhi>=0&&t.reworkPhi<=1&&Number.isInteger(t.calibrationMinSamples)&&t.calibrationMinSamples>=3&&t.calibrationMinSamples<=100;
}

/** Keep missing values distinct from a reported zero, including legacy payloads. */
export function formatUsd(value: number | null | undefined, digits = 2): string {
  if (value == null || !Number.isFinite(value)) return "—";
  return `${value < 0 ? "-" : ""}$${Math.abs(value).toFixed(digits)}`;
}
export function formatCostChoice(reported: number | null | undefined, estimate: number | null | undefined): string {
  if (reported != null) return formatUsd(reported);
  return estimate == null ? "—" : `≈ ${formatUsd(estimate)}`;
}
export function humanEffortSeconds(minutes: string): number | undefined {
  if (!minutes.trim()) return undefined;
  const value=Number(minutes);
  const seconds=Math.round(value*60);
  if (!Number.isFinite(value)||value<0||!Number.isSafeInteger(seconds)) throw new Error("Invalid human effort");
  return seconds;
}

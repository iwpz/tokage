/**
 * Fixed, language-independent tokens shared with the Rust task-report format
 * (see `tokage-orchestration/src/brief.rs`). Free text follows the user's language;
 * these sentinels never do.
 */
export const REPORT_NONE = "None";

/** True when a report section only holds the `none` sentinel. */
export function isNoneToken(text: string): boolean {
  return text.trim().replace(/^[`]+|[`.。]+$/g, "").toLowerCase() === REPORT_NONE.toLowerCase();
}

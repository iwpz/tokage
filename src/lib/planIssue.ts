import i18n from "../i18n";
import type { ValidationIssue } from "./ipc";

/** Localized text for a plan validation issue; falls back to the backend's English message. */
export function localizeIssue(issue: ValidationIssue): string {
  const params: Record<string, string> = {};
  (issue.params ?? []).forEach((v, i) => {
    params[`p${i}`] = v;
  });
  return (i18n.t as (k: string, o?: Record<string, unknown>) => string)(`planIssue.${issue.rule}`, {
    ...params,
    defaultValue: issue.message,
  });
}

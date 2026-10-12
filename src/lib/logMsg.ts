import i18n from "../i18n";

/**
 * Locale-independent log messages.
 *
 * Task logs are persisted as plain strings, but several of them must follow the UI
 * language at render time. `lm()` encodes an i18n key plus params into a marker string;
 * `renderLogMsg()` resolves it with the current language. Strings without the marker
 * (raw CLI output, legacy logs) pass through unchanged.
 */
const MARK = "\u0001i18n:";

export type LmParam = string | number | boolean | null | undefined | LmRef;
export interface LmRef {
  $k: string;
  $p?: Record<string, LmParam>;
}

const isRef = (v: unknown): v is LmRef =>
  typeof v === "object" && v !== null && typeof (v as LmRef).$k === "string";

function norm(v: unknown): LmParam {
  if (v == null || typeof v === "string" || typeof v === "number" || typeof v === "boolean") {
    return v as LmParam;
  }
  if (isRef(v)) return v;
  if (v instanceof Error) return v.message;
  return String(v);
}

function normParams(p?: Record<string, unknown>): Record<string, LmParam> | undefined {
  if (!p) return undefined;
  const out: Record<string, LmParam> = {};
  for (const [k, v] of Object.entries(p)) out[k] = norm(v);
  return out;
}

/** A nested message to be used as a param of another `lm()` call. */
export function lmRef(key: string, params?: Record<string, unknown>): LmRef {
  return { $k: key, $p: normParams(params) };
}

/** Encode a translatable message for storage; resolve it with `renderLogMsg`. */
export function lm(key: string, params?: Record<string, unknown>): string {
  return MARK + JSON.stringify({ k: key, p: normParams(params) });
}

/** Translate right now (for text that leaves the UI, e.g. prompts or thrown errors). */
export function lmText(key: string, params?: Record<string, unknown>): string {
  return resolve({ $k: key, $p: normParams(params) });
}

function resolveParam(v: LmParam): string | number | boolean | null | undefined {
  if (isRef(v)) return resolve(v);
  if (typeof v === "string" && v.startsWith(MARK)) return renderLogMsg(v);
  return v;
}

function resolve(ref: LmRef): string {
  const params: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(ref.$p ?? {})) params[k] = resolveParam(v);
  // Keys are validated by the call sites' tests; runtime keys are plain strings here.
  return (i18n.t as (k: string, o?: Record<string, unknown>) => string)(ref.$k, params);
}

/** Render a stored message (encoded or plain) in the current UI language. */
export function renderLogMsg(s: string): string {
  if (!s.startsWith(MARK)) return s;
  try {
    const { k, p } = JSON.parse(s.slice(MARK.length)) as { k: string; p?: Record<string, LmParam> };
    return resolve({ $k: k, $p: p });
  } catch {
    return s;
  }
}

/** Render a possibly multi-line blob where each line / the whole value may be encoded. */
export function renderLogMsgs(list: readonly string[]): string {
  return list.map(renderLogMsg).join("\n");
}

/**
 * Compares the model Tokage asked a CLI to run with the one the CLI reported running.
 * Reports arrive for free in the turn's own events, so this costs nothing extra.
 */
import type { ProviderId } from "./ipc";
import { idMatches } from "./modelSpec";

export type Reconciled =
  /** Same model (modes and spelling aside). */
  | { kind: "match" }
  /** An alias or router resolved to a concrete model: worth remembering, not a warning. */
  | { kind: "resolved"; actual: string }
  /** The CLI ran something other than what was picked. */
  | { kind: "mismatch"; actual: string };

/** `deepseek/deepseek-chat`, `anthropic/claude-x` → the model part; dates and `[1m]` dropped. */
function core(id: string): string {
  const last = id.trim().toLowerCase().split("/").pop() ?? "";
  return last
    .replace(/\[[^\]]*\]$/, "")
    .replace(/-\d{8}$/, "")
    .replace(/\./g, "-");
}

const ROUTERS = new Set(["auto", "default", "cursor-auto", "kiro-auto"]);

export function reconcileModel(_provider: ProviderId, requested: string, actual: string): Reconciled {
  const want = core(requested);
  const got = core(actual);
  if (!want || !got) return { kind: "match" };
  if (want === got || idMatches(got, want) || idMatches(want, got)) return { kind: "match" };
  // `opus` / `sonnet` / `haiku` / `fable`: a family alias that the CLI resolved to a full name.
  if (/^[a-z]+$/.test(want) && !ROUTERS.has(want) && got.split("-").includes(want)) {
    return { kind: "resolved", actual };
  }
  // The CLI's own router (Cursor/Copilot/Kiro `auto`) picks per request.
  if (ROUTERS.has(want) || want.endsWith("-auto")) return { kind: "resolved", actual };
  return { kind: "mismatch", actual };
}

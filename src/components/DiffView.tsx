import { useMemo } from "react";
import { cn } from "../lib/cn";

interface Line {
  kind: "add" | "del" | "ctx" | "hunk";
  text: string;
  old: number | null;
  new: number | null;
}

/** Parses a unified diff into display lines with old/new line numbers. File headers are dropped. */
export function parseDiff(diff: string): Line[] {
  const out: Line[] = [];
  let o = 0;
  let n = 0;
  for (const raw of diff.split("\n")) {
    if (raw.startsWith("--- ") || raw.startsWith("+++ ") || raw.startsWith("diff --git") || raw.startsWith("index ")) continue;
    const h = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@(.*)$/.exec(raw);
    if (h) {
      o = Number(h[1]);
      n = Number(h[2]);
      out.push({ kind: "hunk", text: raw, old: null, new: null });
    } else if (raw.startsWith("+")) {
      out.push({ kind: "add", text: raw.slice(1), old: null, new: n++ });
    } else if (raw.startsWith("-")) {
      out.push({ kind: "del", text: raw.slice(1), old: o++, new: null });
    } else if (raw.startsWith(" ")) {
      out.push({ kind: "ctx", text: raw.slice(1), old: o++, new: n++ });
    } else if (raw.startsWith("\\")) {
      out.push({ kind: "hunk", text: raw, old: null, new: null });
    }
  }
  return out;
}

export function DiffView({ diff, className }: { diff: string; className?: string }) {
  const lines = useMemo(() => parseDiff(diff), [diff]);
  return (
    <div data-selectable className={cn("overflow-auto font-mono text-[12px] leading-[1.6]", className)}>
      <table className="min-w-full border-collapse">
        <tbody>
          {lines.map((l, i) =>
            l.kind === "hunk" ? (
              <tr key={i} className="bg-hover/60 text-subtle">
                <td colSpan={3} className="px-3 py-0.5">
                  {l.text}
                </td>
              </tr>
            ) : (
              <tr
                key={i}
                className={cn(
                  l.kind === "add" && "bg-[color-mix(in_srgb,var(--success)_14%,transparent)]",
                  l.kind === "del" && "bg-[color-mix(in_srgb,var(--danger)_14%,transparent)]",
                )}
              >
                <td className="w-10 pr-2 pl-3 text-right text-subtle select-none">{l.old ?? ""}</td>
                <td className="w-10 pr-2 text-right text-subtle select-none">{l.new ?? ""}</td>
                <td className="pr-4 whitespace-pre">
                  <span
                    aria-hidden
                    className={cn(
                      "inline-block w-4 select-none",
                      l.kind === "add" && "text-success",
                      l.kind === "del" && "text-danger",
                    )}
                  >
                    {l.kind === "add" ? "+" : l.kind === "del" ? "-" : ""}
                  </span>
                  <span className="sr-only">{l.kind === "add" ? "added: " : l.kind === "del" ? "removed: " : ""}</span>
                  {l.text}
                </td>
              </tr>
            ),
          )}
        </tbody>
      </table>
    </div>
  );
}

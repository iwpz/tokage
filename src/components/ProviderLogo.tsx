import { cn } from "../lib/cn";
import type { ProviderId } from "../lib/ipc";

// Official marks live in src/assets/providers/<id>.svg|png (sources in SOURCES.md).
// Missing files fall back to a monogram, so the whitelist never depends on an asset.
const files = import.meta.glob<string>("../assets/providers/*.{svg,png}", {
  eager: true,
  query: "?url",
  import: "default",
});
const LOGOS: Record<string, string> = {};
for (const [path, url] of Object.entries(files)) {
  const id = path.split("/").pop()!.replace(/\.(svg|png)$/, "");
  LOGOS[id.toUpperCase()] = url;
}

// Single-colour marks (black-only, or switching via prefers-color-scheme, which in an
// <img> follows the OS rather than the in-app theme). Drawn as an alpha mask in the
// current text colour so they stay visible on both themes, as each brand's
// monochrome/inverse variants allow.
const MONOCHROME = new Set<string>(["AIDER", "AUGGIE", "CLINE", "CODEX_CLI", "COPILOT_CLI", "GOOSE"]);
// Black marks baked onto an opaque white square: blend the square away instead.
const ON_WHITE = new Set<string>(["GROK_CLI"]);

export function ProviderLogo({ id, name, className }: { id: ProviderId; name: string; className?: string }) {
  const src = LOGOS[id];
  return (
    <span
      aria-hidden
      className={cn("flex size-9 shrink-0 items-center justify-center overflow-hidden rounded-lg bg-hover", className)}
    >
      {!src ? (
        <span className="text-xs font-semibold text-muted">{name.slice(0, 2)}</span>
      ) : MONOCHROME.has(id) ? (
        <span
          className="size-5 bg-fg"
          style={{
            maskImage: `url("${src}")`,
            WebkitMaskImage: `url("${src}")`,
            maskSize: "contain",
            WebkitMaskSize: "contain",
            maskRepeat: "no-repeat",
            WebkitMaskRepeat: "no-repeat",
            maskPosition: "center",
            WebkitMaskPosition: "center",
          }}
        />
      ) : (
        <img
          src={src}
          alt=""
          draggable={false}
          className={cn(
            "size-5 object-contain",
            ON_WHITE.has(id) && "mix-blend-multiply dark:mix-blend-screen dark:invert",
          )}
        />
      )}
    </span>
  );
}

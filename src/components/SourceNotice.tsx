import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Info } from "lucide-react";
import { openExternalUrl } from "../lib/ipc";
import { sourceFileUrl, useBuildInfo } from "../lib/build";
import { cn } from "../lib/cn";
import { usePopover } from "../lib/usePopover";

/** Where the R11 reading code lives; linked at the revision this build came from. */
const READER_PATH = "src-tauri/crates/tokage-native/src/usage_query/mod.rs";

function ExternalLink({ href, children }: { href: string; children: ReactNode }) {
  return (
    <a
      href={href}
      title={href}
      onClick={(e) => {
        e.preventDefault();
        void openExternalUrl(href);
      }}
      className="text-accent underline decoration-accent/40 underline-offset-2 hover:decoration-accent"
    >
      {children}
    </a>
  );
}

/**
 * The R11 reading boundary and the "check where you got this build" reminder (docs/11 §7.1).
 * `full` is the two-paragraph wording for the first-launch page and About; `compact` is the
 * one-liner kept under account details in the CLI panel; `popover` shows an info icon with a popup bubble.
 */
export function SourceNotice({
  variant,
  className,
}: {
  variant: "full" | "compact" | "popover";
  className?: string;
}) {
  const { t } = useTranslation();
  const info = useBuildInfo();
  const reader = info ? sourceFileUrl(info, READER_PATH) : null;
  const repo = info?.repoUrl ?? null;
  const { open, setOpen, ref } = usePopover<HTMLSpanElement>();

  if (variant === "popover") {
    return (
      <span ref={ref} className={cn("relative inline-flex items-center align-middle", className)}>
        <button
          type="button"
          onClick={() => setOpen((prev) => !prev)}
          aria-label={t("provenance.compact")}
          title={t("provenance.compact")}
          className="inline-flex size-4 items-center justify-center rounded text-subtle transition-colors hover:bg-hover hover:text-fg focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent"
        >
          <Info className="size-3" aria-hidden />
        </button>
        {open && (
          <div
            role="dialog"
            aria-label={t("provenance.compact")}
            className="absolute left-0 top-full z-30 mt-1 w-72 rounded-lg border border-border bg-elevated p-2.5 text-[11px] leading-relaxed text-muted shadow-xl select-text"
          >
            <p>
              {t("provenance.compact")}{" "}
              {reader && <ExternalLink href={reader}>{t("provenance.source")}</ExternalLink>}
            </p>
          </div>
        )}
      </span>
    );
  }

  if (variant === "compact") {
    return (
      <p role="note" className={cn("text-[11px] leading-relaxed text-subtle", className)}>
        {t("provenance.compact")}{" "}
        {reader && <ExternalLink href={reader}>{t("provenance.source")}</ExternalLink>}
      </p>
    );
  }
  return (
    <div role="note" className={cn("space-y-2 text-[13px] leading-relaxed text-muted", className)}>
      <p>
        {t("provenance.reads")}{" "}
        {reader && <ExternalLink href={reader}>{READER_PATH}</ExternalLink>}
      </p>
      <p className="text-warning">
        {t("provenance.origin")}{" "}
        {repo && <ExternalLink href={repo}>{repo.replace(/^https:\/\//, "")}</ExternalLink>}
      </p>
    </div>
  );
}

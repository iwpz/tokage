import { useId, useLayoutEffect, useRef, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { cn } from "../../lib/cn";
import type { ResolvedContextInputs } from "../../lib/contextInputs";
import { usePopover } from "../../lib/usePopover";

function usagePercentage(inputs?: ResolvedContextInputs | null) {
  if (!inputs || inputs.params.effectiveContext <= 0) return null;
  return Math.round(100 * inputs.totalTokens / inputs.params.effectiveContext);
}

function usageColor(percentage: number | null) {
  if (percentage == null) return "text-subtle";
  if (percentage >= 85) return "text-danger";
  if (percentage >= 65) return "text-warning";
  return "text-accent";
}

export function ContextUsagePopover({
  inputs,
  children,
}: {
  inputs?: ResolvedContextInputs | null;
  children: ReactNode;
}) {
  const { t } = useTranslation();
  const { open, setOpen, ref } = usePopover<HTMLDivElement>();
  const panelRef = useRef<HTMLElement>(null);
  const id = useId();
  const percentage = usagePercentage(inputs);
  const angle = Math.min(100, percentage ?? 0) * 3.6;

  useLayoutEffect(() => {
    const trigger = ref.current;
    const panel = panelRef.current;
    if (!open || !trigger || !panel) return;
    const place = () => {
      const edge = 8;
      const { top, right, bottom } = trigger.getBoundingClientRect();
      const minimumRight = panel.offsetWidth + edge;
      const panelRight = Math.max(minimumRight, Math.min(right, window.innerWidth - edge));
      const below = window.innerHeight - bottom;
      const opensBelow = top < 160 && below > top;
      panel.style.right = `${right - panelRight}px`;
      panel.style.top = opensBelow ? "calc(100% + 8px)" : "";
      panel.style.bottom = opensBelow ? "" : "calc(100% + 8px)";
      panel.style.maxHeight = `min(45vh, ${Math.max(0, (opensBelow ? below : top) - 2 * edge)}px)`;
    };
    place();
    window.addEventListener("resize", place);
    return () => window.removeEventListener("resize", place);
  }, [open, ref]);

  return (
    <div ref={ref} className="relative shrink-0">
      <button
        type="button"
        aria-label={t("workspace.context.usage")}
        aria-expanded={open}
        aria-controls={id}
        onClick={() => setOpen(value => !value)}
        className="grid size-8 place-items-center rounded-full hover:bg-hover focus-visible:outline-2 focus-visible:outline-ring"
      >
        <span
          aria-hidden="true"
          className={cn("grid size-4 place-items-center rounded-full bg-line-strong", usageColor(percentage))}
          style={{ backgroundImage: `conic-gradient(currentColor ${angle}deg, transparent 0)` }}
        >
          <span className="size-3 rounded-full bg-elevated" />
        </span>
      </button>
      <section
        ref={panelRef}
        id={id}
        aria-label={t("workspace.context.usage")}
        hidden={!open}
        className="absolute right-0 z-40 w-80 max-w-[calc(100vw-1rem)] overflow-auto overscroll-contain rounded-xl border border-line surface-pop p-3 text-xs"
      >
        {open && <>
          <h2 className="mb-2 font-semibold text-fg">{t("workspace.context.usage")}</h2>
          <div className="space-y-2">{children}</div>
        </>}
      </section>
    </div>
  );
}

export function ContextUsageRow({
  label,
  model,
  inputs,
}: {
  label: string;
  model?: string;
  inputs: ResolvedContextInputs | null;
}) {
  const { t, i18n } = useTranslation();
  const percentage = usagePercentage(inputs);
  const format = (value: number) => Math.round(value).toLocaleString(i18n.resolvedLanguage);
  const reported = inputs?.sources.totalTokens === "CLI_CONTEXT_TOKENS";

  return (
    <div className="space-y-1 rounded-lg bg-hover/60 p-2">
      <div className="flex items-baseline justify-between gap-2">
        <span className="min-w-0 break-words font-medium text-fg">{label}</span>
        {percentage != null && <span className={cn("shrink-0 font-mono", usageColor(percentage))}>{percentage}%</span>}
      </div>
      {model && <p className="break-words text-muted">{model}</p>}
      {inputs && percentage != null ? (
        <>
          <progress
            aria-label={label}
            aria-valuetext={t("workspace.context.usageAria", {
              tokens: format(inputs.totalTokens),
              max: format(inputs.params.effectiveContext),
              percentage,
            })}
            value={Math.min(100, percentage)}
            max={100}
            className={cn(
              "block h-1 w-full appearance-none overflow-hidden rounded-full bg-line-strong",
              "[&::-webkit-progress-bar]:bg-transparent [&::-webkit-progress-value]:bg-current [&::-moz-progress-bar]:bg-current",
              usageColor(percentage),
            )}
          />
          <p className="flex flex-wrap justify-between gap-x-2 text-muted">
            <span className="font-mono">{format(inputs.totalTokens)} / {format(inputs.params.effectiveContext)}</span>
            <span title={t(reported ? "workspace.context.reportedInput" : "workspace.context.estimatedInput")}>
              {t(reported ? "workspace.context.reported" : "workspace.context.estimated")}
            </span>
          </p>
        </>
      ) : <p className="text-muted">{t("workspace.context.unavailableInput")}</p>}
    </div>
  );
}

import { useEffect, useId, useRef, useState, type KeyboardEvent, type Ref } from "react";
import { useTranslation } from "react-i18next";
import { Check, ChevronDown, ChevronRight, Info, RotateCcw, Sparkles } from "lucide-react";
import { cn } from "../lib/cn";
import type { ModelGroup as CatalogModel } from "../lib/ipc";
import { usePopover } from "../lib/usePopover";
import {
  available,
  effectiveOptions,
  hasOptions,
  optionAxes,
  optionSummary,
  poolOf,
  refKey,
  sameRef,
  vendorLabel,
  type ModelGroup,
  type ModelOptions,
  type ModelRef,
  type Pool,
} from "../lib/models";

/**
 * Model tree: provider (vendor · CLI) → [pool] → model, one row per model. Models with
 * several modes get a side submenu for effort, context size, thinking and Fast.
 * Opens upward from the composer. `disabled` is used by auto mode, where Tokage picks per request.
 */
export function ModelPicker({
  groups,
  value,
  recommended,
  overridden,
  onChange,
  onReset,
  options,
  onOptionsChange,
  disabled,
  label,
  placeholder,
  placement = "up",
  compact,
}: {
  groups: ModelGroup[];
  value: ModelRef | null;
  recommended: ModelRef | null;
  overridden: boolean;
  onChange: (ref: ModelRef) => void;
  onReset: () => void;
  options: Record<string, ModelOptions>;
  onOptionsChange: (ref: ModelRef, options: ModelOptions) => void;
  disabled?: boolean;
  label: string;
  /** Trigger text when nothing resolves (default: "no model"). */
  placeholder?: string;
  /** Which way the tree opens; "down" for pickers inside tables and inspectors. */
  placement?: "up" | "down";
  /** Bordered, small trigger for dense UIs. */
  compact?: boolean;
}) {
  const { t } = useTranslation();
  const { open, setOpen, ref: rootRef } = usePopover<HTMLDivElement>();
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  // Model whose submenu is showing.
  const [sub, setSub] = useState<ModelRef | null>(null);
  const treeRef = useRef<HTMLUListElement>(null);
  const subRef = useRef<HTMLDivElement>(null);

  const current = groups.find((g) => g.provider.id === value?.provider);
  const currentModel = current?.models.find((m) => m.key === value?.model);

  const summary = (m: CatalogModel, ref: ModelRef) =>
    optionSummary(
      m,
      options[refKey(ref)],
      (axis, v) => (axis === "effort" ? effortLabel(v) : axis === "tier" ? tierLabel(v) : contextLabel(v)),
      { thinking: t("workspace.composer.picker.thinking"), fast: t("workspace.composer.picker.fast") },
    );
  const effortLabel = (e: string) => t(`settings.cli.efforts.${e}` as "settings.cli.efforts.low", { defaultValue: e });
  const contextLabel = (c: string) => (c === "default" ? t("workspace.composer.picker.contextDefault") : c.toUpperCase());
  const tierLabel = (v: string) => t(`workspace.composer.picker.tiers.${v}` as "workspace.composer.picker.tiers.balance", { defaultValue: v });

  // Opening expands the branch holding the current pick and focuses it.
  useEffect(() => {
    if (!open) {
      setSub(null);
      return;
    }
    setExpanded(new Set(value ? [value.provider] : groups.map((g) => g.provider.id)));
    requestAnimationFrame(() => {
      const el =
        treeRef.current?.querySelector<HTMLElement>('[aria-selected="true"]') ??
        treeRef.current?.querySelector<HTMLElement>('[role="treeitem"]');
      el?.focus();
    });
  }, [open]); // eslint-disable-line react-hooks/exhaustive-deps

  const toggle = (id: string, to?: boolean) =>
    setExpanded((s) => {
      const n = new Set(s);
      if (to ?? !n.has(id)) n.add(id);
      else n.delete(id);
      return n;
    });

  const openSub = (ref: ModelRef, focus = false) => {
    setSub(ref);
    if (focus) requestAnimationFrame(() => subRef.current?.querySelector<HTMLElement>("button:not(:disabled)")?.focus());
  };

  const onTreeKey = (e: KeyboardEvent) => {
    const items = Array.from(treeRef.current?.querySelectorAll<HTMLElement>('[role="treeitem"]') ?? []);
    const i = items.indexOf(document.activeElement as HTMLElement);
    if (i < 0) return;
    const el = items[i];
    const group = el.dataset.group;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      setSub(null);
      items[(i + (e.key === "ArrowDown" ? 1 : items.length - 1)) % items.length]?.focus();
    } else if (e.key === "ArrowRight" && group) {
      e.preventDefault();
      if (!expanded.has(group)) toggle(group, true);
      else items[i + 1]?.focus();
    } else if (e.key === "ArrowRight" && el.dataset.options) {
      e.preventDefault();
      openSub({ provider: el.dataset.parent as ModelRef["provider"], model: el.dataset.model! }, true);
    } else if (e.key === "ArrowLeft") {
      e.preventDefault();
      if (group) toggle(group, false);
      else items.find((x) => x.dataset.group === el.dataset.parent)?.focus();
    }
  };

  const triggerText = disabled
    ? t("workspace.composer.picker.auto")
    : currentModel
      ? [vendorLabel(current!.provider.id, current!.provider.name), currentModel.name, ...summary(currentModel, value!)].join(" · ")
      : (placeholder ?? t("workspace.composer.picker.none"));

  const subModel = sub && groups.find((g) => g.provider.id === sub.provider)?.models.find((m) => m.key === sub.model);

  const modelRow = (g: ModelGroup, m: CatalogModel) => {
    const ref = { provider: g.provider.id, model: m.key };
    const selected = sameRef(ref, value);
    const withOptions = hasOptions(m);
    const extra = summary(m, ref);
    return (
      <li key={m.key} role="none" onMouseEnter={() => (withOptions ? setSub(ref) : setSub(null))}>
        <div
          className={cn(
            "flex h-8 items-center rounded-md hover:bg-hover has-focus-visible:bg-hover",
            sameRef(sub, ref) && "bg-hover",
          )}
        >
          <button
            type="button"
            role="treeitem"
            aria-selected={selected}
            aria-haspopup={withOptions ? "menu" : undefined}
            aria-expanded={withOptions ? sameRef(sub, ref) : undefined}
            data-parent={g.provider.id}
            data-model={m.key}
            data-options={withOptions ? "1" : undefined}
            tabIndex={-1}
            onClick={() => {
              onChange(ref);
              setOpen(false);
            }}
            className="flex h-full min-w-0 flex-1 items-center gap-2 pr-1 pl-8 text-left text-fg focus-visible:outline-none"
          >
            <span className="min-w-0 truncate">{m.name}</span>
            {extra.length > 0 && <span className="shrink-0 truncate text-xs text-subtle">{extra.join(" · ")}</span>}
            <span className="flex-1" />
            {sameRef(ref, recommended) && (
              <span className="shrink-0 rounded bg-accent-soft px-1.5 text-[10px] text-accent">
                {t("workspace.composer.picker.recommended")}
              </span>
            )}
            <Check aria-hidden className={cn("size-3.5 shrink-0 text-muted", !selected && "invisible")} />
          </button>
          {m.tags.length > 0 && <ModelInfo model={m} vendor={vendorLabel(g.provider.id, g.provider.name)} />}
          {withOptions ? (
            <button
              type="button"
              tabIndex={-1}
              aria-label={`${t("workspace.composer.picker.options")}: ${m.name}`}
              onClick={() => openSub(ref, true)}
              className="inline-flex h-full w-6 shrink-0 items-center justify-center text-muted hover:text-fg"
            >
              <ChevronRight aria-hidden className="size-3.5" />
            </button>
          ) : (
            <span className="w-6 shrink-0" />
          )}
        </div>
      </li>
    );
  };

  return (
    <div ref={rootRef} className="relative">
      <button
        type="button"
        aria-haspopup="tree"
        aria-expanded={open}
        aria-label={`${label}: ${triggerText}`}
        disabled={disabled || groups.length === 0}
        title={disabled ? t("workspace.composer.picker.autoHint") : undefined}
        onClick={() => setOpen((v) => !v)}
        className={cn(
          "inline-flex items-center gap-1.5 text-muted hover:bg-hover hover:text-fg disabled:cursor-not-allowed disabled:opacity-60 disabled:hover:bg-transparent disabled:hover:text-muted",
          compact
            ? "h-6 max-w-[190px] rounded border border-border bg-base px-2 text-[11px] text-fg hover:border-accent"
            : "h-7 max-w-72 rounded-md px-2 text-xs",
        )}
      >
        {disabled ? <Sparkles aria-hidden className="size-3.5 shrink-0" /> : null}
        <span className="truncate">{triggerText}</span>
        {!disabled && overridden && (
          <span className="shrink-0 rounded bg-hover px-1 text-[10px] text-subtle">{t("workspace.composer.picker.manual")}</span>
        )}
        <ChevronDown aria-hidden className="size-3 shrink-0" />
      </button>

      {open && (
        <div
          className={cn(
            "absolute right-0 z-30 w-80 rounded-lg border border-border bg-elevated p-1 text-[13px] shadow-lg",
            placement === "down" ? "top-full mt-2" : "bottom-full mb-2",
          )}
        >
          <div className="px-2 pt-1 pb-1.5 text-xs text-subtle">{label}</div>
          <ul ref={treeRef} role="tree" aria-label={label} onKeyDown={onTreeKey} className="max-h-80 overflow-y-auto">
            {groups.map((g) => {
              const isOpen = expanded.has(g.provider.id);
              const pools = poolSections(g);
              return (
                <li key={g.provider.id} role="none" onMouseEnter={() => setSub((s) => (s?.provider === g.provider.id ? s : null))}>
                  <button
                    type="button"
                    role="treeitem"
                    aria-expanded={isOpen}
                    data-group={g.provider.id}
                    tabIndex={-1}
                    onClick={() => toggle(g.provider.id)}
                    className="flex h-8 w-full items-center gap-1.5 rounded-md px-2 text-left text-fg hover:bg-hover focus-visible:bg-hover focus-visible:outline-none"
                  >
                    <ChevronRight aria-hidden className={cn("size-3.5 text-muted transition-transform", isOpen && "rotate-90")} />
                    <span className="font-medium">{vendorLabel(g.provider.id, g.provider.name)}</span>
                    {vendorLabel(g.provider.id, g.provider.name) !== g.provider.name && (
                      <span className="truncate text-xs text-subtle">{g.provider.name}</span>
                    )}
                  </button>
                  {isOpen &&
                    (pools ? (
                      pools.map(([pool, models]) => (
                        <ul key={pool} role="group" aria-label={t(`workspace.composer.picker.pools.${pool}`)}>
                          <li role="presentation" className="px-8 pt-1.5 pb-0.5 text-[11px] text-subtle">
                            {t(`workspace.composer.picker.pools.${pool}`)}
                          </li>
                          {models.map((m) => modelRow(g, m))}
                        </ul>
                      ))
                    ) : (
                      <ul role="group">{g.models.map((m) => modelRow(g, m))}</ul>
                    ))}
                </li>
              );
            })}
          </ul>
          {overridden && (
            <>
              <div className="my-1 border-t border-border" />
              <button
                type="button"
                onClick={() => {
                  onReset();
                  setOpen(false);
                }}
                className="flex h-8 w-full items-center gap-2 rounded-md px-2 text-left text-muted hover:bg-hover hover:text-fg"
              >
                <RotateCcw aria-hidden className="size-3.5" />
                {t("workspace.composer.picker.reset")}
              </button>
            </>
          )}

          {sub && subModel && (
            <OptionsMenu
              ref={subRef}
              model={subModel}
              placement={placement}
              options={options[refKey(sub)] ?? {}}
              effortLabel={effortLabel}
              contextLabel={contextLabel}
              onChange={(o) => {
                onOptionsChange(sub, o);
                // Adjusting a model's options picks it.
                if (!sameRef(sub, value)) onChange(sub);
              }}
              onBack={() => {
                const back = treeRef.current?.querySelector<HTMLElement>(
                  `[data-parent="${sub.provider}"][data-model="${CSS.escape(sub.model)}"]`,
                );
                setSub(null);
                back?.focus();
              }}
            />
          )}
        </div>
      )}
    </div>
  );
}

/** Cursor's models split into its own pool and the third-party pool; null for other CLIs. */
function poolSections(g: ModelGroup): [Pool, CatalogModel[]][] | null {
  if (!g.models.some((m) => poolOf(g.provider.id, m.key))) return null;
  const order: Pool[] = ["router", "own", "third"];
  return order
    .map((p) => [p, g.models.filter((m) => poolOf(g.provider.id, m.key) === p)] as [Pool, CatalogModel[]])
    .filter(([, list]) => list.length > 0);
}

/**
 * Info icon for facts kept out of the model name (context size, NO ZDR). The tip is
 * fixed-positioned so the tree's scroll container doesn't clip it; it shows on hover and focus.
 */
function ModelInfo({ model, vendor }: { model: CatalogModel; vendor: string }) {
  const { t } = useTranslation();
  const id = useId();
  const btn = useRef<HTMLButtonElement>(null);
  const [at, setAt] = useState<{ top: number; right: number } | null>(null);
  const show = () => {
    const r = btn.current?.getBoundingClientRect();
    if (r) setAt({ top: r.bottom + 4, right: window.innerWidth - r.right });
  };
  const hide = () => setAt(null);
  const tagLabel = (tag: string) =>
    tag === "no-zdr" ? "NO ZDR" : /^\d+[km]$/.test(tag) ? tag.toUpperCase() : tag;
  const tagText = (tag: string) =>
    tag === "no-zdr"
      ? t("workspace.composer.picker.tags.noZdr", { vendor })
      : /^\d+[km]$/.test(tag)
        ? t("workspace.composer.picker.tags.context", { size: tag.toUpperCase() })
        : null;
  return (
    <>
      <button
        ref={btn}
        type="button"
        tabIndex={-1}
        aria-label={`${t("workspace.composer.picker.tags.label")}: ${model.name}`}
        aria-describedby={at ? id : undefined}
        onMouseEnter={show}
        onMouseLeave={hide}
        onFocus={show}
        onBlur={hide}
        onKeyDown={(e) => e.key === "Escape" && at && (e.stopPropagation(), hide())}
        className={cn(
          "inline-flex h-full w-6 shrink-0 items-center justify-center text-muted hover:text-fg focus-visible:text-fg focus-visible:outline-none",
          model.tags.includes("no-zdr") && "text-warning",
        )}
      >
        <Info aria-hidden className="size-3.5" />
      </button>
      {at && (
        <div
          id={id}
          role="tooltip"
          style={{ top: at.top, right: at.right }}
          className="pointer-events-none fixed z-50 w-64 rounded-lg border border-border bg-elevated p-2.5 text-xs text-fg shadow-lg"
        >
          <ul className="flex flex-col gap-2">
            {model.tags.map((tag) => (
              <li key={tag} className="flex flex-col gap-1">
                <span className="w-fit rounded bg-hover px-1.5 font-mono text-[10px] text-muted">[{tagLabel(tag)}]</span>
                {tagText(tag) && <span className="leading-snug text-muted">{tagText(tag)}</span>}
              </li>
            ))}
          </ul>
        </div>
      )}
    </>
  );
}

/** Side panel to the left of the picker with the model's selectable modes. */
function OptionsMenu({
  ref,
  model,
  options,
  effortLabel,
  contextLabel,
  onChange,
  onBack,
  placement,
}: {
  ref: Ref<HTMLDivElement>;
  placement: "up" | "down";
  model: CatalogModel;
  options: ModelOptions;
  effortLabel: (e: string) => string;
  contextLabel: (c: string) => string;
  onChange: (o: ModelOptions) => void;
  onBack: () => void;
}) {
  const { t } = useTranslation();
  const axes = optionAxes(model);
  const eff = effectiveOptions(model, options);
  // Not every combination exists (Cursor has Fast for some efforts only): drop Fast
  // rather than show a toggle the CLI can't honour.
  const set = (patch: ModelOptions) => {
    const next = { ...options, ...patch };
    onChange(next.fast && !available(model, next, {}) ? { ...next, fast: false } : next);
  };

  const onKey = (e: KeyboardEvent<HTMLDivElement>) => {
    const items = Array.from(e.currentTarget.querySelectorAll<HTMLElement>("button:not(:disabled)"));
    const i = items.indexOf(document.activeElement as HTMLElement);
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      items[(i + (e.key === "ArrowDown" ? 1 : items.length - 1)) % items.length]?.focus();
    } else if (e.key === "ArrowLeft") {
      e.preventDefault();
      onBack();
    }
  };

  const radio = (checked: boolean, text: string, onClick: () => void, enabled = true) => (
    <button
      key={text}
      type="button"
      role="menuitemradio"
      aria-checked={checked}
      disabled={!enabled}
      onClick={onClick}
      className="flex h-7 w-full items-center gap-2 rounded-md px-2 text-left text-fg hover:bg-hover focus-visible:bg-hover focus-visible:outline-none disabled:text-subtle disabled:hover:bg-transparent"
    >
      <Check aria-hidden className={cn("size-3.5 text-muted", !checked && "invisible")} />
      {text}
    </button>
  );

  const toggle = (checked: boolean, text: string, onClick: () => void, enabled = true) => (
    <button
      type="button"
      role="menuitemcheckbox"
      aria-checked={checked}
      disabled={!enabled}
      onClick={onClick}
      className="flex h-7 w-full items-center justify-between gap-2 rounded-md px-2 text-left text-fg hover:bg-hover focus-visible:bg-hover focus-visible:outline-none disabled:text-subtle disabled:hover:bg-transparent"
    >
      {text}
      <span
        aria-hidden
        className={cn("relative inline-flex h-4 w-7 shrink-0 items-center rounded-full transition-colors", checked ? "bg-accent" : "bg-active")}
      >
        <span className={cn("inline-block size-3 rounded-full bg-white shadow transition-transform", checked ? "translate-x-3.5" : "translate-x-0.5")} />
      </span>
    </button>
  );

  const heading = (text: string) => <div className="px-2 pt-1.5 pb-0.5 text-[11px] text-subtle">{text}</div>;

  return (
    <div
      ref={ref}
      role="menu"
      aria-label={`${t("workspace.composer.picker.options")}: ${model.name}`}
      onKeyDown={onKey}
      className={cn(
        "absolute right-full mr-1 w-52 rounded-lg border border-border bg-elevated p-1 text-[13px] shadow-lg",
        placement === "down" ? "top-0" : "bottom-0",
      )}
    >
      <div className="truncate px-2 pt-1 pb-1 text-xs font-medium text-fg">{model.name}</div>
      {axes.tiers.length > 1 && (
        <div role="group" aria-label={t("workspace.composer.picker.tier")}>
          {heading(t("workspace.composer.picker.tier"))}
          {axes.tiers.map((v) =>
            radio(eff.tier === v, t(`workspace.composer.picker.tiers.${v}` as "workspace.composer.picker.tiers.balance", { defaultValue: v }), () => set({ tier: v })),
          )}
        </div>
      )}
      {axes.efforts.length > 1 && (
        <div role="group" aria-label={t("workspace.composer.picker.effort")}>
          {heading(t("workspace.composer.picker.effort"))}
          {axes.efforts.map((e) =>
            radio(eff.effort === e, effortLabel(e), () => set({ effort: e })),
          )}
        </div>
      )}
      {axes.contexts.length > 1 && (
        <div role="group" aria-label={t("workspace.composer.picker.context")}>
          {heading(t("workspace.composer.picker.context"))}
          {axes.contexts.map((c) => radio(eff.context === c, contextLabel(c), () => set({ context: c })))}
        </div>
      )}
      {(axes.thinking || axes.fast) && <div className="my-1 border-t border-border" />}
      {axes.thinking &&
        toggle(eff.thinking, t("workspace.composer.picker.thinking"), () => set({ thinking: !eff.thinking }), available(model, options, { thinking: !eff.thinking }))}
      {axes.fast &&
        toggle(eff.fast, t("workspace.composer.picker.fast"), () => set({ fast: !eff.fast }), eff.fast || available(model, options, { fast: true }))}
    </div>
  );
}

import { useId, type ButtonHTMLAttributes, type ReactNode } from "react";
import { cn } from "../lib/cn";
import { useTranslation } from "react-i18next";

export function Button({
  variant = "secondary",
  size = "md",
  className,
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: "primary" | "secondary" | "ghost";
  size?: "sm" | "md";
}) {
  return (
    <button
      type="button"
      className={cn(
        "inline-flex items-center justify-center gap-1.5 rounded-md font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-50",
        size === "sm" ? "h-7 px-2.5 text-xs" : "h-8 px-3 text-[13px]",
        variant === "primary" && "bg-accent text-accent-fg hover:opacity-90",
        variant === "secondary" && "border border-border bg-elevated text-fg hover:bg-hover",
        variant === "ghost" && "text-muted hover:bg-hover hover:text-fg",
        className,
      )}
      {...props}
    />
  );
}

export function IconButton({
  label,
  className,
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { label: string }) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      className={cn(
        "inline-flex size-7 items-center justify-center rounded-md text-muted transition-colors hover:bg-hover hover:text-fg",
        className,
      )}
      {...props}
    />
  );
}

export function Switch({
  checked,
  onChange,
  label,
  disabled = false,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label: string;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => {
        if (!disabled) onChange(!checked);
      }}
      className={cn(
        "relative inline-flex h-5 w-9 shrink-0 items-center rounded-full transition-colors",
        checked ? "bg-accent" : "bg-active",
        disabled && "opacity-50 cursor-not-allowed",
      )}
    >
      <span
        className={cn(
          "inline-block size-4 rounded-full bg-white shadow transition-transform",
          checked ? "translate-x-[18px]" : "translate-x-0.5",
        )}
      />
    </button>
  );
}

export function Segmented<T extends string>({
  value,
  options,
  onChange,
  label,
  disabled = false,
}: {
  value: T;
  options: { value: T; label: string; icon?: ReactNode }[];
  onChange: (v: T) => void;
  label: string;
  disabled?: boolean;
}) {
  return (
    <div
      role="radiogroup"
      aria-label={label}
      aria-disabled={disabled || undefined}
      className={cn("inline-flex rounded-lg bg-hover p-0.5", disabled && "opacity-50")}
    >
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          role="radio"
          aria-checked={value === o.value}
          disabled={disabled}
          onClick={() => onChange(o.value)}
          className={cn(
            "inline-flex h-7 items-center gap-1.5 rounded-md px-3 text-[13px] transition-colors disabled:cursor-not-allowed",
            value === o.value ? "bg-elevated text-fg shadow-sm" : "text-muted enabled:hover:text-fg",
          )}
        >
          {o.icon}
          {o.label}
        </button>
      ))}
    </div>
  );
}

export function Select<T extends string>({
  value,
  options,
  onChange,
  label,
  className,
}: {
  value: T;
  options: { value: T; label: string; disabled?: boolean }[];
  onChange: (v: T) => void;
  label: string;
  className?: string;
}) {
  return (
    <select
      aria-label={label}
      value={value}
      onChange={(e) => onChange(e.target.value as T)}
      className={cn(
        "h-8 min-w-40 rounded-md border border-border bg-elevated px-2 text-[13px] text-fg",
        className,
      )}
    >
      {options.map((o) => (
        <option key={o.value} value={o.value} disabled={o.disabled}>
          {o.label}
        </option>
      ))}
    </select>
  );
}

export function NumberInput({
  value,
  onChange,
  label,
  min,
  max,
  step = 1,
  suffix,
}: {
  value: number;
  onChange: (v: number) => void;
  label: string;
  min?: number;
  max?: number;
  step?: number;
  suffix?: string;
}) {
  const id = useId();
  return (
    <div className="flex items-center gap-1.5">
      <input
        id={id}
        type="number"
        aria-label={label}
        value={value}
        min={min}
        max={max}
        step={step}
        onChange={(e) => {
          const n = Number(e.target.value);
          if (Number.isFinite(n)) onChange(Math.min(max ?? n, Math.max(min ?? n, n)));
        }}
        className="h-8 w-24 rounded-md border border-border bg-elevated px-2 text-right text-[13px] text-fg tabular-nums"
      />
      {suffix && <span className="text-[13px] text-muted">{suffix}</span>}
    </div>
  );
}

/** One labelled row inside a settings card. */
export function SettingRow({
  title,
  description,
  children,
}: {
  title: string;
  description?: string;
  children: ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-6 px-4 py-3.5">
      <div className="min-w-0">
        <div className="text-[13px] font-medium text-fg">{title}</div>
        {description && <div className="mt-0.5 text-xs leading-relaxed text-muted">{description}</div>}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}

export function Card({
  children,
  className,
  divided,
}: {
  children: ReactNode;
  className?: string;
  divided?: boolean;
}) {
  const hasPadding = className && /\bp(?:[xytb]|)-\d+/.test(className);
  const shouldDivide = divided ?? !hasPadding;
  return (
    <div
      className={cn(
        "rounded-xl border border-border bg-elevated",
        shouldDivide && "divide-y divide-border",
        className,
      )}
    >
      {children}
    </div>
  );
}

export function Badge({
  variant = "default",
  size = "sm",
  children,
  className,
}: {
  variant?: "default" | "warning" | "danger" | "success" | "accent" | "muted";
  size?: "xs" | "sm";
  children: ReactNode;
  className?: string;
}) {
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1 rounded-md font-medium tracking-tight",
        size === "xs" ? "px-1.5 py-0.5 text-[11px]" : "px-2 py-0.5 text-xs",
        variant === "default" && "bg-hover text-fg border border-border",
        variant === "accent" && "bg-accent/15 text-accent border border-accent/20",
        variant === "warning" && "bg-warning/15 text-warning border border-warning/20",
        variant === "danger" && "bg-danger/15 text-danger border border-danger/20",
        variant === "success" && "bg-success/15 text-success border border-success/20",
        variant === "muted" && "bg-elevated text-muted border border-border",
        className,
      )}
    >
      {children}
    </span>
  );
}

export function Tabs<T extends string>({
  value,
  options,
  onChange,
  className,
}: {
  value: T;
  options: { value: T; label: string; count?: number; icon?: ReactNode }[];
  onChange: (v: T) => void;
  className?: string;
}) {
  return (
    <div role="tablist" className={cn("flex items-center gap-1 border-b border-border px-3 py-1.5", className)}>
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          role="tab"
          aria-selected={value === o.value}
          onClick={() => onChange(o.value)}
          className={cn(
            "inline-flex h-7 items-center gap-1.5 rounded-md px-2.5 text-[13px] font-medium transition-colors",
            value === o.value ? "bg-active text-fg" : "text-muted hover:bg-hover hover:text-fg",
          )}
        >
          {o.icon}
          {o.label}
          {o.count !== undefined && (
            <span
              className={cn(
                "rounded px-1.5 py-0.2 text-[11px] tabular-nums",
                value === o.value ? "bg-hover text-fg" : "bg-elevated text-subtle",
              )}
            >
              {o.count}
            </span>
          )}
        </button>
      ))}
    </div>
  );
}

export function Modal({
  open,
  onClose,
  title,
  children,
  className,
}: {
  open: boolean;
  onClose: () => void;
  title?: string;
  children: ReactNode;
  className?: string;
}) {
  const { t } = useTranslation();
  if (!open) return null;
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4 backdrop-blur-xs">
      <div
        role="dialog"
        aria-modal="true"
        aria-label={title}
        className={cn(
          "flex max-h-[90vh] w-full max-w-2xl flex-col rounded-2xl border border-border bg-elevated shadow-2xl overflow-hidden",
          className,
        )}
      >
        {title && (
          <div className="flex items-center justify-between border-b border-border px-5 py-3">
            <h3 className="text-sm font-semibold text-fg">{title}</h3>
            <button
              type="button"
              onClick={onClose}
              aria-label={t("misc.close")}
              className="rounded-md p-1 text-muted hover:bg-hover hover:text-fg"
            >
              ✕
            </button>
          </div>
        )}
        <div className="flex-1 overflow-y-auto p-5">{children}</div>
      </div>
    </div>
  );
}

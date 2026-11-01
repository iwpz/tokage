import { useId, type ButtonHTMLAttributes, type InputHTMLAttributes, type ReactNode } from "react";
import { Check, X } from "lucide-react";
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
        "inline-flex items-center justify-center gap-1.5 rounded-lg font-medium transition-[background-color,color,box-shadow,transform,filter] duration-150 enabled:active:scale-[0.98] disabled:cursor-not-allowed disabled:opacity-50",
        size === "sm" ? "h-7 px-2.5 text-xs" : "h-8 px-3 text-[13px]",
        variant === "primary" &&
          "bg-(image:--accent-grad) text-accent-fg shadow-[inset_0_1px_0_rgba(255,255,255,0.25),0_0_0_1px_rgba(0,0,0,0.12),0_2px_6px_-1px_rgba(0,0,0,0.25)] enabled:hover:brightness-105",
        variant === "secondary" && "surface-card text-fg enabled:hover:bg-hover",
        variant === "ghost" && "text-muted enabled:hover:bg-hover enabled:hover:text-fg",
        className,
      )}
      {...props}
    />
  );
}

/** Native checkbox semantics with the application's theme, independent of OS accent settings. */
export function Checkbox({ className, ...props }: Omit<InputHTMLAttributes<HTMLInputElement>, "type">) {
  return <span className="relative inline-flex size-4 shrink-0 align-middle">
    <input {...props} type="checkbox" className={cn("peer size-4 cursor-pointer appearance-none rounded border border-border bg-base transition-colors checked:border-accent checked:bg-accent focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent disabled:cursor-not-allowed disabled:opacity-50", className)} />
    <Check aria-hidden="true" className="pointer-events-none absolute inset-0 size-4 p-0.5 text-accent-fg opacity-0 peer-checked:opacity-100" strokeWidth={3} />
  </span>;
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
        "inline-flex size-7 items-center justify-center rounded-md text-muted transition-colors duration-150 hover:bg-hover hover:text-fg active:scale-95",
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
        "relative inline-flex h-5 w-9 shrink-0 items-center rounded-full shadow-[inset_0_0_0_1px_var(--line)] transition-colors duration-200",
        checked ? "bg-accent" : "bg-active",
        disabled && "opacity-50 cursor-not-allowed",
      )}
    >
      <span
        className={cn(
          "inline-block size-4 rounded-full bg-white shadow-[0_1px_2px_rgba(0,0,0,0.3)] transition-transform duration-200 ease-(--ease-out-soft)",
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
      className={cn("inline-flex rounded-[9px] bg-hover p-0.5 shadow-[inset_0_0_0_1px_var(--line)]", disabled && "opacity-50")}
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
            value === o.value ? "surface-card text-fg" : "text-muted enabled:hover:text-fg",
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
        "h-8 min-w-40 rounded-lg border-0 surface-card px-2 text-[13px] text-fg transition-colors hover:bg-hover",
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
        className="h-8 w-24 rounded-lg border-0 bg-base px-2 text-right text-[13px] text-fg tabular-nums shadow-[inset_0_0_0_1px_var(--line-strong)] focus:shadow-[inset_0_0_0_1px_var(--accent)] focus:outline-none"
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
        "rounded-xl surface-card",
        shouldDivide && "divide-y divide-line",
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
        "inline-flex items-center gap-1 rounded-[5px] font-medium",
        size === "xs" ? "px-1.5 py-px text-[11px]" : "px-1.5 py-0.5 text-xs",
        variant === "default" && "bg-active/70 text-fg",
        variant === "accent" && "bg-accent/12 text-accent",
        variant === "warning" && "bg-warning/12 text-warning",
        variant === "danger" && "bg-danger/12 text-danger",
        variant === "success" && "bg-success/12 text-success",
        variant === "muted" && "bg-hover text-muted",
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
    <div role="tablist" className={cn("flex items-center gap-1 border-b border-line px-3", className)}>
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          role="tab"
          aria-selected={value === o.value}
          onClick={() => onChange(o.value)}
          className={cn(
            "relative inline-flex h-9 items-center gap-1.5 px-2.5 text-[13px] font-medium transition-colors duration-150",
            "after:absolute after:inset-x-2 after:-bottom-px after:h-0.5 after:rounded-full after:transition-colors after:duration-200",
            value === o.value ? "text-fg after:bg-fg" : "text-muted hover:text-fg after:bg-transparent",
          )}
        >
          {o.icon}
          {o.label}
          {o.count !== undefined && (
            <span
              className={cn(
                "text-xs font-normal tabular-nums",
                value === o.value ? "text-muted" : "text-subtle",
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
    <div className="animate-fade-in fixed inset-0 z-50 flex items-center justify-center bg-black/45 p-4 backdrop-blur-sm">
      <div
        role="dialog"
        aria-modal="true"
        aria-label={title}
        className={cn(
          "animate-pop-in flex max-h-[90vh] w-full max-w-2xl flex-col overflow-hidden rounded-2xl surface-glass",
          className,
        )}
      >
        {title && (
          <div className="flex items-center justify-between border-b border-line px-5 py-3">
            <h3 className="text-sm font-semibold tracking-[-0.01em] text-fg">{title}</h3>
            <button
              type="button"
              onClick={onClose}
              aria-label={t("misc.close")}
              className="inline-flex size-7 items-center justify-center rounded-md text-muted transition-colors hover:bg-hover hover:text-fg"
            >
              <X className="size-4" aria-hidden="true" />
            </button>
          </div>
        )}
        <div className="flex-1 overflow-y-auto p-5">{children}</div>
      </div>
    </div>
  );
}

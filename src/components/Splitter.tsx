import { useRef, type KeyboardEvent, type PointerEvent } from "react";
import { cn } from "../lib/cn";

/**
 * Vertical drag handle between two panes. Dragging, arrow keys (Shift for bigger steps),
 * Home/End and double-click (reset) all adjust the width through `onChange`.
 * `side` says which pane it sizes: "left" grows when dragged right, "right" the opposite.
 */
export function Splitter({
  label,
  value,
  min,
  max,
  side,
  onChange,
  onReset,
  className,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  side: "left" | "right";
  onChange: (width: number) => void;
  onReset?: () => void;
  className?: string;
}) {
  const drag = useRef<{ x: number; width: number } | null>(null);
  const clamp = (w: number) => Math.min(Math.max(w, min), Math.max(min, max));

  const onPointerDown = (e: PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    e.preventDefault();
    e.currentTarget.setPointerCapture(e.pointerId);
    drag.current = { x: e.clientX, width: value };
    // Disables width transitions and text selection app-wide while dragging.
    document.documentElement.classList.add("is-resizing");
  };
  const onPointerMove = (e: PointerEvent<HTMLDivElement>) => {
    if (!drag.current) return;
    const dx = e.clientX - drag.current.x;
    onChange(clamp(drag.current.width + (side === "left" ? dx : -dx)));
  };
  const end = () => {
    drag.current = null;
    document.documentElement.classList.remove("is-resizing");
  };

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const step = e.shiftKey ? 48 : 16;
    const grow = side === "left" ? "ArrowRight" : "ArrowLeft";
    const shrink = side === "left" ? "ArrowLeft" : "ArrowRight";
    let next: number | null = null;
    if (e.key === grow) next = value + step;
    else if (e.key === shrink) next = value - step;
    else if (e.key === "Home") next = min;
    else if (e.key === "End") next = max;
    else if (e.key === "Enter" && onReset) {
      e.preventDefault();
      onReset();
      return;
    }
    if (next === null) return;
    e.preventDefault();
    onChange(clamp(next));
  };

  return (
    <div
      role="separator"
      aria-orientation="vertical"
      aria-label={label}
      aria-valuenow={Math.round(value)}
      aria-valuemin={min}
      aria-valuemax={Math.max(min, max)}
      tabIndex={0}
      title={label}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={end}
      onPointerCancel={end}
      onLostPointerCapture={end}
      onDoubleClick={onReset}
      onKeyDown={onKeyDown}
      className={cn(
        // A 9px hit area centred on the pane border; the accent line shows on hover/focus.
        "group absolute inset-y-0 z-20 w-[9px] cursor-col-resize touch-none outline-none",
        side === "left" ? "-right-[5px]" : "-left-[5px]",
        className,
      )}
    >
      <div className="mx-auto h-full w-0.5 bg-transparent transition-colors group-hover:bg-accent/60 group-focus-visible:bg-ring group-active:bg-accent" />
    </div>
  );
}

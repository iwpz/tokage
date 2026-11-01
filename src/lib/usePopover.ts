import { useEffect, useRef, useState } from "react";

/** Open state for a click-triggered popover; closes on outside click and Esc. */
export function usePopover<T extends HTMLElement = HTMLSpanElement>() {
  const [open, setOpen] = useState(false);
  const ref = useRef<T>(null);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey, true);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey, true);
    };
  }, [open]);

  return { open, setOpen, ref };
}

/**
 * Calculates { top, left } for a `position: fixed` popup relative to its actual containing block.
 * When an ancestor has `backdrop-filter`, `transform`, or `filter`, the browser treats that ancestor
 * as the containing block for `fixed` elements instead of the viewport.
 */
export function getFixedPosition(el: HTMLElement, offset = 4): { top: number; left: number } {
  const r = el.getBoundingClientRect();
  let container = el.parentElement;
  let offsetTop = 0;
  let offsetLeft = 0;
  while (container && container !== document.body && container !== document.documentElement) {
    const style = window.getComputedStyle(container);
    const webkitBf = (style as unknown as { webkitBackdropFilter?: string }).webkitBackdropFilter;
    if (
      (style.backdropFilter && style.backdropFilter !== "none") ||
      (webkitBf && webkitBf !== "none") ||
      (style.transform && style.transform !== "none") ||
      (style.filter && style.filter !== "none")
    ) {
      const cr = container.getBoundingClientRect();
      offsetTop = cr.top;
      offsetLeft = cr.left;
      break;
    }
    container = container.parentElement;
  }
  return {
    top: Math.round(r.bottom - offsetTop + offset),
    left: Math.round(r.left - offsetLeft),
  };
}

/**
 * Calculates { top, left } for a `position: fixed` popup positioned at specific mouse coordinates (clientX, clientY),
 * taking into account any containing block established by backdrop-filter or transform.
 */
export function getFixedCoords(el: HTMLElement, clientX: number, clientY: number): { top: number; left: number } {
  let container = el.parentElement;
  let offsetTop = 0;
  let offsetLeft = 0;
  while (container && container !== document.body && container !== document.documentElement) {
    const style = window.getComputedStyle(container);
    const webkitBf = (style as unknown as { webkitBackdropFilter?: string }).webkitBackdropFilter;
    if (
      (style.backdropFilter && style.backdropFilter !== "none") ||
      (webkitBf && webkitBf !== "none") ||
      (style.transform && style.transform !== "none") ||
      (style.filter && style.filter !== "none")
    ) {
      const cr = container.getBoundingClientRect();
      offsetTop = cr.top;
      offsetLeft = cr.left;
      break;
    }
    container = container.parentElement;
  }
  return {
    top: Math.round(clientY - offsetTop + 2),
    left: Math.round(clientX - offsetLeft + 2),
  };
}

import { useEffect, useSyncExternalStore } from "react";
import { PREVIEW_WIDTH, SIDEBAR_WIDTH, useUi } from "../stores/ui";

/** Below this window width the sidebar stops docking and becomes an overlay drawer. */
export const COMPACT_BREAKPOINT = 860;
/** The conversation never gets squeezed narrower than this by the side panes. */
const MAIN_MIN = 360;

const subscribe = (cb: () => void) => {
  window.addEventListener("resize", cb);
  return () => window.removeEventListener("resize", cb);
};

export function useWindowWidth() {
  return useSyncExternalStore(subscribe, () => window.innerWidth);
}

const clamp = (v: number, min: number, max: number) => Math.min(Math.max(v, min), Math.max(min, max));

/** Upper bound for the docked sidebar at a given window width. */
export const sidebarMax = (win: number) => Math.min(SIDEBAR_WIDTH.max, Math.floor(win * 0.4));

/** Upper bound for the preview pane once the sidebar has taken its share. */
export const previewMax = (win: number, sidebar: number) =>
  Math.min(PREVIEW_WIDTH.max, win - sidebar - MAIN_MIN);

/**
 * Effective pane widths for the current window: the stored preferences clamped so the
 * workspace always keeps room. Also flips the store's `compact` flag at the breakpoint.
 */
export function useLayout() {
  const win = useWindowWidth();
  const compact = win < COMPACT_BREAKPOINT;
  const setCompact = useUi((s) => s.setCompact);
  useEffect(() => setCompact(compact), [compact, setCompact]);

  const collapsed = useUi((s) => s.sidebarCollapsed);
  const storedSidebar = useUi((s) => s.sidebarWidth);
  const storedPreview = useUi((s) => s.previewWidth);

  const sidebarWidth = clamp(storedSidebar, SIDEBAR_WIDTH.min, sidebarMax(win));
  // Space the docked sidebar actually occupies (none when collapsed or drawn over).
  const docked = compact || collapsed ? 0 : sidebarWidth;
  const pMax = previewMax(win, docked);
  // Too narrow to sit beside the conversation: the preview covers it instead.
  const previewOverlay = pMax < PREVIEW_WIDTH.min;
  const previewWidth = previewOverlay
    ? Math.min(win - docked, storedPreview)
    : clamp(storedPreview, PREVIEW_WIDTH.min, pMax);

  return { win, compact, sidebarWidth, docked, previewWidth, previewOverlay, previewMax: pMax };
}

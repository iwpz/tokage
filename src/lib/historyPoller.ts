/** Serial refreshes; stopped readers cannot overwrite a newer scope or selection. */
export function pollHistory<T>(read: () => Promise<T>, accept: (value: T) => void, reject: (error: unknown) => void, interval = 1000) {
  let stopped = false;
  let pending = false;
  let againAutomatic = false;
  let againManual = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const hasDocument = typeof document !== "undefined";
  const isVisible = () => !hasDocument || document.visibilityState !== "hidden";
  const schedule = (delay: number) => {
    clearTimeout(timer);
    timer = setTimeout(() => { timer = undefined; void run("automatic"); }, delay);
  };
  const run = async (kind: "initial" | "automatic" | "manual") => {
    if (stopped) return;
    if (kind === "automatic" && !isVisible()) return;
    if (pending) {
      if (kind === "manual") againManual = true;
      else againAutomatic = true;
      return;
    }
    clearTimeout(timer);
    timer = undefined;
    pending = true;
    try { const value = await read(); if (!stopped) accept(value); }
    catch (error) { if (!stopped) reject(error); }
    finally {
      pending = false;
      if (stopped) return;
      if (againManual) {
        againManual = false; againAutomatic = false;
        void run("manual");
      } else if (againAutomatic && isVisible()) {
        againAutomatic = false;
        schedule(0);
      } else {
        againAutomatic = false;
        if (isVisible()) schedule(interval);
      }
    }
  };
  const onVisibilityChange = () => {
    if (!isVisible()) {
      clearTimeout(timer); timer = undefined;
      againAutomatic = false;
    } else {
      if (pending) againAutomatic = true;
      else void run("automatic");
    }
  };
  if (hasDocument) document.addEventListener("visibilitychange", onVisibilityChange);
  // Preserve the initial read even when the window starts hidden.
  void run("initial");
  return {
    refresh: () => run("manual"),
    stop: () => {
      if (stopped) return;
      stopped = true; clearTimeout(timer); timer = undefined;
      if (hasDocument) document.removeEventListener("visibilitychange", onVisibilityChange);
      againAutomatic = false; againManual = false;
    },
  };
}

/** Serial refreshes; stopped readers cannot overwrite a newer scope or selection. */
export function pollHistory<T>(read: () => Promise<T>, accept: (value: T) => void, reject: (error: unknown) => void, interval = 1000) {
  let stopped = false;
  let pending = false;
  let again = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const refresh = async () => {
    if (stopped) return;
    if (pending) { again = true; return; }
    clearTimeout(timer);
    pending = true;
    try { const value = await read(); if (!stopped) accept(value); }
    catch (error) { if (!stopped) reject(error); }
    finally {
      pending = false;
      if (!stopped) { timer = setTimeout(() => { void refresh(); }, again ? 0 : interval); again = false; }
    }
  };
  void refresh();
  return { refresh, stop: () => { stopped = true; clearTimeout(timer); } };
}

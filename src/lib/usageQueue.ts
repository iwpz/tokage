/** Bound management processes before IPC deadlines start. Queued work rechecks consent/source. */
export function createUsageQueue(limit = 2) {
  let active = 0;
  const waiting: (() => void)[] = [];
  return { acquire: () => new Promise<() => void>((resolve) => {
    const grant = () => {
      active++;
      let released = false;
      resolve(() => {
        if (released) return;
        released = true;
        active--;
        waiting.shift()?.();
      });
    };
    if (active < limit) grant(); else waiting.push(grant);
  }) };
}

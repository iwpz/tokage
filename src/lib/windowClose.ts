/** Tauri's close listener must preventDefault synchronously before awaiting IO. */
export function createWindowCloseHandler(
  flush: () => Promise<void>,
  destroy: () => Promise<void>,
): (event: { preventDefault(): void }) => Promise<void> {
  let closing: Promise<void> | null = null;
  return async (event) => {
    event.preventDefault();
    if (closing) return closing;
    closing = (async () => {
      await flush();
      await destroy();
    })();
    try {
      await closing;
    } finally {
      closing = null;
    }
  };
}

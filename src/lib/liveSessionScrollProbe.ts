/** Opt-in desktop evidence only: real DOM geometry and real streaming events. */
export async function verifyLiveSessionScroll(
  turnId: string,
  report: (payload: object) => Promise<unknown>,
) {
  const wait = async <T>(read: () => T | false | null | undefined, label: string): Promise<T> => {
    const deadline = Date.now() + 180_000;
    while (Date.now() < deadline) {
      const value = read();
      if (value) return value;
      await new Promise(resolve => setTimeout(resolve, 50));
    }
    throw new Error(`Scroll probe timed out: ${label}`);
  };
  const frame = () => new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
  const active = () => document.querySelector<HTMLElement>('[data-message-role="ASSISTANT"][data-message-status="streaming"]');
  const size = () => active()?.dataset.streamSize;
  const geometry = (pane: HTMLElement) => ({ top: pane.scrollTop, height: pane.scrollHeight, viewport: pane.clientHeight });
  const bottom = (pane: HTMLElement) => pane.scrollHeight - pane.clientHeight - pane.scrollTop <= 8;
  const button = await wait(() => document.querySelector<HTMLButtonElement>(`button[data-attempt-id="${turnId}"]`), 'attempt');
  button.click();
  await wait(() => active(), 'actual streaming transcript');
  const pane = await wait(() => {
    const message = active();
    let p = message?.parentElement;
    while (p) {
      if (getComputedStyle(p).overflowY === 'auto') return p;
      p = p.parentElement;
    }
    return null;
  }, 'scroll container');
  await wait(() => pane.scrollHeight > pane.clientHeight + 100, 'real overflowing content');
  await frame();
  if (!bottom(pane)) throw new Error(`Initial selection did not show latest: ${JSON.stringify(geometry(pane))}`);
  let before = size();
  await wait(() => active() && size() !== before, 'new actual output at bottom');
  await frame();
  if (!bottom(pane)) throw new Error('New output did not follow while at bottom');
  await report({ status: 'running', step: 'scroll-follow-verified', turnId, geometry: geometry(pane), streamSize: size() });
  pane.scrollTop = 0;
  pane.dispatchEvent(new Event('scroll', { bubbles: true }));
  await frame();
  before = size();
  await wait(() => active() && size() !== before, 'new actual output while reading above');
  await frame();
  if (pane.scrollTop > 2) throw new Error(`Streaming stole reading position: ${pane.scrollTop}`);
  const latest = await wait(() => [...document.querySelectorAll<HTMLButtonElement>('button')].find(b => /latest/i.test(b.textContent ?? '') && b.getClientRects().length > 0), 'return to latest button');
  if (latest.disabled || latest.tabIndex < 0 || latest.type !== 'button') throw new Error('Return to latest is not keyboard accessible');
  latest.focus();
  if (document.activeElement !== latest) throw new Error('Cannot focus return to latest');
  latest.click();
  await frame();
  await wait(() => bottom(pane), 'return to bottom');
  await report({ status: 'running', step: 'scroll-reading-position-verified', turnId, preservedTop: 0, keyboardFocusable: true, geometry: geometry(pane), streamSize: size() });
  // Switch to another actual persisted attempt, then return: old reading intent
  // must not carry across attempt identity changes.
  pane.scrollTop = 0; pane.dispatchEvent(new Event('scroll', { bubbles: true }));
  await frame();
  const other = [...document.querySelectorAll<HTMLButtonElement>('button[data-attempt-id]')].find(b => b.dataset.attemptId !== turnId);
  if (!other) throw new Error('Missing independent attempt for selection-reset probe');
  other.click(); await frame();
  button.click(); await frame();
  await wait(() => pane.isConnected && bottom(pane), 'attempt switch resets follow intent');
  await report({ status: 'running', step: 'scroll-attempt-reset-verified', turnId, otherAttempt: other.dataset.attemptId, geometry: geometry(pane) });
  return pane;
}

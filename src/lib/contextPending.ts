/** F16.e: pending compact payload / settle ids between suggestion apply and next turn. */

let pendingCompactPrompt: string | null = null;
let pendingSettleActionId: number | null = null;
let pendingSettleBaseline: number | null = null;

export function setPendingCompact(prompt: string, actionId: number, baselineContinueCost: number) {
  pendingCompactPrompt = prompt;
  pendingSettleActionId = actionId;
  pendingSettleBaseline = baselineContinueCost;
}

export function takePendingCompactPrompt(): string | null {
  const p = pendingCompactPrompt;
  pendingCompactPrompt = null;
  return p;
}

export function takePendingSettle(): { actionId: number; baseline: number } | null {
  if (pendingSettleActionId == null || pendingSettleBaseline == null) return null;
  const out = { actionId: pendingSettleActionId, baseline: pendingSettleBaseline };
  pendingSettleActionId = null;
  pendingSettleBaseline = null;
  return out;
}

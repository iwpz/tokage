/** Kiro stores its rawOutput as JSON. The text inside that wrapper is already decoded once by JSON.parse. */
export function formatToolOutput(output: string, provider?: string): string {
  if (provider !== "KIRO_CLI") return output;

  let raw: unknown;
  try {
    raw = JSON.parse(output);
  } catch {
    return output;
  }

  if (typeof raw === "string") return raw;
  if (!raw || typeof raw !== "object" || !("items" in raw) || !Array.isArray(raw.items) || Object.keys(raw).length !== 1) {
    return output;
  }

  const blocks: string[] = [];
  for (const item of raw.items) {
    if (typeof item?.Text !== "string" || Object.keys(item).length !== 1) return output;
    blocks.push(item.Text);
  }
  return blocks.length > 0 ? blocks.join("\n") : output;
}

/**
 * Resolve domains for planning prompts / validation from the knowledge pack.
 * Builtin pack domains are the base; enabled custom packs may add extras.
 */
import builtinPackText from "../../src-tauri/resources/knowledge/builtin.json?raw";
import { parsePack } from "./knowledge";
import { useKnowledge } from "../stores/knowledge";

const builtinDomains = parsePack(builtinPackText).pack.domains;

/** Role-only domain — not assigned to execution tasks in the planner prompt. */
const ROLE_ONLY = new Set(["planning"]);

/** Domains the planner may put on tasks (excludes role-only `planning`). */
export function resolvePlanningDomains(): string[] {
  const extras = new Set<string>();
  for (const p of useKnowledge.getState().customPacks) {
    if (!p.enabled) continue;
    for (const d of p.pack.domains) extras.add(d);
  }

  const ordered: string[] = [];
  for (const d of builtinDomains) {
    if (ROLE_ONLY.has(d)) continue;
    ordered.push(d);
  }
  for (const d of extras) {
    if (ROLE_ONLY.has(d) || ordered.includes(d)) continue;
    ordered.push(d);
  }
  return ordered.length > 0 ? ordered : [...builtinDomains.filter((d) => !ROLE_ONLY.has(d))];
}

/** Full pack domain list for validatePlan (includes `planning`). */
export function resolveValidDomains(): string[] {
  const extras = new Set<string>();
  for (const p of useKnowledge.getState().customPacks) {
    if (!p.enabled) continue;
    for (const d of p.pack.domains) extras.add(d);
  }

  const ordered = [...builtinDomains];
  for (const d of extras) {
    if (!ordered.includes(d)) ordered.push(d);
  }
  return ordered;
}

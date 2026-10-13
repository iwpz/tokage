/**
 * Store persistence (docs/08, P1.m3). The stores live in memory; this module restores them
 * from SQLite at startup and sends their changes back as row upserts, batched every 300ms.
 *
 * On first launch after the switch, the old localStorage data (`tokage.*`) is imported in
 * one transaction. It is never deleted: if the import (or the database) fails, Tokage runs
 * on that data and keeps writing to localStorage in the old format, so a later retry starts
 * from the latest state. A fallback session after a done import marks the data dirty, so the
 * next successful start imports it again (rows are upserted). Outside Tauri (plain `vite`
 * dev) localStorage is the only store.
 */
import { create } from "zustand";
import {
  inTauri,
  storageImport,
  storageLoad,
  storageWrite,
  type StoredSnapshot,
  type WriteBatch,
} from "./ipc";
import { settleOnLoad, upgradeV1Message, useChat, type ChatMessage, type Conversation } from "../stores/chat";
import { useKnowledge, type CustomPack } from "../stores/knowledge";
import { useProjects, type Project } from "../stores/projects";
import { useProviders } from "../stores/providers";
import { migrateSettings, useSettings, type SettingsData } from "../stores/settings";
import { flushPipelines } from "../stores/execution";
import { UI_KEPT, useUi } from "../stores/ui";

const FLUSH_MS = 300;
export const LEGACY_IMPORTED = "meta.legacyImported";

/** Everything that is kept across launches. */
export interface Persisted {
  projects: Project[];
  activeProjectId: string | null;
  activeSessionId: string | null;
  conversations: Record<string, Conversation>;
  /** Plain values keyed as in the `settings` table: `settings.*`, `ui.*`, `chat.*`, ... */
  values: Record<string, unknown>;
  providers: unknown[];
  customPack: CustomPack | null;
}

// ---------------------------------------------------------------------------------------
// Capturing and applying store state

const plain = (o: object) => Object.entries(o).filter(([, v]) => typeof v !== "function");

export function capture(): Persisted {
  const p = useProjects.getState();
  const c = useChat.getState();
  const k = useKnowledge.getState();
  const pr = useProviders.getState();
  const ui = useUi.getState();
  const values: Record<string, unknown> = {
    "chat.picks": c.picks,
    "chat.modelOptions": c.modelOptions,
    "knowledge.url": k.url,
    "providers.lastScan": pr.lastScan,
    "projects.activeProjectId": p.activeProjectId,
    "projects.activeSessionId": p.activeSessionId,
  };
  for (const [key, v] of plain(useSettings.getState())) values[`settings.${key}`] = v;
  for (const key of UI_KEPT) values[`ui.${key}`] = ui[key];
  return {
    projects: p.projects,
    activeProjectId: p.activeProjectId,
    activeSessionId: p.activeSessionId,
    conversations: c.conversations,
    values,
    providers: pr.realProviders,
    customPack: k.custom,
  };
}

function apply(s: Persisted) {
  const pick = (prefix: string) =>
    Object.fromEntries(
      Object.entries(s.values)
        .filter(([key]) => key.startsWith(prefix))
        .map(([key, v]) => [key.slice(prefix.length), v]),
    );
  const v = s.values;
  useSettings.setState(migrateSettings(pick("settings.") as Partial<SettingsData>));
  useUi.setState(pick("ui."));
  useProjects.setState({
    projects: s.projects,
    activeProjectId: (v["projects.activeProjectId"] as string | null) ?? null,
    activeSessionId: (v["projects.activeSessionId"] as string | null) ?? null,
  });
  useChat.setState({
    conversations: Object.fromEntries(Object.entries(s.conversations).map(([id, c]) => [id, settleOnLoad(c)])),
    ...(v["chat.picks"] ? { picks: v["chat.picks"] as never } : {}),
    ...(v["chat.modelOptions"] ? { modelOptions: v["chat.modelOptions"] as never } : {}),
  });
  useProviders.setState({
    providers: s.providers as never,
    realProviders: s.providers as never,
    lastScan: (v["providers.lastScan"] as number | null) ?? null,
  });
  useKnowledge.setState({ url: (v["knowledge.url"] as string) ?? "", custom: s.customPack });
}

// ---------------------------------------------------------------------------------------
// Diffing into row writes

export const emptyBatch = (): WriteBatch => ({
  projects: [],
  sessions: [],
  sessionChat: [],
  messages: [],
  settings: [],
  providers: null,
  customPack: null,
  clearCustomPack: false,
  deleteSessions: [],
  deleteProjects: [],
});

/** Turn index, and the model and mode that ran the turn, for each message. */
function messageRow(sessionId: string, msgs: ChatMessage[], i: number, turn: number) {
  const m = msgs[i];
  // A prompt belongs to the attempt of the reply that follows it.
  const owner = m.role === "user" ? msgs[i + 1] : m;
  return {
    sessionId,
    seq: i,
    turnIndex: Math.max(turn, 0),
    provider: owner?.debugRoute?.actualProvider ?? owner?.ref?.provider ?? owner?.provider ?? null,
    model: owner?.debugRoute?.actualModel ?? owner?.ref?.model ?? owner?.model ?? null,
    mode: owner?.mode ?? null,
    message: owner?.debugRoute ? { ...m, debugRoute: owner.debugRoute, cwd: owner.cwd } : m,
  };
}

/**
 * Rows that changed from `prev` to `next`, by reference (the stores update immutably).
 * With `prev` null, everything. Conversations without a session row are skipped.
 */
export function diff(prev: Persisted | null, next: Persisted): WriteBatch {
  const b = emptyBatch();

  const sessionIds = new Set<string>();
  next.projects.forEach((p, i) => {
    const old = prev?.projects.find((x) => x.id === p.id);
    if (prev?.projects[i] !== p) {
      b.projects.push({ id: p.id, name: p.name, path: p.path, position: i, expanded: p.expanded, trusted: p.trusted ?? null });
    }
    p.sessions.forEach((s, j) => {
      sessionIds.add(s.id);
      if (old?.sessions[j] === s) return;
      b.sessions.push({
        id: s.id,
        projectId: p.id,
        title: s.title,
        state: s.state,
        position: j,
        archived: !!s.archived,
        forkedFrom: s.forkedFrom ?? null,
        worktree: s.worktree ?? null,
        updatedAt: s.updatedAt,
      });
    });
  });
  if (prev) {
    const projectIds = new Set(next.projects.map((p) => p.id));
    for (const p of prev.projects) {
      if (!projectIds.has(p.id)) b.deleteProjects.push(p.id);
      for (const s of p.sessions) if (!sessionIds.has(s.id)) b.deleteSessions.push(s.id);
    }
  }

  const prevSessionIds = new Set(prev?.projects.flatMap((p) => p.sessions.map((s) => s.id)) ?? []);
  for (const [id, c] of Object.entries(next.conversations)) {
    if (!sessionIds.has(id)) continue;
    // A conversation whose session row is new is written in full, even if it existed before.
    const old = prevSessionIds.has(id) ? prev?.conversations[id] : undefined;
    if (old === c) continue;
    if (old?.thread !== c.thread || old?.queue !== c.queue) {
      b.sessionChat.push({ sessionId: id, thread: c.thread ?? null, queue: c.queue ?? [] });
    }
    let turn = -1;
    c.messages.forEach((m, i) => {
      if (m.role === "user") turn++;
      const same = old?.messages[i] === m && (m.role !== "user" || (old.messages[i + 1]?.id === c.messages[i + 1]?.id && old.messages[i + 1]?.debugRoute === c.messages[i + 1]?.debugRoute));
      if (!same) b.messages.push(messageRow(id, c.messages, i, turn));
    });
  }

  for (const [key, v] of Object.entries(next.values)) {
    if (!prev || prev.values[key] !== v) b.settings.push({ key, value: v ?? null });
  }
  if (prev?.providers !== next.providers) b.providers = next.providers;
  if (prev?.customPack !== next.customPack) {
    if (next.customPack) b.customPack = next.customPack;
    else b.clearCustomPack = true;
  }
  return b;
}

const isEmpty = (b: WriteBatch) =>
  !b.providers &&
  !b.customPack &&
  !b.clearCustomPack &&
  [b.projects, b.sessions, b.sessionChat, b.messages, b.settings, b.deleteSessions, b.deleteProjects].every(
    (x) => x.length === 0,
  );

// ---------------------------------------------------------------------------------------
// The database snapshot and the legacy localStorage format

export function fromSnapshot(s: StoredSnapshot): Persisted {
  const values = { ...s.settings };
  delete values[LEGACY_IMPORTED];
  return {
    projects: s.projects as Project[],
    activeProjectId: (values["projects.activeProjectId"] as string | null) ?? null,
    activeSessionId: (values["projects.activeSessionId"] as string | null) ?? null,
    conversations: s.conversations as Record<string, Conversation>,
    values,
    providers: s.providers,
    customPack: s.customPack as CustomPack | null,
  };
}

/** The old persist keys and the versions their data was written with. */
const LEGACY = {
  projects: ["tokage.projects", 1],
  chat: ["tokage.chat", 2],
  settings: ["tokage.settings", 1],
  providers: ["tokage.providers", 2],
  ui: ["tokage.ui", 1],
  knowledge: ["tokage.knowledge", 1],
} as const;

/** Set while running on localStorage; its data is newer than the database's. */
const LEGACY_DIRTY = "tokage.legacyDirty";

type KV = Pick<Storage, "getItem" | "setItem" | "removeItem">;

function readKey(store: KV, key: string): { state: Record<string, unknown>; version: number } | null {
  try {
    const raw = store.getItem(key);
    if (!raw) return null;
    const v = JSON.parse(raw) as { state?: Record<string, unknown>; version?: number };
    return v && typeof v.state === "object" && v.state ? { state: v.state, version: v.version ?? 0 } : null;
  } catch {
    return null;
  }
}

export const hasLegacy = (store: KV) => Object.values(LEGACY).some(([key]) => store.getItem(key) !== null);

/** Reads the old zustand-persist data, applying the migrations the stores used to run. */
export function readLegacy(store: KV): Persisted {
  const projects = readKey(store, LEGACY.projects[0])?.state ?? {};
  const chat = readKey(store, LEGACY.chat[0]);
  const settings = readKey(store, LEGACY.settings[0])?.state ?? {};
  const providers = readKey(store, LEGACY.providers[0]);
  const ui = readKey(store, LEGACY.ui[0])?.state ?? {};
  const knowledge = readKey(store, LEGACY.knowledge[0])?.state ?? {};

  const conversations = (chat?.state.conversations ?? {}) as Record<string, Conversation>;
  if (chat && chat.version < 2) {
    for (const c of Object.values(conversations)) c.messages = c.messages.map(upgradeV1Message);
  }
  const values: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(settings)) values[`settings.${k}`] = v;
  for (const k of UI_KEPT) if (k in ui) values[`ui.${k}`] = ui[k];
  if (chat?.state.picks) values["chat.picks"] = chat.state.picks;
  if (chat?.state.modelOptions) values["chat.modelOptions"] = chat.state.modelOptions;
  if (typeof knowledge.url === "string") values["knowledge.url"] = knowledge.url;
  values["projects.activeProjectId"] = projects.activeProjectId ?? null;
  values["projects.activeSessionId"] = projects.activeSessionId ?? null;
  // A cache written for another ProviderInfo shape is dropped; the scan rebuilds it.
  const providersOk = providers?.version === LEGACY.providers[1];
  values["providers.lastScan"] = providersOk ? (providers.state.lastScan ?? null) : null;

  return {
    projects: (projects.projects as Project[]) ?? [],
    activeProjectId: (projects.activeProjectId as string | null) ?? null,
    activeSessionId: (projects.activeSessionId as string | null) ?? null,
    conversations,
    values,
    providers: providersOk ? ((providers.state.providers as unknown[]) ?? []) : [],
    customPack: (knowledge.custom as CustomPack | null) ?? null,
  };
}

/** Writes the state back in the old format (legacy mode only). */
export function writeLegacy(store: KV, s: Persisted) {
  const pick = (prefix: string) =>
    Object.fromEntries(
      Object.entries(s.values)
        .filter(([k]) => k.startsWith(prefix))
        .map(([k, v]) => [k.slice(prefix.length), v]),
    );
  const put = ([key, version]: readonly [string, number], state: unknown) =>
    store.setItem(key, JSON.stringify({ state, version }));
  put(LEGACY.projects, { projects: s.projects, activeProjectId: s.activeProjectId, activeSessionId: s.activeSessionId });
  put(LEGACY.chat, {
    conversations: s.conversations,
    picks: s.values["chat.picks"],
    modelOptions: s.values["chat.modelOptions"],
  });
  put(LEGACY.settings, pick("settings."));
  put(LEGACY.providers, { providers: s.providers, lastScan: s.values["providers.lastScan"] ?? null });
  put(LEGACY.ui, pick("ui."));
  put(LEGACY.knowledge, { url: s.values["knowledge.url"] ?? "", custom: s.customPack });
  store.setItem(LEGACY_DIRTY, "1");
}

// ---------------------------------------------------------------------------------------
// Runtime

interface StorageStatus {
  /** `db`: SQLite. `legacy`: localStorage, because the database or the import failed. */
  mode: "db" | "legacy";
  error: string | null;
  retrying: boolean;
}

export const useStorageStatus = create<StorageStatus>()(() => ({ mode: "db", error: null, retrying: false }));

let last: Persisted | null = null;
let timer: ReturnType<typeof setTimeout> | null = null;
let subscribed = false;

async function flush() {
  if (timer) clearTimeout(timer);
  timer = null;
  const next = capture();
  if (useStorageStatus.getState().mode === "legacy") {
    last = next;
    try {
      writeLegacy(localStorage, next);
    } catch (e) {
      console.error("localStorage write failed", e);
    }
    return;
  }
  const batch = diff(last, next);
  last = next;
  if (isEmpty(batch)) return;
  try {
    await storageWrite(batch);
  } catch (e) {
    console.error("database write failed", e);
  }
}

const schedule = () => {
  timer ??= setTimeout(() => void flush(), FLUSH_MS);
};

function subscribe() {
  if (subscribed) return;
  subscribed = true;
  for (const store of [useProjects, useChat, useSettings, useUi, useProviders, useKnowledge] as const) {
    (store.subscribe as (fn: () => void) => () => void)(schedule);
  }
  // Commit pending writes before the window goes.
  if (inTauri) {
    void import("@tauri-apps/api/window").then(({ getCurrentWindow }) =>
      getCurrentWindow().onCloseRequested(async () => {
        await Promise.all([flush(), flushPipelines()]);
      }),
    );
  }
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "hidden") void Promise.all([flush(), flushPipelines()]);
  });
}

/** Imports localStorage into SQLite if that hasn't happened, then loads from SQLite. */
async function loadFromDb(): Promise<Persisted> {
  const snap = await storageLoad();
  const dirty = localStorage.getItem(LEGACY_DIRTY) !== null;
  if ((snap.settings[LEGACY_IMPORTED] === true && !dirty) || !hasLegacy(localStorage)) return fromSnapshot(snap);
  await storageImport(diff(null, readLegacy(localStorage)));
  localStorage.removeItem(LEGACY_DIRTY);
  return fromSnapshot(await storageLoad());
}

/** Restores the stores before the first render and starts persisting their changes. */
export async function hydrate() {
  if (!inTauri) {
    useStorageStatus.setState({ mode: "legacy", error: null });
    apply(readLegacy(localStorage));
  } else {
    try {
      apply(await loadFromDb());
      useStorageStatus.setState({ mode: "db", error: null });
      // F14.f: custom packs + model overrides live in dedicated tables
      await useKnowledge.getState().hydrateFromDb();
    } catch (e) {
      console.error("storage", e);
      useStorageStatus.setState({ mode: "legacy", error: String(e) });
      apply(readLegacy(localStorage));
    }
  }
  last = capture();
  subscribe();
}

/** After a failed import: writes the latest state to localStorage, then imports again. */
export async function retryImport() {
  useStorageStatus.setState({ retrying: true });
  await flush();
  try {
    const s = await loadFromDb();
    useStorageStatus.setState({ mode: "db", error: null, retrying: false });
    apply(s);
    last = capture();
  } catch (e) {
    useStorageStatus.setState({ error: String(e), retrying: false });
  }
}

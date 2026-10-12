import { create } from "zustand";
import {
  fetchKnowledgePack,
  checkKnowledgePackUpdate,
  getPendingKnowledgeUpdate,
  applyKnowledgePackUpdate,
  getBuildInfo,
  replaceCustomPacks,
  listCustomPacks,
  upsertModelOverride,
  deleteModelOverride,
  listModelOverrides,
  type StagedUpdateInfo,
} from "../lib/ipc";
import { parsePack, serializePack, type KnowledgePack, type ModelOverride, type PackModel } from "../lib/knowledge";
import i18n from "../i18n";

/** Persist custom packs to SQLite (F14.f). */
function persistPacks(packs: CustomPackItem[]) {
  void replaceCustomPacks(
    packs.map((p) => ({
      name: p.name,
      url: p.url,
      pack: p.pack,
      warnings: p.warnings,
      enabled: p.enabled,
    })),
  ).catch((err) => console.warn("[knowledge] replace_custom_packs failed:", err));
}

/** A custom pack item in the managed list (docs/04 knowledge-pack, P6.m3a, P6.m3b). */
export interface CustomPackItem {
  id: string;
  name: string;
  url?: string;
  loadedAt: number;
  pack: KnowledgePack;
  warnings: string[];
  enabled: boolean;
  isLocalFile: boolean;
}

/** Legacy-compatible single custom pack interface. */
export interface CustomPack {
  url: string;
  loadedAt: number;
  pack: KnowledgePack;
  warnings: string[];
}

interface KnowledgeState {
  /** Last URL typed in the field. */
  url: string;
  /** Primary / active custom pack for backward compatibility with db persistence. */
  custom: CustomPack | null;
  /** Ordered list of imported custom packs (P6.m3a). */
  customPacks: CustomPackItem[];
  /** Locally saved model overrides (P6.m3c). */
  modelOverrides: Record<string, ModelOverride>;
  loading: boolean;
  error: string | null;
  setUrl: (url: string) => void;
  /** Downloads and parses `url`, adds to customPacks and updates active custom pack. */
  load: (url: string) => Promise<void>;
  /** Imports a local JSON pack file (P6.m3b). */
  importFromFile: (fileName: string, content: string) => void;
  /** Removes a pack by ID. */
  removeCustomPack: (id: string) => void;
  /** Toggles a pack's enabled status. */
  toggleCustomPack: (id: string) => void;
  /** Reorders a pack up or down. */
  moveCustomPack: (id: string, direction: "up" | "down") => void;
  /** Copies a model as a local override (P6.m3c). */
  addModelOverride: (model: PackModel) => void;
  /** Removes a model override. */
  removeModelOverride: (modelId: string) => void;
  /** Exports effective or specified pack as downloadable custom pack JSON (P6.m3c). */
  exportPackJson: (pack: KnowledgePack) => string;
  /** Staged official update if verified and pending (P8.m4a). */
  stagedUpdate: StagedUpdateInfo | null;
  checkingUpdate: boolean;
  applyingUpdate: boolean;
  updateMessage: string | null;
  /** Checks remote official source for knowledge pack update (P8.m4b). */
  checkOfficialUpdate: (currentPackVersion: string) => Promise<void>;
  /** Applies staged update to official layer (P8.m4a). */
  applyOfficialUpdate: () => Promise<void>;
  /** Refreshes staged update status on startup or mount. */
  refreshPendingUpdate: () => Promise<void>;
  /** F14.f: load customPacks + modelOverrides from SQLite. */
  hydrateFromDb: () => Promise<void>;
  resetToBuiltin: () => void;
}

function syncActive(packs: CustomPackItem[]): CustomPack | null {
  const active = packs.find((p) => p.enabled);
  if (!active) return null;
  return {
    url: active.url ?? active.name,
    loadedAt: active.loadedAt,
    pack: active.pack,
    warnings: active.warnings,
  };
}

export const useKnowledge = create<KnowledgeState>()(
  (set, get) => ({
    url: "",
    custom: null,
    customPacks: [],
    modelOverrides: {},
    loading: false,
    error: null,
    stagedUpdate: null,
    checkingUpdate: false,
    applyingUpdate: false,
    updateMessage: null,
    setUrl: (url) => set({ url, error: null }),
    load: async (url) => {
      set({ loading: true, error: null });
      try {
        const text = await fetchKnowledgePack(url);
        const { pack, warnings } = parsePack(text);
        const newItem: CustomPackItem = {
          id: `pack-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`,
          name: pack.packVersion || url,
          url,
          loadedAt: Date.now(),
          pack,
          warnings,
          enabled: true,
          isLocalFile: false,
        };
        const updated = [newItem, ...get().customPacks];
        set({
          customPacks: updated,
          custom: { url, loadedAt: newItem.loadedAt, pack, warnings },
          loading: false,
        });
        persistPacks(updated);
      } catch (e) {
        set({ loading: false, error: e instanceof Error ? e.message : String(e) });
      }
    },
    importFromFile: (fileName: string, content: string) => {
      try {
        const { pack, warnings } = parsePack(content);
        const newItem: CustomPackItem = {
          id: `file-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`,
          name: fileName || pack.packVersion || "local.json",
          loadedAt: Date.now(),
          pack,
          warnings,
          enabled: true,
          isLocalFile: true,
        };
        const updated = [newItem, ...get().customPacks];
        set({
          customPacks: updated,
          custom: syncActive(updated),
          error: null,
        });
        persistPacks(updated);
      } catch (e) {
        set({ error: e instanceof Error ? e.message : String(e) });
        throw e;
      }
    },
    removeCustomPack: (id: string) => {
      const updated = get().customPacks.filter((p) => p.id !== id);
      set({
        customPacks: updated,
        custom: syncActive(updated),
      });
      persistPacks(updated);
    },
    toggleCustomPack: (id: string) => {
      const updated = get().customPacks.map((p) =>
        p.id === id ? { ...p, enabled: !p.enabled } : p,
      );
      set({
        customPacks: updated,
        custom: syncActive(updated),
      });
      persistPacks(updated);
    },
    moveCustomPack: (id: string, direction: "up" | "down") => {
      const list = [...get().customPacks];
      const idx = list.findIndex((p) => p.id === id);
      if (idx === -1) return;
      const target = direction === "up" ? idx - 1 : idx + 1;
      if (target < 0 || target >= list.length) return;
      const temp = list[idx];
      list[idx] = list[target];
      list[target] = temp;
      set({
        customPacks: list,
        custom: syncActive(list),
      });
      persistPacks(list);
    },
    addModelOverride: (model: PackModel) => {
      const override: ModelOverride = {
        modelId: model.modelId,
        displayName: model.displayName,
        skills: { ...model.skills },
        price: model.price ? { ...model.price } : undefined,
        contextWindow: model.contextWindow ?? undefined,
        effectiveContext: model.effectiveContext ?? undefined,
        speed: model.speed ?? undefined,
        tags: [...model.tags],
        updatedAt: Date.now(),
      };
      set({
        modelOverrides: {
          ...get().modelOverrides,
          [model.modelId]: override,
        },
      });
      // Provider unknown at this layer — store under "*" until F14 enrich
      void upsertModelOverride("*", model.modelId, override).catch((err) =>
        console.warn("[knowledge] upsert_model_override failed:", err),
      );
    },
    removeModelOverride: (modelId: string) => {
      const next = { ...get().modelOverrides };
      delete next[modelId];
      set({ modelOverrides: next });
      void deleteModelOverride("*", modelId).catch((err) =>
        console.warn("[knowledge] delete_model_override failed:", err),
      );
    },
    exportPackJson: (pack: KnowledgePack) => {
      return serializePack(pack);
    },
    checkOfficialUpdate: async (currentPackVersion: string) => {
      // F8.a: skip IPC entirely when the official channel is disabled (no pubkey).
      const info = await getBuildInfo();
      if (!info.knowledgeUpdateEnabled) {
        set({
          checkingUpdate: false,
          updateMessage: null,
          error: null,
        });
        return;
      }
      set({ checkingUpdate: true, updateMessage: null, error: null });
      try {
        const update = await checkKnowledgePackUpdate(currentPackVersion);
        if (update) {
          set({
            stagedUpdate: update,
            checkingUpdate: false,
            updateMessage: i18n.t("knowledgeStore.updateFound", { packVersion: update.packVersion }),
          });
        } else {
          set({
            stagedUpdate: null,
            checkingUpdate: false,
            updateMessage: i18n.t("knowledgeStore.upToDate"),
          });
        }
      } catch (e) {
        set({
          checkingUpdate: false,
          error: e instanceof Error ? e.message : String(e),
        });
      }
    },
    applyOfficialUpdate: async () => {
      set({ applyingUpdate: true, error: null });
      try {
        await applyKnowledgePackUpdate();
        set({
          stagedUpdate: null,
          applyingUpdate: false,
          updateMessage: i18n.t("knowledgeStore.updated"),
        });
      } catch (e) {
        set({
          applyingUpdate: false,
          error: e instanceof Error ? e.message : String(e),
        });
      }
    },
    refreshPendingUpdate: async () => {
      try {
        const pending = await getPendingKnowledgeUpdate();
        set({ stagedUpdate: pending });
      } catch (err) {
        console.error("Failed to check pending knowledge update:", err);
      }
    },
    hydrateFromDb: async () => {
      try {
        const [packs, overrides] = await Promise.all([
          listCustomPacks(),
          listModelOverrides(),
        ]);
        const customPacks: CustomPackItem[] = packs.map((p, i) => {
          let pack: KnowledgePack;
          try {
            pack =
              typeof p.pack === "object" && p.pack
                ? (p.pack as KnowledgePack)
                : parsePack(JSON.stringify(p.pack ?? {})).pack;
          } catch {
            pack = parsePack("{}").pack;
          }
          return {
            id: `db-${i}-${p.name}`,
            name: p.name,
            loadedAt: Date.now(),
            pack,
            warnings: [],
            enabled: p.enabled,
            isLocalFile: false,
          };
        });
        const modelOverrides: Record<string, ModelOverride> = {};
        for (const o of overrides) {
          try {
            const patch = JSON.parse(o.patchJson) as ModelOverride;
            modelOverrides[o.modelId] = {
              ...patch,
              modelId: o.modelId,
              updatedAt: patch.updatedAt ?? Date.now(),
            };
          } catch {
            /* skip */
          }
        }
        set({
          customPacks,
          custom: syncActive(customPacks),
          modelOverrides,
        });
      } catch (err) {
        console.warn("[knowledge] hydrateFromDb failed:", err);
      }
    },
    resetToBuiltin: () => {
      set({
        custom: null,
        customPacks: [],
        modelOverrides: {},
        error: null,
      });
      persistPacks([]);
    },
  }),
);

import { create } from "zustand";
import { fetchKnowledgePack } from "../lib/ipc";
import { parsePack, serializePack, type KnowledgePack, type ModelOverride, type PackModel } from "../lib/knowledge";

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
    },
    toggleCustomPack: (id: string) => {
      const updated = get().customPacks.map((p) =>
        p.id === id ? { ...p, enabled: !p.enabled } : p,
      );
      set({
        customPacks: updated,
        custom: syncActive(updated),
      });
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
    },
    removeModelOverride: (modelId: string) => {
      const next = { ...get().modelOverrides };
      delete next[modelId];
      set({ modelOverrides: next });
    },
    exportPackJson: (pack: KnowledgePack) => {
      return serializePack(pack);
    },
    resetToBuiltin: () =>
      set({
        custom: null,
        customPacks: [],
        modelOverrides: {},
        error: null,
      }),
  }),
);

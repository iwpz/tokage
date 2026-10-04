import { create } from "zustand";
import { fetchKnowledgePack } from "../lib/ipc";
import { parsePack, type KnowledgePack } from "../lib/knowledge";

/** A pack loaded from a custom URL. Anything not shipped with Tokage counts as third-party. */
export interface CustomPack {
  url: string;
  loadedAt: number;
  pack: KnowledgePack;
  warnings: string[];
}

interface KnowledgeState {
  /** Last URL typed in the field, kept so updating a pack is one click. */
  url: string;
  custom: CustomPack | null;
  loading: boolean;
  error: string | null;
  setUrl: (url: string) => void;
  /** Downloads and parses `url`; the active pack only changes when both succeed. */
  load: (url: string) => Promise<void>;
  resetToBuiltin: () => void;
}

export const useKnowledge = create<KnowledgeState>()(
  (set) => ({
    url: "",
    custom: null,
    loading: false,
    error: null,
    setUrl: (url) => set({ url, error: null }),
    load: async (url) => {
      set({ loading: true, error: null });
      try {
        const { pack, warnings } = parsePack(await fetchKnowledgePack(url));
        set({ custom: { url, loadedAt: Date.now(), pack, warnings }, loading: false });
      } catch (e) {
        set({ loading: false, error: e instanceof Error ? e.message : String(e) });
      }
    },
    resetToBuiltin: () => set({ custom: null, error: null }),
  }),
);

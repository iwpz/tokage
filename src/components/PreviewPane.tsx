import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { FileCode2, X } from "lucide-react";
import { cn } from "../lib/cn";
import { readProjectFile, type FileText } from "../lib/ipc";
import { PREVIEW_WIDTH, useUi } from "../stores/ui";
import { useLayout } from "../lib/layout";
import { DiffView } from "./DiffView";
import { Markdown } from "./Markdown";
import { IconButton } from "./ui";
import { Splitter } from "./Splitter";

type View = "diff" | "file";

/** Right-hand pane: a changed file's diff, or its current content. */
export function PreviewPane() {
  const { t } = useTranslation();
  const preview = useUi((s) => s.preview);
  const close = useUi((s) => s.closePreview);
  const setWidth = useUi((s) => s.setPreviewWidth);
  const { previewWidth, previewOverlay, previewMax } = useLayout();
  const [view, setView] = useState<View>("diff");
  const [file, setFile] = useState<FileText | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setView(preview?.diff ? "diff" : "file");
    setFile(null);
    setError(null);
    if (!preview || preview.change === "deleted") return;
    let cancelled = false;
    readProjectFile(preview.root, preview.path)
      .then((f) => !cancelled && setFile(f))
      .catch((e) => !cancelled && setError(String(e)));
    return () => {
      cancelled = true;
    };
  }, [preview]);

  useEffect(() => {
    if (!preview) return;
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && close();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [preview, close]);

  if (!preview) return null;
  const name = preview.path.split(/[\\/]/).pop() ?? preview.path;
  const markdown = /\.(md|markdown)$/i.test(name);

  return (
    <aside
      aria-label={t("workspace.preview.label")}
      style={{ width: previewWidth }}
      className={cn(
        "flex shrink-0 flex-col border-l border-border bg-elevated",
        // No room beside the conversation: float over it, flush right.
        previewOverlay ? "absolute inset-y-0 right-0 z-20 shadow-xl" : "relative",
      )}
    >
      {!previewOverlay && (
        <Splitter
          label={t("workspace.preview.resize")}
          side="right"
          value={previewWidth}
          min={PREVIEW_WIDTH.min}
          max={previewMax}
          onChange={setWidth}
          onReset={() => setWidth(PREVIEW_WIDTH.default)}
        />
      )}
      <header className="flex h-10 shrink-0 items-center gap-2 border-b border-border pr-1.5 pl-3">
        <FileCode2 aria-hidden className="size-4 shrink-0 text-muted" />
        <div className="min-w-0 flex-1">
          <p className="truncate text-[13px] font-medium" title={preview.path}>
            {name}
          </p>
        </div>
        {preview.diff && preview.change !== "deleted" && (
          <div role="tablist" aria-label={t("workspace.preview.view")} className="flex gap-0.5 rounded-md bg-hover p-0.5">
            {(["diff", "file"] as View[]).map((v) => (
              <button
                key={v}
                type="button"
                role="tab"
                aria-selected={view === v}
                onClick={() => setView(v)}
                className={cn("h-6 rounded px-2 text-xs", view === v ? "bg-elevated text-fg shadow-sm" : "text-muted hover:text-fg")}
              >
                {t(`workspace.preview.${v}`)}
              </button>
            ))}
          </div>
        )}
        <IconButton label={t("workspace.preview.close")} onClick={close}>
          <X className="size-4" />
        </IconButton>
      </header>
      <p className="truncate border-b border-border px-3 py-1 font-mono text-[11px] text-subtle" title={preview.path}>
        {preview.path}
      </p>

      <div className="min-h-0 flex-1 overflow-auto">
        {view === "diff" && preview.diff ? (
          <DiffView diff={preview.diff} className="py-2" />
        ) : preview.change === "deleted" ? (
          <Empty text={t("workspace.preview.deleted")} />
        ) : error ? (
          <Empty text={error} danger />
        ) : !file ? (
          <Empty text={t("workspace.preview.loading")} />
        ) : file.text === null ? (
          <Empty text={file.binary ? t("workspace.preview.binary") : t("workspace.preview.tooLarge")} />
        ) : markdown ? (
          <Markdown text={file.text} className="px-5 py-4 text-[14px]" />
        ) : (
          <Code text={file.text} />
        )}
      </div>
    </aside>
  );
}

function Code({ text }: { text: string }) {
  const lines = text.replace(/\n$/, "").split("\n");
  return (
    <div data-selectable className="py-2 font-mono text-[12px] leading-[1.6]">
      <table className="min-w-full border-collapse">
        <tbody>
          {lines.map((l, i) => (
            <tr key={i}>
              <td className="w-12 pr-3 pl-3 text-right text-subtle select-none">{i + 1}</td>
              <td className="pr-4 whitespace-pre">{l}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function Empty({ text, danger }: { text: string; danger?: boolean }) {
  return <p className={cn("p-6 text-center text-[13px]", danger ? "text-danger" : "text-subtle")}>{text}</p>;
}

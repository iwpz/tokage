import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Code2, Edit3, Eye, FileCode, Plus, Trash2 } from "lucide-react";
import { Badge, Button, Segmented } from "../ui";
import { Markdown } from "../Markdown";
import type { PlanContract } from "../../lib/ipc";
import { cn } from "../../lib/cn";

export function ContractsEditor({
  contracts,
  onUpdate,
  onAdd,
  onRemove,
}: {
  contracts: PlanContract[];
  onUpdate: (id: string, patch: Partial<PlanContract>) => void;
  onAdd: (contract: PlanContract) => void;
  onRemove: (id: string) => void;
}) {
  const { t } = useTranslation();
  const [selectedId, setSelectedId] = useState<string>(contracts[0]?.id ?? "");
  const [tab, setTab] = useState<"preview" | "edit">("preview");

  const [newTitle, setNewTitle] = useState("");
  const [showAddModal, setShowAddModal] = useState(false);

  const selectedContract = contracts.find((c) => c.id === selectedId) ?? contracts[0];

  const handleCreateContract = () => {
    if (!newTitle.trim()) return;
    const nextId = `C${contracts.length + 1}`;
    const newContract: PlanContract = {
      id: nextId,
      title: newTitle.trim(),
      content: `### ${newTitle.trim()}\n\n\`\`\`typescript\n${t("plan.contracts.templateComment")}\n\`\`\`\n`,
    };
    onAdd(newContract);
    setSelectedId(nextId);
    setNewTitle("");
    setShowAddModal(false);
    setTab("edit");
  };

  return (
    <div className="flex h-full flex-col overflow-hidden bg-base">
      {/* Subheader bar */}
      <div className="flex items-center justify-between border-b border-border px-5 py-2.5">
        <div className="flex items-center gap-2">
          <Code2 className="size-4 text-accent" />
          <h2 className="text-sm font-semibold text-fg">{t("plan.contracts.title")}</h2>
          <Badge variant="accent" size="xs">
            {t("plan.contracts.count", { count: contracts.length })}
          </Badge>
        </div>
        <div className="flex items-center gap-2">
          {selectedContract && (
            <Segmented
              label={t("plan.contracts.viewMode")}
              value={tab}
              onChange={(v) => setTab(v)}
              options={[
                { value: "preview", label: t("plan.contracts.preview"), icon: <Eye className="size-3.5" /> },
                { value: "edit", label: t("plan.contracts.edit"), icon: <Edit3 className="size-3.5" /> },
              ]}
            />
          )}
          <Button
            size="sm"
            variant="secondary"
            onClick={() => setShowAddModal(true)}
            className="gap-1 text-xs"
          >
            <Plus className="size-3.5" />
            {t("plan.contracts.new")}
          </Button>
        </div>
      </div>

      {contracts.length === 0 ? (
        <div className="flex flex-1 flex-col items-center justify-center p-8 text-center">
          <FileCode className="size-10 text-subtle mb-3" />
          <h3 className="text-sm font-medium text-fg">{t("plan.contracts.emptyTitle")}</h3>
          <p className="mt-1 text-xs text-muted max-w-sm">
            {t("plan.contracts.emptyBody")}
          </p>
          <Button
            size="sm"
            variant="primary"
            onClick={() => setShowAddModal(true)}
            className="mt-4 gap-1.5"
          >
            <Plus className="size-3.5" />
            {t("plan.contracts.addFirst")}
          </Button>
        </div>
      ) : (
        <div className="flex flex-1 min-h-0">
          {/* Left contract list tab bar */}
          <div className="w-56 shrink-0 border-r border-border bg-elevated/40 flex flex-col overflow-y-auto p-2 space-y-1">
            <div className="px-2 py-1.5 text-[11px] font-semibold uppercase tracking-wider text-muted">
              {t("plan.contracts.list")}
            </div>
            {contracts.map((c) => {
              const active = c.id === (selectedContract?.id ?? "");
              return (
                <button
                  key={c.id}
                  type="button"
                  onClick={() => setSelectedId(c.id)}
                  className={cn(
                    "flex w-full items-center justify-between rounded-lg px-2.5 py-2 text-left transition-colors",
                    active
                      ? "bg-accent/15 text-accent font-medium border border-accent/20"
                      : "text-muted hover:bg-hover hover:text-fg border border-transparent",
                  )}
                >
                  <div className="flex items-center gap-2 min-w-0">
                    <span className="font-mono text-xs font-semibold shrink-0">{c.id}</span>
                    <span className="text-xs truncate">{c.title || t("plan.contracts.untitled")}</span>
                  </div>
                </button>
              );
            })}
          </div>

          {/* Right main contract view/edit area */}
          {selectedContract ? (
            <div className="flex-1 flex flex-col min-h-0 bg-base">
              {/* Contract header */}
              <div className="flex items-center justify-between border-b border-border px-5 py-3 bg-elevated/20">
                <div className="flex items-center gap-2.5 flex-1 max-w-lg">
                  <span className="font-mono text-xs font-bold px-1.5 py-0.5 rounded bg-hover text-accent">
                    {selectedContract.id}
                  </span>
                  <input
                    type="text"
                    value={selectedContract.title}
                    onChange={(e) => onUpdate(selectedContract.id, { title: e.target.value })}
                    placeholder={t("plan.contracts.titlePlaceholder")}
                    className="flex-1 rounded-md border border-border/80 bg-base px-2.5 py-1 text-sm font-semibold text-fg focus:outline-none focus:border-ring"
                  />
                </div>
                <button
                  type="button"
                  onClick={() => onRemove(selectedContract.id)}
                  title={t("plan.contracts.deleteTitle")}
                  className="rounded-md p-1.5 text-subtle hover:bg-hover hover:text-danger transition-colors"
                >
                  <Trash2 className="size-4" />
                </button>
              </div>

              {/* Editor/Preview content */}
              <div className="flex-1 overflow-y-auto p-5">
                {tab === "edit" ? (
                  <textarea
                    value={selectedContract.content}
                    onChange={(e) => onUpdate(selectedContract.id, { content: e.target.value })}
                    placeholder={t("plan.contracts.contentPlaceholder")}
                    rows={20}
                    className="w-full h-full min-h-[380px] rounded-xl border border-border bg-elevated p-4 font-mono text-[13px] leading-relaxed text-fg focus:outline-none focus:border-ring resize-none"
                  />
                ) : (
                  <div className="rounded-xl border border-border bg-elevated p-6 min-h-full">
                    {selectedContract.content ? (
                      <Markdown text={selectedContract.content} defaultCollapsed={false} className="text-[13px]" />
                    ) : (
                      <div className="text-xs text-subtle italic">{t("plan.contracts.contentEmpty")}</div>
                    )}
                  </div>
                )}
              </div>
            </div>
          ) : null}
        </div>
      )}

      {/* Add contract dialog */}
      {showAddModal && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4 backdrop-blur-xs">
          <div className="w-full max-w-md rounded-2xl border border-border bg-elevated p-5 shadow-2xl">
            <h3 className="text-sm font-semibold text-fg mb-1">{t("plan.contracts.addModalTitle")}</h3>
            <p className="text-xs text-muted mb-4">
              {t("plan.contracts.addModalBody")}
            </p>
            <input
              type="text"
              autoFocus
              value={newTitle}
              onChange={(e) => setNewTitle(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && (e.preventDefault(), handleCreateContract())}
              placeholder={t("plan.contracts.namePlaceholder")}
              className="w-full rounded-lg border border-border bg-elevated px-3 py-2 text-sm text-fg focus:outline-none focus:border-ring mb-4"
            />
            <div className="flex justify-end gap-2">
              <Button size="sm" variant="secondary" onClick={() => setShowAddModal(false)}>
                {t("common.cancel")}
              </Button>
              <Button size="sm" variant="primary" onClick={handleCreateContract} disabled={!newTitle.trim()}>
                {t("plan.contracts.create")}
              </Button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

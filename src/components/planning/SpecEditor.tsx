import { useState } from "react";
import { useTranslation } from "react-i18next";
import { CheckCircle2, CircleSlash, Compass, Edit3, Eye, Lightbulb, Plus, Trash2 } from "lucide-react";
import { Badge, Button, Segmented } from "../ui";
import { Markdown } from "../Markdown";
import type { PlanSpec } from "../../lib/ipc";

export function SpecEditor({
  spec,
  onChange,
}: {
  spec: PlanSpec;
  onChange: (patch: Partial<PlanSpec>) => void;
}) {
  const { t } = useTranslation();
  const [tab, setTab] = useState<"structured" | "markdown">("structured");
  const [editing, setEditing] = useState(false);

  const [newInScope, setNewInScope] = useState("");
  const [newOutOfScope, setNewOutOfScope] = useState("");
  const [newDecision, setNewDecision] = useState("");

  const addInScope = () => {
    if (!newInScope.trim()) return;
    onChange({ inScope: [...spec.inScope, newInScope.trim()] });
    setNewInScope("");
  };

  const removeInScope = (idx: number) => {
    onChange({ inScope: spec.inScope.filter((_, i) => i !== idx) });
  };

  const addOutOfScope = () => {
    if (!newOutOfScope.trim()) return;
    onChange({ outOfScope: [...spec.outOfScope, newOutOfScope.trim()] });
    setNewOutOfScope("");
  };

  const removeOutOfScope = (idx: number) => {
    onChange({ outOfScope: spec.outOfScope.filter((_, i) => i !== idx) });
  };

  const addDecision = () => {
    if (!newDecision.trim()) return;
    onChange({ decisions: [...spec.decisions, newDecision.trim()] });
    setNewDecision("");
  };

  const removeDecision = (idx: number) => {
    onChange({ decisions: spec.decisions.filter((_, i) => i !== idx) });
  };

  return (
    <div className="@container flex h-full min-h-0 min-w-0 flex-col overflow-hidden bg-base">
      {/* Subheader bar */}
      <div className="flex shrink-0 flex-wrap items-center justify-between gap-2 border-b border-border px-5 py-2.5">
        <div className="flex min-w-0 flex-wrap items-center gap-2">
          <Compass className="size-4 text-accent" />
          <h2 className="text-sm font-semibold text-fg">{t("plan.spec.title")}</h2>
          <Badge variant="accent" size="xs">
            {t("plan.spec.stats", { inScope: spec.inScope.length, decisions: spec.decisions.length })}
          </Badge>
        </div>
        <div className="flex min-w-0 flex-wrap items-center gap-2">
          <Segmented
            label={t("plan.spec.viewMode")}
            value={tab}
            onChange={(v) => setTab(v)}
            options={[
              { value: "structured", label: t("plan.spec.structured") },
              { value: "markdown", label: t("plan.spec.markdown") },
            ]}
          />
          {tab === "structured" && (
            <Button
              size="sm"
              variant={editing ? "primary" : "secondary"}
              onClick={() => setEditing(!editing)}
              className="gap-1 text-xs"
            >
              {editing ? <Eye className="size-3.5" /> : <Edit3 className="size-3.5" />}
              {editing ? t("plan.spec.doneEditing") : t("plan.spec.edit")}
            </Button>
          )}
        </div>
      </div>

      {/* Main content area */}
      <div className="min-h-0 min-w-0 flex-1 overflow-y-auto p-6 space-y-6">
        {tab === "markdown" ? (
          <div className="flex flex-col gap-2 h-full">
            <textarea
              value={spec.raw}
              onChange={(e) => onChange({ raw: e.target.value })}
              placeholder={t("plan.spec.markdownPlaceholder")}
              rows={18}
              className="w-full flex-1 rounded-xl border border-border bg-elevated p-4 font-mono text-[13px] leading-relaxed text-fg focus:outline-none focus:border-ring resize-none"
            />
          </div>
        ) : (
          <>
            {/* Goal Card */}
            <div className="rounded-xl border border-border bg-elevated p-5">
              <div className="flex items-center gap-2 mb-2 text-xs font-semibold uppercase tracking-wider text-muted">
                <Compass className="size-3.5 text-accent" />
                <span>{t("plan.spec.goal")}</span>
              </div>
              {editing ? (
                <textarea
                  value={spec.goal}
                  onChange={(e) => onChange({ goal: e.target.value })}
                  placeholder={t("plan.spec.goalPlaceholder")}
                  rows={3}
                  className="w-full rounded-lg border border-border bg-base px-3 py-2 text-sm text-fg focus:outline-none focus:border-ring"
                />
              ) : (
                <div className="text-[14px] leading-relaxed font-medium text-fg">
                  {spec.goal || <span className="text-subtle italic">{t("plan.spec.goalEmpty")}</span>}
                </div>
              )}
            </div>

            {/* Scope Grid: In Scope vs Out of Scope */}
            <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
              {/* In Scope */}
              <div className="rounded-xl border border-border bg-elevated p-5 flex min-w-0 flex-col">
                <div className="flex items-center justify-between mb-3">
                  <div className="flex items-center gap-1.5 text-xs font-semibold uppercase tracking-wider text-success">
                    <CheckCircle2 className="size-3.5" />
                    <span>{t("plan.spec.inScope")}</span>
                  </div>
                  <Badge variant="success" size="xs">
                    {spec.inScope.length}
                  </Badge>
                </div>
                <div className="space-y-2 flex-1">
                  {spec.inScope.length === 0 ? (
                    <div className="text-xs text-subtle italic py-2">{t("plan.spec.inScopeEmpty")}</div>
                  ) : (
                    spec.inScope.map((item, idx) => (
                      <div
                        key={idx}
                        className="group flex items-start justify-between gap-2 rounded-lg bg-base px-3 py-2 text-[13px] text-fg border border-border/50"
                      >
                        <div className="flex flex-1 items-start gap-2 min-w-0">
                          <span className="mt-1 size-1.5 rounded-full bg-success shrink-0" />
                          <span className="min-w-0 whitespace-normal [overflow-wrap:anywhere] leading-relaxed">{item}</span>
                        </div>
                        {editing && (
                          <button
                            type="button"
                            onClick={() => removeInScope(idx)}
                            className="text-subtle hover:text-danger opacity-0 group-hover:opacity-100 transition-opacity p-0.5"
                          >
                            <Trash2 className="size-3.5" />
                          </button>
                        )}
                      </div>
                    ))
                  )}
                </div>
                {editing && (
                  <div className="mt-3 flex gap-2">
                    <input
                      type="text"
                      value={newInScope}
                      onChange={(e) => setNewInScope(e.target.value)}
                      onKeyDown={(e) => e.key === "Enter" && (e.preventDefault(), addInScope())}
                      placeholder={t("plan.spec.inScopePlaceholder")}
                      className="flex-1 rounded-md border border-border bg-base px-2.5 py-1.5 text-xs text-fg focus:outline-none focus:border-ring"
                    />
                    <Button size="sm" onClick={addInScope}>
                      <Plus className="size-3" />
                    </Button>
                  </div>
                )}
              </div>

              {/* Out of Scope */}
              <div className="rounded-xl border border-border bg-elevated p-5 flex min-w-0 flex-col">
                <div className="flex items-center justify-between mb-3">
                  <div className="flex items-center gap-1.5 text-xs font-semibold uppercase tracking-wider text-muted">
                    <CircleSlash className="size-3.5 text-subtle" />
                    <span>{t("plan.spec.outOfScope")}</span>
                  </div>
                  <Badge variant="muted" size="xs">
                    {spec.outOfScope.length}
                  </Badge>
                </div>
                <div className="space-y-2 flex-1">
                  {spec.outOfScope.length === 0 ? (
                    <div className="text-xs text-subtle italic py-2">{t("plan.spec.outOfScopeEmpty")}</div>
                  ) : (
                    spec.outOfScope.map((item, idx) => (
                      <div
                        key={idx}
                        className="group flex items-start justify-between gap-2 rounded-lg bg-base px-3 py-2 text-[13px] text-muted border border-border/50"
                      >
                        <div className="flex flex-1 items-start gap-2 min-w-0">
                          <span className="mt-1 size-1.5 rounded-full bg-subtle shrink-0" />
                          <span className="min-w-0 whitespace-normal [overflow-wrap:anywhere] leading-relaxed">{item}</span>
                        </div>
                        {editing && (
                          <button
                            type="button"
                            onClick={() => removeOutOfScope(idx)}
                            className="text-subtle hover:text-danger opacity-0 group-hover:opacity-100 transition-opacity p-0.5"
                          >
                            <Trash2 className="size-3.5" />
                          </button>
                        )}
                      </div>
                    ))
                  )}
                </div>
                {editing && (
                  <div className="mt-3 flex gap-2">
                    <input
                      type="text"
                      value={newOutOfScope}
                      onChange={(e) => setNewOutOfScope(e.target.value)}
                      onKeyDown={(e) => e.key === "Enter" && (e.preventDefault(), addOutOfScope())}
                      placeholder={t("plan.spec.outOfScopePlaceholder")}
                      className="flex-1 rounded-md border border-border bg-base px-2.5 py-1.5 text-xs text-fg focus:outline-none focus:border-ring"
                    />
                    <Button size="sm" onClick={addOutOfScope}>
                      <Plus className="size-3" />
                    </Button>
                  </div>
                )}
              </div>
            </div>

            {/* Key Decisions */}
            <div className="rounded-xl border border-border bg-elevated p-5">
              <div className="flex items-center justify-between mb-3">
                <div className="flex items-center gap-1.5 text-xs font-semibold uppercase tracking-wider text-accent">
                  <Lightbulb className="size-3.5" />
                  <span>{t("plan.spec.decisions")}</span>
                </div>
                <Badge variant="accent" size="xs">
                  {spec.decisions.length}
                </Badge>
              </div>
              <div className="space-y-2.5">
                {spec.decisions.length === 0 ? (
                  <div className="text-xs text-subtle italic py-2">{t("plan.spec.decisionsEmpty")}</div>
                ) : (
                  spec.decisions.map((decision, idx) => (
                    <div
                      key={idx}
                      className="group flex items-start justify-between gap-3 rounded-lg bg-base px-3.5 py-2.5 text-[13px] text-fg border border-border/50"
                    >
                      <div className="flex items-start gap-2.5 min-w-0">
                        <span className="inline-flex size-5 shrink-0 items-center justify-center rounded-full bg-accent/15 text-[11px] font-bold text-accent">
                          {idx + 1}
                        </span>
                        <div className="leading-relaxed font-mono text-xs">{decision}</div>
                      </div>
                      {editing && (
                        <button
                          type="button"
                          onClick={() => removeDecision(idx)}
                          className="text-subtle hover:text-danger opacity-0 group-hover:opacity-100 transition-opacity p-0.5"
                        >
                          <Trash2 className="size-3.5" />
                        </button>
                      )}
                    </div>
                  ))
                )}
              </div>
              {editing && (
                <div className="mt-3 flex gap-2">
                  <input
                    type="text"
                    value={newDecision}
                    onChange={(e) => setNewDecision(e.target.value)}
                    onKeyDown={(e) => e.key === "Enter" && (e.preventDefault(), addDecision())}
                    placeholder={t("plan.spec.decisionsPlaceholder")}
                    className="flex-1 rounded-md border border-border bg-base px-2.5 py-1.5 text-xs text-fg focus:outline-none focus:border-ring"
                  />
                  <Button size="sm" onClick={addDecision}>
                    <Plus className="size-3" />
                  </Button>
                </div>
              )}
            </div>

            {/* Raw spec text preview if present */}
            {spec.raw && (
              <div className="rounded-xl border border-border bg-elevated p-5">
                <div className="text-xs font-semibold uppercase tracking-wider text-muted mb-2">
                  {t("plan.spec.rawNotes")}
                </div>
                <Markdown text={spec.raw} className="text-xs text-muted" />
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );
}

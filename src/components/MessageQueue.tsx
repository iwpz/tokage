import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Clock, Pencil, Trash2, Undo2, Zap } from "lucide-react";
import { sendQueuedNow } from "../lib/dispatch";
import { queueOf, useChat, type QueuedMessage } from "../stores/chat";
import { useUi } from "../stores/ui";
import { IconButton } from "./ui";

/**
 * Messages waiting behind the running turn, shown above the composer. Each can be edited in
 * place, pulled back into the composer, deleted, or sent now (interrupting the current turn).
 */
export function MessageQueue({ sessionId, onRestore }: { sessionId: string; onRestore: (text: string) => void }) {
  const { t } = useTranslation();
  const queue = useChat((s) => queueOf(s.conversations[sessionId]));
  const [error, setError] = useState<string | null>(null);
  if (queue.length === 0) return null;

  return (
    <section
      aria-label={t("workspace.queue.label")}
      className="mb-2 overflow-hidden rounded-xl border border-border bg-sidebar text-[13px]"
    >
      <header className="flex items-center gap-1.5 px-3 py-1.5 text-xs text-muted">
        <Clock aria-hidden className="size-3.5" />
        {t("workspace.queue.summary", { count: queue.length })}
      </header>
      <ol className="max-h-48 divide-y divide-border overflow-y-auto border-t border-border">
        {queue.map((q, i) => (
          <QueueItem
            key={q.id}
            sessionId={sessionId}
            item={q}
            index={i}
            onRestore={onRestore}
            onError={setError}
          />
        ))}
      </ol>
      {error && (
        <p role="alert" className="border-t border-border px-3 py-1.5 text-xs text-warning">
          {error}
        </p>
      )}
    </section>
  );
}

function QueueItem({
  sessionId,
  item,
  index,
  onRestore,
  onError,
}: {
  sessionId: string;
  item: QueuedMessage;
  index: number;
  onRestore: (text: string) => void;
  onError: (msg: string | null) => void;
}) {
  const { t } = useTranslation();
  const { updateQueued, takeQueued } = useChat();
  const openTrustPrompt = useUi((s) => s.openTrustPrompt);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(item.text);

  const sendNow = async () => {
    onError(null);
    const r = await sendQueuedNow(sessionId, item.id);
    if (r === "untrusted") {
      onError(t("settings.cli.trust.blocked"));
      openTrustPrompt();
    } else if (r === "noModel") onError(t("workspace.composer.noModels"));
  };

  if (editing) {
    const save = () => {
      const text = draft.trim();
      if (text) updateQueued(sessionId, item.id, text);
      else takeQueued(sessionId, item.id);
      setEditing(false);
    };
    return (
      <li className="px-3 py-2">
        <textarea
          autoFocus
          value={draft}
          rows={Math.min(6, Math.max(2, draft.split("\n").length))}
          aria-label={t("workspace.queue.edit")}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
              e.preventDefault();
              save();
            }
            if (e.key === "Escape") {
              e.stopPropagation();
              setDraft(item.text);
              setEditing(false);
            }
          }}
          className="block w-full resize-none rounded-md border border-border bg-elevated px-2 py-1.5 text-[13px] text-fg outline-none focus:border-ring"
        />
        <div className="mt-1.5 flex justify-end gap-1.5 text-xs">
          <button
            type="button"
            onClick={() => {
              setDraft(item.text);
              setEditing(false);
            }}
            className="h-6 rounded-md px-2 text-muted hover:bg-hover hover:text-fg"
          >
            {t("workspace.queue.cancel")}
          </button>
          <button type="button" onClick={save} className="h-6 rounded-md bg-accent px-2 text-accent-fg">
            {t("workspace.queue.save")}
          </button>
        </div>
      </li>
    );
  }

  return (
    <li className="group flex items-start gap-2 px-3 py-1.5">
      <span className="mt-0.5 w-4 shrink-0 text-right text-xs text-subtle tabular-nums">{index + 1}</span>
      <p data-selectable title={item.text} className="line-clamp-2 min-w-0 flex-1 whitespace-pre-wrap text-fg">
        {item.text}
      </p>
      <div className="flex shrink-0 items-center gap-0.5">
        <IconButton label={t("workspace.queue.sendNow")} onClick={() => void sendNow()} className="size-6 text-accent">
          <Zap className="size-3.5" />
        </IconButton>
        <IconButton label={t("workspace.queue.edit")} onClick={() => setEditing(true)} className="size-6">
          <Pencil className="size-3.5" />
        </IconButton>
        <IconButton
          label={t("workspace.queue.withdraw")}
          onClick={() => {
            const taken = takeQueued(sessionId, item.id);
            if (taken) onRestore(taken.text);
          }}
          className="size-6"
        >
          <Undo2 className="size-3.5" />
        </IconButton>
        <IconButton label={t("workspace.queue.remove")} onClick={() => takeQueued(sessionId, item.id)} className="size-6">
          <Trash2 className="size-3.5" />
        </IconButton>
      </div>
    </li>
  );
}

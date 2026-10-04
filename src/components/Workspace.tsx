import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { ArrowDown, ArrowUp, ChevronDown, ListPlus, Folder, FolderPlus, GitBranch, GitFork, MessagesSquare, Network, Square, Workflow, ScrollText } from "lucide-react";
import { cn } from "../lib/cn";
import { createSessionWorktree, pickProjectDirectory, type GitInfo, type TurnMode } from "../lib/ipc";
import { recommend, sameRef, selectableGroups, type ModelRef } from "../lib/models";
import { dispatch } from "../lib/dispatch";
import { planTurn } from "../lib/turn";
import { sessionCwd, useProjects } from "../stores/projects";
import { useProviders } from "../stores/providers";
import { isStreaming, useChat } from "../stores/chat";
import { useSettings, type RequestMode } from "../stores/settings";
import { useUi, type WorkspaceTab } from "../stores/ui";
import { useGitInfo } from "../lib/useGitInfo";
import { ProjectMenu } from "./ProjectMenu";
import { ModelPicker } from "./ModelPicker";
import { Conversation } from "./Conversation";
import { MessageQueue } from "./MessageQueue";
import { Logo } from "./Logo";
import { Button } from "./ui";

export function Workspace() {
  const { projects, activeProjectId } = useProjects();
  const project = projects.find((p) => p.id === activeProjectId) ?? null;

  return (
    project ? <ProjectWorkspace /> : <Welcome />
  );
}

function Welcome() {
  const { t } = useTranslation();
  const addProject = useProjects((s) => s.addProject);

  return (
    <div className="flex flex-1 items-center justify-center p-8">
      <div className="max-w-md text-center">
        <Logo className="mx-auto size-12" />
        <h1 className="mt-5 text-xl font-semibold">{t("workspace.welcomeTitle")}</h1>
        <p className="mt-2 text-[13px] leading-relaxed text-muted">{t("workspace.welcomeBody")}</p>
        <Button
          variant="primary"
          className="mt-6"
          onClick={async () => {
            const path = await pickProjectDirectory();
            if (path) addProject(path);
          }}
        >
          <FolderPlus className="size-4" />
          {t("sidebar.addProject")}
        </Button>
      </div>
    </div>
  );
}

const TAB_ICONS: Record<WorkspaceTab, typeof Network> = {
  chat: MessagesSquare,
  plan: Network,
  pipeline: Workflow,
  session: ScrollText,
};

function ProjectWorkspace() {
  const { t } = useTranslation();
  const activeSessionId = useProjects((s) => s.activeSessionId);
  const { workspaceTab, setWorkspaceTab } = useUi();

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {activeSessionId ? (
        <>
          <div role="tablist" aria-label="Views" className="flex gap-1 border-b border-border px-4 py-1.5">
            {(Object.keys(TAB_ICONS) as WorkspaceTab[]).map((tab) => {
              const Icon = TAB_ICONS[tab];
              return (
                <button
                  key={tab}
                  type="button"
                  role="tab"
                  aria-selected={workspaceTab === tab}
                  onClick={() => setWorkspaceTab(tab)}
                  className={cn(
                    "inline-flex h-7 items-center gap-1.5 rounded-md px-2.5 text-[13px]",
                    workspaceTab === tab ? "bg-active text-fg" : "text-muted hover:bg-hover hover:text-fg",
                  )}
                >
                  <Icon className="size-3.5" />
                  {t(`workspace.tabs.${tab}`)}
                </button>
              );
            })}
          </div>
          {workspaceTab === "chat" ? (
            <Conversation sessionId={activeSessionId} />
          ) : (
            <div role="tabpanel" className="flex flex-1 items-center justify-center p-8 text-[13px] text-subtle">
              {t(`workspace.placeholder.${workspaceTab}`)}
            </div>
          )}
        </>
      ) : (
        <div className="flex flex-1 items-center justify-center p-8">
          <div className="max-w-lg text-center">
            <h1 className="text-2xl font-semibold">
              {t("workspace.emptyTitleBefore")}
              <ProjectSwitcher />
              {t("workspace.emptyTitleAfter")}
            </h1>
            <p className="mt-2 text-[13px] leading-relaxed text-muted">{t("workspace.emptyBody")}</p>
          </div>
        </div>
      )}
      <Composer />
    </div>
  );
}

/** Underlined project name in the empty-state title; click to switch or add projects. */
function ProjectSwitcher() {
  const project = useProjects((s) => s.projects.find((p) => p.id === s.activeProjectId));
  if (!project) return null;
  return (
    <ProjectMenu
      trigger={project.name}
      triggerClassName="rounded underline decoration-subtle decoration-2 underline-offset-4 hover:decoration-fg focus-visible:outline-2 focus-visible:outline-ring"
    />
  );
}

/** Project picker, git status and the worktree option, shown under the composer. */
function ComposerContext({ worktree, onWorktree }: { worktree: boolean; onWorktree: (v: boolean) => void }) {
  const { t } = useTranslation();
  const project = useProjects((s) => s.projects.find((p) => p.id === s.activeProjectId));
  const activeSessionId = useProjects((s) => s.activeSessionId);
  const session = project?.sessions.find((x) => x.id === activeSessionId);
  // Refresh after each turn: the agent may have edited files or switched branches.
  const turns = useChat((s) => (activeSessionId ? s.conversations[activeSessionId]?.messages.length : 0));
  const busy = useChat((s) => (activeSessionId ? isStreaming(s.conversations[activeSessionId]) : false));
  const cwd = project ? sessionCwd(project, activeSessionId) : undefined;
  const git = useGitInfo(cwd, `${turns}:${busy}`);
  if (!project) return null;

  // An existing session keeps the folder it started in; the checkbox applies to new ones.
  const locked = !!session;
  const checked = session ? !!session.worktree : worktree;
  const canWorktree = !!git?.isRepo && git.hasCommits;

  return (
    <div className="mt-2 flex flex-wrap items-center gap-x-1 gap-y-1 px-1 text-xs text-muted">
      <ProjectMenu
        placement="top"
        trigger={
          <>
            <Folder aria-hidden className="size-3.5" />
            <span className="max-w-48 truncate">{project.name}</span>
            <ChevronDown aria-hidden className="size-3" />
          </>
        }
        triggerClassName="inline-flex h-6 items-center gap-1.5 rounded-md px-1.5 hover:bg-hover hover:text-fg"
      />
      {git?.isRepo && <GitStatus git={git} />}
      {git && !git.isRepo && <span className="px-1.5 text-subtle">{t("workspace.git.notRepo")}</span>}
      {git?.isRepo && (
        <label
          title={
            locked
              ? session?.worktree
                ? t("workspace.git.worktreeAt", { path: session.worktree.root })
                : t("workspace.git.worktreeLocked")
              : canWorktree
                ? t("workspace.git.worktreeHint")
                : t("workspace.git.worktreeNoCommits")
          }
          className={cn(
            "ml-auto inline-flex h-6 items-center gap-1.5 rounded-md px-1.5",
            locked || !canWorktree ? "text-subtle" : "cursor-pointer hover:bg-hover hover:text-fg",
          )}
        >
          <input
            type="checkbox"
            checked={checked && (locked || canWorktree)}
            disabled={locked || !canWorktree}
            onChange={(e) => onWorktree(e.target.checked)}
            className="size-3.5 accent-[var(--accent)]"
          />
          <GitFork aria-hidden className="size-3.5" />
          {t("workspace.git.worktree")}
        </label>
      )}
    </div>
  );
}

function GitStatus({ git }: { git: GitInfo }) {
  const { t } = useTranslation();
  const detail = [
    git.staged ? t("workspace.git.staged", { count: git.staged }) : null,
    git.unstaged ? t("workspace.git.unstaged", { count: git.unstaged }) : null,
    git.untracked ? t("workspace.git.untracked", { count: git.untracked }) : null,
    git.conflicted ? t("workspace.git.conflicted", { count: git.conflicted }) : null,
  ]
    .filter(Boolean)
    .join(" · ");
  return (
    <>
      <span
        className="inline-flex h-6 min-w-0 items-center gap-1.5 px-1.5"
        title={[
          git.detached ? t("workspace.git.detached") : t("workspace.git.branch"),
          git.upstream ? t("workspace.git.tracking", { upstream: git.upstream }) : t("workspace.git.noUpstream"),
          git.linkedWorktree ? t("workspace.git.inWorktree") : null,
        ]
          .filter(Boolean)
          .join("\n")}
      >
        {git.linkedWorktree ? <GitFork aria-hidden className="size-3.5 shrink-0" /> : <GitBranch aria-hidden className="size-3.5 shrink-0" />}
        <span className="max-w-48 truncate font-mono">{git.branch ?? t("workspace.git.noBranch")}</span>
        {git.detached && <span className="text-warning">{t("workspace.git.detachedShort")}</span>}
        {!git.hasCommits && <span className="text-subtle">{t("workspace.git.noCommits")}</span>}
        {(git.ahead > 0 || git.behind > 0) && (
          <span className="inline-flex items-center gap-0.5 tabular-nums" aria-label={t("workspace.git.aheadBehind", { ahead: git.ahead, behind: git.behind })}>
            {git.ahead > 0 && (
              <>
                <ArrowUp aria-hidden className="size-3" />
                {git.ahead}
              </>
            )}
            {git.behind > 0 && (
              <>
                <ArrowDown aria-hidden className="size-3" />
                {git.behind}
              </>
            )}
          </span>
        )}
      </span>
      {git.changes > 0 ? (
        <span className="inline-flex h-6 items-center gap-1.5 px-1.5" title={detail}>
          <span className="text-warning">{t("workspace.git.changes", { count: git.changes })}</span>
          {(git.insertions > 0 || git.deletions > 0) && (
            <span className="font-mono tabular-nums">
              <span className="text-success">+{git.insertions}</span> <span className="text-danger">−{git.deletions}</span>
            </span>
          )}
        </span>
      ) : (
        git.hasCommits && <span className="px-1.5 text-subtle">{t("workspace.git.clean")}</span>
      )}
    </>
  );
}

const MODES: RequestMode[] = ["auto", "plan", "direct"];

function Composer() {
  const { t } = useTranslation();
  const defaultMode = useSettings((s) => (s.rememberMode ? s.lastMode : s.requestMode));
  const setSetting = useSettings((s) => s.set);
  const enabled = useSettings((s) => s.providerEnabled);
  const trust = useSettings((s) => s.providerTrust);
  const providers = useProviders((s) => s.providers);
  const project = useProjects((s) => s.projects.find((p) => p.id === s.activeProjectId));
  const activeSessionId = useProjects((s) => s.activeSessionId);
  const createSession = useProjects((s) => s.createSession);
  const { picks, setPick, cancel, enqueue, modelOptions, setModelOptions } = useChat();
  const busy = useChat((s) => (activeSessionId ? isStreaming(s.conversations[activeSessionId]) : false));
  const setWorkspaceTab = useUi((s) => s.setWorkspaceTab);
  const openTrustPrompt = useUi((s) => s.openTrustPrompt);
  const [mode, setModeState] = useState<RequestMode>(defaultMode);
  // Always recorded, so turning "remember" on later starts from the latest choice.
  const setMode = (m: RequestMode) => {
    setModeState(m);
    setSetting("lastMode", m);
  };
  const [text, setText] = useState("");
  const [error, setError] = useState<string | null>(null);
  const worktree = useSettings((s) => s.sessionWorktree);
  const [starting, setStarting] = useState(false);

  const groups = useMemo(() => selectableGroups(providers, enabled, trust), [providers, enabled, trust]);
  const recommended = useMemo(
    () => ({ plan: recommend(groups, "plan"), direct: recommend(groups, "direct") }),
    [groups],
  );
  // A manual pick only sticks while its provider and model are still selectable.
  const valid = (ref: ModelRef | null) =>
    ref && groups.some((g) => g.provider.id === ref.provider && g.models.some((m) => m.key === ref.model)) ? ref : null;
  const pickFor = (m: TurnMode) => valid(picks[m]) ?? recommended[m];

  const submit = async () => {
    const prompt = text.trim();
    if (!prompt || !project || starting) return;
    if (project.trusted !== true) {
      setError(t("settings.cli.trust.blocked"));
      openTrustPrompt();
      return;
    }
    // While a turn runs, new messages line up behind it.
    if (activeSessionId && busy) {
      enqueue(activeSessionId, prompt, mode);
      setText("");
      setError(null);
      return;
    }
    if (!planTurn(prompt, mode)) {
      setError(t("workspace.composer.noModels"));
      return;
    }
    setError(null);
    let sessionId = activeSessionId;
    if (!sessionId) {
      const title = prompt.split("\n")[0].slice(0, 60);
      let wt;
      if (worktree) {
        // Branch names are short; uniqueness comes from the time-based suffix.
        setStarting(true);
        try {
          wt = await createSessionWorktree(project.path, `s-${Date.now().toString(36)}`);
        } catch (e) {
          setError(t("workspace.git.worktreeFailed", { error: String(e) }));
          return;
        } finally {
          setStarting(false);
        }
      }
      sessionId = createSession(project.id, title, wt);
    }
    setWorkspaceTab("chat");
    setText("");
    const result = dispatch(sessionId, prompt, mode);
    if (result === "noModel") setError(t("workspace.composer.noModels"));
  };

  const pickerMode: TurnMode | null = mode === "auto" ? null : mode;

  return (
    <div className="mx-auto w-full max-w-3xl px-3 pb-4 sm:px-6 sm:pb-6">
      {error && (
        <p role="alert" className="mb-2 text-center text-xs text-warning">
          {error}
        </p>
      )}
      {activeSessionId && <MessageQueue sessionId={activeSessionId} onRestore={(v) => setText((cur) => (cur.trim() ? `${cur.trimEnd()}\n\n${v}` : v))} />}
      <div className="rounded-2xl border border-border bg-elevated shadow-sm focus-within:border-ring">
        <textarea
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
              e.preventDefault();
              void submit();
            }
          }}
          rows={3}
          aria-label={busy ? t("workspace.composer.placeholderBusy") : t("workspace.composer.placeholder")}
          aria-describedby="composer-mode-desc"
          placeholder={busy ? t("workspace.composer.placeholderBusy") : t("workspace.composer.placeholder")}
          className="block w-full resize-none bg-transparent px-4 pt-3 text-[14px] text-fg placeholder:text-subtle focus:outline-none"
        />
        <div className="flex items-center justify-between gap-2 px-3 pb-3">
          <div role="radiogroup" aria-label={t("workspace.composer.mode")} className="flex shrink-0 gap-0.5 rounded-lg bg-hover p-0.5">
            {MODES.map((m) => (
              <button
                key={m}
                type="button"
                role="radio"
                aria-checked={mode === m}
                onClick={() => setMode(m)}
                className={cn(
                  "h-6 rounded-md px-2.5 text-xs",
                  mode === m ? "bg-elevated text-fg shadow-sm" : "text-muted hover:text-fg",
                )}
              >
                {t(`workspace.composer.modes.${m}`)}
              </button>
            ))}
          </div>
          <div className="flex min-w-0 items-center gap-1">
            <ModelPicker
              label={t(`workspace.composer.picker.label.${pickerMode ?? "auto"}`)}
              groups={groups}
              disabled={pickerMode === null}
              value={pickerMode ? pickFor(pickerMode) : null}
              recommended={pickerMode ? recommended[pickerMode] : null}
              overridden={pickerMode ? !!valid(picks[pickerMode]) && !sameRef(picks[pickerMode], recommended[pickerMode]) : false}
              onChange={(ref) => pickerMode && setPick(pickerMode, sameRef(ref, recommended[pickerMode]) ? null : ref)}
              onReset={() => pickerMode && setPick(pickerMode, null)}
              options={modelOptions}
              onOptionsChange={setModelOptions}
            />
            {busy && (
              <button
                type="button"
                aria-label={t("workspace.composer.stop")}
                title={t("workspace.composer.stop")}
                onClick={() => activeSessionId && void cancel(activeSessionId)}
                className="inline-flex size-8 shrink-0 items-center justify-center rounded-full bg-fg text-bg"
              >
                <Square className="size-3 fill-current" />
              </button>
            )}
            {(!busy || text.trim()) && (
              <button
                type="button"
                aria-label={busy ? t("workspace.composer.queue") : t("workspace.composer.send")}
                title={busy ? t("workspace.composer.queueHint") : t("workspace.composer.hint")}
                disabled={!text.trim() || starting}
                onClick={() => void submit()}
                className="inline-flex size-8 shrink-0 items-center justify-center rounded-full bg-accent text-accent-fg disabled:opacity-40"
              >
                {busy ? <ListPlus className="size-4" /> : <ArrowUp className="size-4" />}
              </button>
            )}
          </div>
        </div>
        <p id="composer-mode-desc" className="border-t border-border px-4 py-2 text-xs leading-relaxed text-muted">
          {t(`workspace.composer.modeDesc.${mode}`)}
        </p>
      </div>
      <ComposerContext worktree={worktree} onWorktree={(v) => setSetting("sessionWorktree", v)} />
    </div>
  );
}

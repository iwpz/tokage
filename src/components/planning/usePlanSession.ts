import { useCallback, useEffect, useState } from "react";
import {
  allocatePlanModels,
  getRepoDigest,
  loadPlan,
  parsePlanningOutput,
  persistPlan,
  persistPlanUserEdited,
  renderPlanRepairPrompt,
  resetPlanToDefault,
  validatePlan,
  type Plan,
  type PlanAllocationResult,
  type PlanContract,
  type PlanSpec,
  type TaskItem,
  type ValidationResult,
} from "../../lib/ipc";
import { fixedModelOverrides } from "../../lib/fixedModel";
import { currentAvailableModels, pickCheapest, selectableGroups } from "../../lib/models";
import { resolveValidDomains } from "../../lib/planningDomains";
import { messagesOf, useChat } from "../../stores/chat";
import { sessionCwd, useProjects } from "../../stores/projects";
import { useProviders } from "../../stores/providers";
import { useSettings } from "../../stores/settings";
import i18n from "../../i18n";

async function allowedCommandsFor(cwd: string | null): Promise<string[] | undefined> {
  if (!cwd) return undefined;
  try {
    const digest = await getRepoDigest(cwd);
    const c = digest.commands;
    const all = [...c.build, ...c.test, ...c.check, ...c.lint];
    return [...new Set(all.filter(Boolean))];
  } catch {
    return undefined;
  }
}

function validateOpts(allowedCommands: string[] | undefined) {
  return {
    allowedCommands,
    validDomains: resolveValidDomains(),
  };
}

export function usePlanSession(sessionId: string | null) {
  const project = useProjects((s) => s.projects.find((p) => p.sessions.some((x) => x.id === sessionId)));
  const cwd = project && sessionId ? sessionCwd(project, sessionId) : null;
  const messages = useChat((s) =>
    messagesOf(sessionId ? s.conversations[sessionId] : undefined),
  );

  const [plan, setPlan] = useState<Plan | null>(null);
  const [allocation, setAllocation] = useState<PlanAllocationResult | null>(null);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [repairing, setRepairing] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [validation, setValidation] = useState<ValidationResult | null>(null);

  // Attempt to load persisted plan or extract from assistant message
  const reload = useCallback(async () => {
    if (!sessionId || !cwd) {
      setPlan(null);
      setValidation(null);
      return;
    }

    const validateSoon = (target: Plan) => {
      void (async () => {
        try {
          const allowedCommands = await allowedCommandsFor(cwd);
          const val = await validatePlan({ plan: target, ...validateOpts(allowedCommands) });
          setValidation(val);
        } catch (e) {
          console.warn("Initial plan validation failed:", e);
        }
      })();
    };

    setLoading(true);
    setError(null);
    try {
      // 1. Try loading structured plan from .tokage/plans/<sessionId>/
      const loaded = await loadPlan(cwd, sessionId);
      if (loaded && (loaded.spec.goal || loaded.tasks.length > 0 || loaded.contracts.length > 0)) {
        setPlan(loaded);
        setDirty(false);
        setLoading(false);
        validateSoon(loaded);
        return;
      }
    } catch {
      // File may not exist yet, fallback to parsing message
    }

    // 2. Fallback: Parse assistant plan message if user conducted a Plan turn
    // Check for messages specifically produced in "plan" mode or containing explicit planning headers
    const planMsg = [...messages].reverse().find(
      (m) =>
        m.role === "assistant" &&
        m.text.trim() &&
        (m.mode === "plan" || (m.text.includes("# Spec") && m.text.includes("# Tasks"))),
    );

    if (planMsg) {
      try {
        const parsed = await parsePlanningOutput({ markdown: planMsg.text });
        if (parsed && (parsed.spec.goal || parsed.tasks.length > 0 || parsed.contracts.length > 0)) {
          setPlan(parsed);
          setDirty(false);
          setLoading(false);
          validateSoon(parsed);
          return;
        }
      } catch (e) {
        // Only report parsing error if it genuinely was a plan turn
        if (planMsg.mode === "plan") {
          setError(i18n.t("plan.errors.parseFailed", { error: String(e) }));
        }
      }
    }

    // 3. Fallback: Default draft plan (no plan generated yet in this session)
    setPlan({
      spec: {
        raw: "",
        goal: "",
        inScope: [],
        outOfScope: [],
        decisions: [],
      },
      contracts: [],
      tasks: [],
      notes: "",
      rawMarkdown: "",
    });
    setValidation(null);
    setDirty(false);
    setLoading(false);
  }, [sessionId, cwd, messages]);

  useEffect(() => {
    void reload();
  }, [sessionId, cwd]);

  const updateSpec = useCallback((patch: Partial<PlanSpec>) => {
    setPlan((prev) => {
      if (!prev) return prev;
      setDirty(true);
      return {
        ...prev,
        spec: { ...prev.spec, ...patch },
      };
    });
  }, []);

  const updateContract = useCallback((id: string, patch: Partial<PlanContract>) => {
    setPlan((prev) => {
      if (!prev) return prev;
      setDirty(true);
      return {
        ...prev,
        contracts: prev.contracts.map((c) => (c.id === id ? { ...c, ...patch } : c)),
      };
    });
  }, []);

  const addContract = useCallback((contract: PlanContract) => {
    setPlan((prev) => {
      if (!prev) return prev;
      setDirty(true);
      return {
        ...prev,
        contracts: [...prev.contracts, contract],
      };
    });
  }, []);

  const removeContract = useCallback((id: string) => {
    setPlan((prev) => {
      if (!prev) return prev;
      setDirty(true);
      return {
        ...prev,
        contracts: prev.contracts.filter((c) => c.id !== id),
      };
    });
  }, []);

  const updateTask = useCallback((id: string, patch: Partial<TaskItem>) => {
    setPlan((prev) => {
      if (!prev) return prev;
      setDirty(true);
      return {
        ...prev,
        tasks: prev.tasks.map((t) => (t.id === id ? { ...t, ...patch } : t)),
      };
    });
  }, []);

  const addTask = useCallback((task: TaskItem) => {
    setPlan((prev) => {
      if (!prev) return prev;
      setDirty(true);
      return {
        ...prev,
        tasks: [...prev.tasks, task],
      };
    });
  }, []);

  const removeTask = useCallback((id: string) => {
    setPlan((prev) => {
      if (!prev) return prev;
      setDirty(true);
      return {
        ...prev,
        tasks: prev.tasks.filter((t) => t.id !== id),
      };
    });
  }, []);

  const validate = useCallback(async () => {
    if (!plan) return null;
    try {
      const allowedCommands = await allowedCommandsFor(cwd);
      const val = await validatePlan({ plan, ...validateOpts(allowedCommands) });
      setValidation(val);
      return val;
    } catch (e) {
      setError(i18n.t("plan.errors.validateFailed", { error: String(e) }));
      return null;
    }
  }, [plan, cwd]);

  const save = useCallback(async (): Promise<Plan | null> => {
    if (!sessionId || !cwd || !plan) return null;
    setSaving(true);
    setError(null);
    try {
      const allowedCommands = await allowedCommandsFor(cwd);
      const val = await validatePlan({ plan, ...validateOpts(allowedCommands) });
      setValidation(val);
      await persistPlan(cwd, sessionId, val.plan);
      const saved = { ...val.plan, isUserEdited: false };
      setPlan(saved);
      setDirty(false);
      return saved;
    } catch (e) {
      setError(i18n.t("plan.errors.saveFailed", { error: String(e) }));
      return null;
    } finally {
      setSaving(false);
    }
  }, [sessionId, cwd, plan]);

  // Saves to user_edited directory immediately and updates plan state
  const saveUserEdited = useCallback(
    async (updatedPlan?: Plan) => {
      const target = updatedPlan ?? plan;
      if (!sessionId || !cwd || !target) return;
      try {
        const allowedCommands = await allowedCommandsFor(cwd);
        const val = await validatePlan({ plan: target, ...validateOpts(allowedCommands) });
        setValidation(val);
        await persistPlanUserEdited(cwd, sessionId, val.plan);
        setPlan({ ...val.plan, isUserEdited: true });
        setDirty(false);
      } catch (e) {
        console.warn("Failed to auto-save user_edited plan:", e);
      }
    },
    [sessionId, cwd, plan]
  );

  // Recalculates model allocation whenever plan tasks change or user manually overrides
  const runAllocation = useCallback(
    async (manualOverrides?: Record<string, string>) => {
      if (!plan || plan.tasks.length === 0) {
        setAllocation(null);
        return;
      }
      try {
        // Single-model mode: force every task onto the session's locked model.
        const locked = sessionId
          ? useChat.getState().conversations[sessionId]?.lockedModel
          : null;
        let overrides: Record<string, string>;
        if (locked) {
          // Locked model always wins over per-task manual picks.
          overrides = fixedModelOverrides(plan, locked);
        } else {
          overrides = { ...(manualOverrides ?? {}) };
          // If some tasks already have assigned_model in the plan, honor them unless explicitly cleared in overrides
          for (const t of plan.tasks) {
            if (t.assigned_model && !(t.id in overrides)) {
              overrides[t.id] = t.assigned_model;
            }
          }
        }
        const res = await allocatePlanModels(plan, overrides, undefined, currentAvailableModels());
        setAllocation(res);
      } catch (e) {
        console.warn("Auto-allocation failed:", e);
      }
    },
    [plan, sessionId]
  );

  // Resets to initial default plan, deleting user_edited version
  const resetToDefault = useCallback(async () => {
    if (!sessionId || !cwd) return;
    setLoading(true);
    setError(null);
    try {
      const restored = await resetPlanToDefault(cwd, sessionId);
      setPlan({ ...restored, isUserEdited: false });
      setDirty(false);
      // Re-run allocation with restored plan (which has no manual overrides)
      void runAllocation();
    } catch (e) {
      setError(i18n.t("plan.errors.resetFailed", { error: String(e) }));
    } finally {
      setLoading(false);
    }
  }, [sessionId, cwd, runAllocation]);

  useEffect(() => {
    if (plan && plan.tasks.length > 0) {
      void runAllocation();
    } else {
      setAllocation(null);
    }
  }, [plan?.tasks.length, plan?.version]);

  const repairWithModel = useCallback(async () => {
    if (!sessionId || !cwd) return;
    const planMsg = [...messages].reverse().find(
      (m) =>
        m.role === "assistant" &&
        m.text.trim() &&
        (m.mode === "plan" || (m.text.includes("# Spec") && m.text.includes("# Tasks")) || (m as any).plan !== undefined)
    );
    if (!planMsg) {
      setError(i18n.t("plan.errors.noPlanReply"));
      return;
    }

    setRepairing(true);
    setError(null);
    try {
      // F16.b: repair prompt from PLAN_FORMAT_SPEC via IPC (no duplicated format text).
      const repairPrompt = await renderPlanRepairPrompt(planMsg.text);

      const settings = useSettings.getState();
      const groups = selectableGroups(
        useProviders.getState().providers,
        settings.providerEnabled,
        settings.providerTrust,
        "plan",
        settings.blockedModels,
        settings.modelBanningLevel,
        settings.modelProviderPreferences,
        settings.modelChannelAccess,
      );
      const cheapest = pickCheapest(groups);
      if (!cheapest) {
        throw new Error(i18n.t("plan.errors.noRepairModel"));
      }

      const devShadow = settings.devShadowModel;
      let providerId = cheapest.ref.provider;
      let modelId = cheapest.ref.model;
      let modelName = `${cheapest.name}${i18n.t("plan.errors.repairModelSuffix")}`;
      if (devShadow === "luna") {
        providerId = "CODEX_CLI";
        modelId = "gpt-6-luna";
        modelName = i18n.t("plan.errors.repairLuna");
      } else if (devShadow === "deepseek") {
        // Keep the selected provider channel; only override the model id for shadow routing.
        modelId = "deepseek/deepseek-v4-pro";
        modelName = i18n.t("plan.errors.repairDeepseek");
      }

      await useChat.getState().send({
        sessionId,
        cwd,
        text: repairPrompt,
        mode: "plan",
        auto: false,
        ref: { provider: providerId, model: modelId },
        variant: { id: modelId, tier: "FAST", effort: null, fast: true },
        modelName,
        framePlanningPrompt: false,
      });
    } catch (e) {
      setError(i18n.t("plan.errors.repairFailed", { error: String(e) }));
    } finally {
      setRepairing(false);
    }
  }, [sessionId, cwd, messages]);

  return {
    plan,
    setPlan,
    allocation,
    runAllocation,
    loading,
    saving,
    repairing,
    dirty,
    error,
    validation,
    updateSpec,
    updateContract,
    addContract,
    removeContract,
    updateTask,
    addTask,
    removeTask,
    validate,
    save,
    saveUserEdited,
    resetToDefault,
    reload,
    repairWithModel,
    cwd,
  };
}

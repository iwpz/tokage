import { useCallback, useEffect, useState } from "react";
import {
  allocatePlanModels,
  loadPlan,
  parsePlanningOutput,
  persistPlan,
  persistPlanUserEdited,
  resetPlanToDefault,
  validatePlan,
  type Plan,
  type PlanAllocationResult,
  type PlanContract,
  type PlanSpec,
  type TaskItem,
  type ValidationResult,
} from "../../lib/ipc";
import { useChat } from "../../stores/chat";
import { sessionCwd, useProjects } from "../../stores/projects";
import { useProviders } from "../../stores/providers";
import { useSettings } from "../../stores/settings";

export function usePlanSession(sessionId: string | null) {
  const project = useProjects((s) => s.projects.find((p) => p.sessions.some((x) => x.id === sessionId)));
  const cwd = project && sessionId ? sessionCwd(project, sessionId) : null;
  const messages = useChat((s) => (sessionId ? s.conversations[sessionId]?.messages ?? [] : []));

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
      return;
    }
    setLoading(true);
    setError(null);
    try {
      // 1. Try loading structured plan from .tokage/plans/<sessionId>/
      const loaded = await loadPlan(cwd, sessionId);
      if (loaded && (loaded.spec.goal || loaded.tasks.length > 0 || loaded.contracts.length > 0)) {
        setPlan(loaded);
        setDirty(false);
        setLoading(false);
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
          return;
        }
      } catch (e) {
        // Only report parsing error if it genuinely was a plan turn
        if (planMsg.mode === "plan") {
          setError(`解析规划输出失败: ${String(e)}`);
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
      const val = await validatePlan({ plan });
      setValidation(val);
      return val;
    } catch (e) {
      setError(`校验计划失败: ${String(e)}`);
      return null;
    }
  }, [plan]);

  const save = useCallback(async () => {
    if (!sessionId || !cwd || !plan) return;
    setSaving(true);
    setError(null);
    try {
      const val = await validatePlan({ plan });
      setValidation(val);
      await persistPlan(cwd, sessionId, val.plan);
      setPlan({ ...val.plan, isUserEdited: false });
      setDirty(false);
    } catch (e) {
      setError(`保存计划失败: ${String(e)}`);
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
        const val = await validatePlan({ plan: target });
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
      setError(`恢复默认计划失败: ${String(e)}`);
    } finally {
      setLoading(false);
    }
  }, [sessionId, cwd]);

  // Recalculates model allocation whenever plan tasks change or user manually overrides
  const runAllocation = useCallback(
    async (manualOverrides?: Record<string, string>) => {
      if (!plan || plan.tasks.length === 0) {
        setAllocation(null);
        return;
      }
      try {
        const overrides = { ...(manualOverrides ?? {}) };
        // If some tasks already have assigned_model in the plan, honor them unless explicitly cleared in overrides
        for (const t of plan.tasks) {
          if (t.assigned_model && !(t.id in overrides)) {
            overrides[t.id] = t.assigned_model;
          }
        }
        const res = await allocatePlanModels(plan, overrides);
        setAllocation(res);
      } catch (e) {
        console.warn("Auto-allocation failed:", e);
      }
    },
    [plan]
  );

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
      setError("当前会话未检测到助手的规划回复内容。");
      return;
    }

    setRepairing(true);
    setError(null);
    try {
      const repairPrompt = `请将以下技术规划内容严格转换为符合 Tokage 规约的 Markdown 格式：\n必须包含以下四个一级段落，顺序固定，且任务图必须使用 yaml 代码块：\n\n# Spec\n## 目标\n一到三句话。\n\n## 范围\n- 包含：...\n- 不包含：...\n\n## 关键决策\n- 决策：理由\n\n# Contracts\n## C1 接口契约\n\`\`\`ts\n// 只写签名与类型，不写实现\n\`\`\`\n\n# Tasks\n\`\`\`yaml\n- id: T1\n  domain: backend-api\n  title: 任务标题\n  goal: 任务目标\n  depends_on: []\n  contracts: [C1]\n  write_scope: [\"src/**\"]\n  deliverables: [\"交付物\"]\n  acceptance:\n    commands: [\"cargo test\"]\n    criteria: [\"自查标准\"]\n  size: M\n  difficulty: 2\n\`\`\`\n\n# Notes\n补充约束。\n\n规则：\n1. 严禁使用 Markdown 表格描述任务，必须使用上面的 yaml 代码块。\n2. 保留原计划中的所有任务拆分、依赖、目标和文件路径。\n\n原始内容如下：\n\n${planMsg.text}`;

      const providers = useProviders.getState().providers.filter((p) => p.status === "READY" || p.status === "UNSUPPORTED_VERSION");
      const provider = providers[0];
      if (!provider) {
        throw new Error("无可用 CLI 模型提供商进行格式修复。");
      }

      const devShadow = useSettings.getState().devShadowModel;
      let modelId = provider.models[0]?.id ?? "default";
      let modelName = provider.models[0]?.name ?? "Default";
      if (devShadow === "luna") {
        modelId = "gpt-6-luna";
        modelName = "GPT-6-Luna (替身修复)";
      } else if (devShadow === "deepseek") {
        modelId = "deepseek/deepseek-v4-pro";
        modelName = "DeepSeek-V4-Pro (修复)";
      }

      await useChat.getState().send({
        sessionId,
        cwd,
        text: repairPrompt,
        mode: "plan",
        auto: false,
        ref: { provider: provider.id, model: modelId },
        variant: { id: modelId, tier: "FAST", effort: null, fast: true },
        modelName,
      });
    } catch (e) {
      setError(`调用便宜模型修复格式失败: ${String(e)}`);
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

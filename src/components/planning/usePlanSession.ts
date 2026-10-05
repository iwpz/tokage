import { useCallback, useEffect, useState } from "react";
import {
  loadPlan,
  parsePlanningOutput,
  persistPlan,
  validatePlan,
  type Plan,
  type PlanContract,
  type PlanSpec,
  type TaskItem,
  type ValidationResult,
} from "../../lib/ipc";
import { useChat } from "../../stores/chat";
import { sessionCwd, useProjects } from "../../stores/projects";

export function usePlanSession(sessionId: string | null) {
  const project = useProjects((s) => s.projects.find((p) => p.sessions.some((x) => x.id === sessionId)));
  const cwd = project && sessionId ? sessionCwd(project, sessionId) : null;
  const messages = useChat((s) => (sessionId ? s.conversations[sessionId]?.messages ?? [] : []));

  const [plan, setPlan] = useState<Plan | null>(null);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
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

    // 2. Fallback: Parse latest assistant plan message in conversation
    const planMsg = [...messages].reverse().find((m) => m.role === "assistant" && m.text.trim());
    if (planMsg) {
      try {
        const parsed = await parsePlanningOutput({ markdown: planMsg.text });
        setPlan(parsed);
        setDirty(false);
        setLoading(false);
        return;
      } catch (e) {
        setError(`解析规划输出失败: ${String(e)}`);
      }
    }

    // 3. Fallback: Default draft plan
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
      setPlan(val.plan);
      setDirty(false);
    } catch (e) {
      setError(`保存计划失败: ${String(e)}`);
    } finally {
      setSaving(false);
    }
  }, [sessionId, cwd, plan]);

  return {
    plan,
    setPlan,
    loading,
    saving,
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
    reload,
    cwd,
  };
}

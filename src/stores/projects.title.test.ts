// @vitest-environment happy-dom
import { beforeEach, describe, it, expect } from "vitest";
import { useProjects } from "./projects";

describe("Projects session title auto-naming and user rename override", () => {
  beforeEach(() => {
    useProjects.setState({
      projects: [
        {
          id: "p1",
          name: "TestProject",
          path: "/workspace/p1",
          expanded: true,
          sessions: [
            {
              id: "s1",
              title: "长提示词第一行截断作为初始临时标题文本样例...",
              state: "draft",
              updatedAt: Date.now(),
            },
          ],
        },
      ],
      activeProjectId: "p1",
      activeSessionId: "s1",
    });
  });

  it("automatically replaces session title with planning model's derived title when user hasn't renamed", () => {
    const store = useProjects.getState();
    expect(store.projects[0].sessions[0].title).toContain("长提示词");
    expect(store.projects[0].sessions[0].userRenamed).toBeFalsy();

    store.autoRenameSession("s1", "重构文本选择交互");

    const updated = useProjects.getState().projects[0].sessions[0];
    expect(updated.title).toBe("重构文本选择交互");
    expect(updated.userRenamed).toBeFalsy();
  });

  it("prevents autoRenameSession from overriding a title manually renamed by the user", () => {
    const store = useProjects.getState();
    // User manually renames the session
    store.renameSession("s1", "用户自定义标题", true);

    const userRenamedSession = useProjects.getState().projects[0].sessions[0];
    expect(userRenamedSession.title).toBe("用户自定义标题");
    expect(userRenamedSession.userRenamed).toBe(true);

    // Planning model tries to automatically rename it
    store.autoRenameSession("s1", "规划模型新生成的标题");

    const afterAttempt = useProjects.getState().projects[0].sessions[0];
    // Must remain user's chosen title!
    expect(afterAttempt.title).toBe("用户自定义标题");
  });

  it("allows user to manually rename and override the planning model's generated title at any time", () => {
    const store = useProjects.getState();
    // 1. Model auto-renames
    store.autoRenameSession("s1", "规划模型生成的标题");
    expect(useProjects.getState().projects[0].sessions[0].title).toBe("规划模型生成的标题");

    // 2. User later manually renames it
    store.renameSession("s1", "用户最终决定重命名的标题", true);
    expect(useProjects.getState().projects[0].sessions[0].title).toBe("用户最终决定重命名的标题");
    expect(useProjects.getState().projects[0].sessions[0].userRenamed).toBe(true);

    // 3. Subsequent auto-rename attempts are blocked
    store.autoRenameSession("s1", "后续重规划产生的其他标题");
    expect(useProjects.getState().projects[0].sessions[0].title).toBe("用户最终决定重命名的标题");
  });
});

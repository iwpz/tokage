import { useEffect, useState } from "react";
import { gitInfo, type GitInfo } from "./ipc";

/** Git summary for `path`; re-read when the window regains focus or `refresh` changes. */
export function useGitInfo(path: string | undefined, refresh: unknown = null): GitInfo | null {
  const [info, setInfo] = useState<GitInfo | null>(null);

  useEffect(() => {
    setInfo(null);
  }, [path]);

  useEffect(() => {
    if (!path) return;
    let cancelled = false;
    const load = () =>
      gitInfo(path)
        .then((i) => !cancelled && setInfo(i))
        .catch(() => !cancelled && setInfo(null));
    void load();
    window.addEventListener("focus", load);
    return () => {
      cancelled = true;
      window.removeEventListener("focus", load);
    };
  }, [path, refresh]);

  return info;
}

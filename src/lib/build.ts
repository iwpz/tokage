import { useEffect, useState } from "react";
import { getBuildInfo, type BuildInfo } from "./ipc";

// Fixed for the life of the process; fetched once.
let cached: Promise<BuildInfo> | null = null;

export function useBuildInfo(): BuildInfo | null {
  const [info, setInfo] = useState<BuildInfo | null>(null);
  useEffect(() => {
    cached ??= getBuildInfo();
    let live = true;
    void cached.then((i) => live && setInfo(i));
    return () => {
      live = false;
    };
  }, []);
  return info;
}

/** Link to a file in the exact source this build came from (tag for releases, else commit). */
export function sourceFileUrl(info: BuildInfo, path: string) {
  return `${info.sourceUrl.replace("/tree/", "/blob/")}/${path}`;
}
